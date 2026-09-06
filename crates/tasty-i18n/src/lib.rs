#![forbid(unsafe_code)]

//! Shared internationalization (i18n) store.
//!
//! Loads translation strings from the workspace `lang/*.toml` files at startup
//! and exposes the global `t()` lookup. Lives in its own crate (rather than the
//! main binary) so that **both** the GUI/host binary and the `tasty-cli` crate
//! can read the *same* translation table: the CLI runs in-process inside the
//! `tasty` binary, and because the global store is a single `OnceLock` in this
//! shared crate, the `locale::init()` call made on the CLI boot path also makes
//! `t()` work for strings emitted from the `tasty-cli` crate.
//!
//! Language is configured in config.toml `general.language` field. Changing
//! language requires restart.
//!
//! Three sources feed the table (`docs/dev-guide/i18n.md` "언어팩"):
//! - built-in `en` / `ko` / `ja` embedded in the binary ([`BUILTIN_CODES`]);
//! - a single `~/.tasty/lang/<code>.toml` — an **override** for a built-in code only;
//! - a **language pack** directory `~/.tasty/lang/<code>/pack.toml` for any other
//!   code. A pack must carry a `[font]` section ([`FontDecl`]); a missing or
//!   malformed pack makes [`init`] fall back to English and report it
//!   ([`LoadReport`]) so the GUI can warn the user — the setting itself is never
//!   rewritten. Discovery for the settings combo is [`available_languages`].
//!
//! Plugins can dynamically register/unregister translation namespaces via
//! [`register_namespace`] / [`unregister_namespace`]. Namespace strings are
//! `Box::leak`ed to satisfy the `&'static str` lookup contract. The leak is
//! acceptable because the string set is bounded: a plugin's namespace is shipped
//! with the plugin, and a user language pack — whose size is chosen by the user,
//! not by tasty — is capped at [`MAX_PACK_BYTES`] before it is ever parsed.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{OnceLock, RwLock};

use tasty_utils::path::tasty_home;

/// Global translation store, initialized once at startup.
static TRANSLATIONS: OnceLock<Translations> = OnceLock::new();
/// What [`init`] decided at boot (requested vs. effective language). The GUI
/// reads it once after boot to surface the English-fallback warning as a toast.
static LOAD_REPORT: OnceLock<LoadReport> = OnceLock::new();

/// Language codes embedded in the binary (`lang/{code}.toml`, kept in sync with
/// the `build.rs` rerun list). These never need a pack directory.
pub const BUILTIN_CODES: [&str; 3] = ["en", "ko", "ja"];
/// Manifest file name inside a language pack directory.
pub const PACK_FILE_NAME: &str = "pack.toml";

/// Stands in for the language directory when the data root cannot be determined
/// (no `TASTY_HOME`, no home directory). It is a label, not a path that is ever
/// read — without a data root there is no user pack, so the fallback message only
/// has to name the convention.
const UNKNOWN_LANG_DIR: &str = "<tasty home>/lang";

/// Whether `code` is one of the embedded languages.
pub fn is_builtin_code(code: &str) -> bool {
    BUILTIN_CODES.contains(&code)
}

fn builtin_toml(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some(include_str!("../../../lang/en.toml")),
        "ko" => Some(include_str!("../../../lang/ko.toml")),
        "ja" => Some(include_str!("../../../lang/ja.toml")),
        _ => None,
    }
}

/// The user language directory (`~/.tasty/lang`). `None` when no data root can
/// be determined (no home directory).
pub fn user_lang_dir() -> Option<PathBuf> {
    tasty_home().map(|dir| dir.join("lang"))
}

/// Path of the pack manifest for `code` under `lang_dir`.
pub fn pack_path(lang_dir: &Path, code: &str) -> PathBuf {
    lang_dir.join(code).join(PACK_FILE_NAME)
}

/// A language code usable as a directory name, env value and settings value:
/// ASCII letters/digits/`-`/`_`, 1..=32 chars.
fn is_valid_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 32
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ── language pack types ─────────────────────────────────────────────────────

/// The mandatory `[font]` declaration of a language pack. Which glyph source the
/// pack expects; resolving it to an actual font (file lookup, system family
/// match) is the host's job, not this crate's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontDecl {
    /// `builtin = true` — the pack declares that the built-in font stack already
    /// covers its script.
    Builtin,
    /// `file = "<path relative to the pack directory>"` — a bundled font file.
    File(String),
    /// `family = "<system font family name>"`.
    Family(String),
    /// `candidates = ["<file or family>", …]` — tried in order, first hit wins.
    Candidates(Vec<String>),
}

/// Where a listed language comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSource {
    /// Embedded in the binary.
    Builtin,
    /// Embedded, with a user `<code>.toml` override file present.
    BuiltinOverridden,
    /// A user language pack directory (`<code>/pack.toml`).
    Pack,
}

/// One row of the "available languages" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageEntry {
    pub code: String,
    /// `[meta] name` — `None` when the file has no usable name; callers fall
    /// back to the code ([`LanguageEntry::label`]).
    pub display_name: Option<String>,
    pub source: LanguageSource,
    /// The pack's `[font]` declaration. `None` for built-in languages (with or
    /// without an override) — they use the built-in font stack.
    pub font: Option<FontDecl>,
    /// Pack manifest (packs) or override file (overridden built-ins). `None` for
    /// a plain built-in.
    pub path: Option<PathBuf>,
}

impl LanguageEntry {
    /// Display label: the `[meta] name`, else the code itself.
    pub fn label(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.code)
    }
}

/// Why a `pack.toml` was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    Read(String),
    Parse(String),
    /// No `[font]` section — a shape violation, the pack is not loaded at all.
    MissingFont,
    /// `[font]` present but none of `builtin` / `file` / `family` / `candidates`
    /// is usable.
    InvalidFont(String),
    /// The manifest is bigger than [`MAX_PACK_BYTES`]. Rejected before parsing —
    /// see that constant for how the limit is set.
    TooLarge {
        bytes: u64,
        max: u64,
    },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "cannot read: {e}"),
            Self::Parse(e) => write!(f, "invalid TOML: {e}"),
            Self::MissingFont => write!(f, "missing [font] section"),
            Self::InvalidFont(e) => write!(f, "invalid [font]: {e}"),
            Self::TooLarge { bytes, max } => write!(
                f,
                "too large: {} KiB (limit {} KiB)",
                bytes / 1024,
                max / 1024
            ),
        }
    }
}

/// A toast-sized, single-line version of a rejection reason. `toml::de::Error`
/// renders as multi-line ASCII art (source excerpt plus caret rows) and the toast
/// is a summary channel with a hard character cap, so anything past the first line
/// would push the message's instruction out of view. The full text still reaches
/// the log — `Translations::warn_fallback` formats the error unabridged.
fn summarize_reason(error: &PackError) -> String {
    /// Keeps the reason from crowding out the path and the instruction. The path
    /// gets whatever is left ([`fit_path`]), so this budget is deliberately tight:
    /// *which pack* is more actionable than *why* — the log has the full reason.
    const MAX_CHARS: usize = 40;
    let text = error.to_string();
    let first = text.lines().next().unwrap_or_default().trim();
    if first.chars().count() <= MAX_CHARS {
        return first.to_string();
    }
    let mut out: String = first.chars().take(MAX_CHARS).collect();
    out.push('…');
    out
}

/// The toast body cap the host applies (`src/adapters/ui/toast.rs`
/// `MAX_MESSAGE_CHARS`). Going over does not just lose the tail — the host appends
/// a "(character limit)" notice, so the overflow is visible *and* the instruction
/// is gone. The warning below is built to stay under it.
pub const TOAST_MAX_CHARS: usize = 200;

/// Shortest fragment worth showing. Below this the fragment says nothing, so the
/// message keeps the ellipsis and the log carries the real text.
///
/// Public because a test that asserts messages fit the cap has to know the floor:
/// a skeleton that leaves less than this cannot be rescued by shrinking.
pub const MIN_FRAGMENT_CHARS: usize = 12;

/// Render a message that carries one variable-length fragment, shrinking **the
/// fragment only** so the whole message fits [`TOAST_MAX_CHARS`].
///
/// `render` must interpolate its argument exactly once, which lets the budget be
/// exact: everything except the fragment is rendered first (with an empty
/// fragment) and whatever is left of the cap goes to the fragment.
///
/// The fragment is not required to be a path. A failure reason that *contains* a
/// path is the other shape this serves: the host's own truncation drops the tail
/// (`src/adapters/ui/toast.rs` `truncate_message`), and for a reason the tail is
/// the OS error — the part that says *why*. Eliding the middle keeps both ends.
pub fn fit_fragment(fragment: &str, render: impl Fn(&str) -> String) -> String {
    let skeleton = render("").chars().count();
    let budget = TOAST_MAX_CHARS
        .saturating_sub(skeleton)
        .max(MIN_FRAGMENT_CHARS);
    render(&elide_middle(fragment, budget))
}

/// [`fit_fragment`] over a path.
fn fit_path(path: &Path, render: impl Fn(&str) -> String) -> String {
    fit_fragment(&path.display().to_string(), render)
}

/// [`fit_fragment`] for the common single-placeholder case — `t_fmt(key, ..)`
/// with the argument shrunk to fit the toast cap.
pub fn t_fmt_fit(key: &str, fragment: &str) -> String {
    fit_fragment(fragment, |f| t_fmt(key, f))
}

/// Drop the middle of `s` so it fits `max` chars, keeping both ends. The tail
/// keeps roughly two thirds of the budget because it is the half that identifies:
/// for a path it carries `<code>/pack.toml`, i.e. *which* pack; for a failure
/// reason it carries the OS error, i.e. *why*.
fn elide_middle(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "\u{2026}".to_string();
    }
    let keep = max - 1;
    let head = keep / 3;
    let tail = keep - head;
    let chars: Vec<char> = s.chars().collect();
    let mut out: String = chars[..head].iter().collect();
    out.push('\u{2026}');
    out.extend(&chars[count - tail..]);
    out
}

/// A parsed, validated language pack manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePack {
    pub code: String,
    /// The `pack.toml` path.
    pub path: PathBuf,
    /// `[meta] name`, if present and non-empty.
    pub display_name: Option<String>,
    pub font: FontDecl,
    /// Flattened translation strings (everything except the `[font]` table).
    strings: HashMap<String, String>,
}

/// [`flatten_catalog_toml`] 호출 계수기 — **테스트 전용**.
///
/// 발견 경로가 문자열 키를 flatten 하지 않는다는 것을 시간이 아니라 사실로 확인한다.
/// 시간 비교는 부하에 따라 흔들려 회귀 신호가 되지 못한다. 스레드 로컬이라 테스트
/// 하네스가 테스트마다 다른 스레드에서 돌려도 다른 테스트의 flatten 이 섞이지 않는다.
#[cfg(test)]
mod flatten_probe {
    use std::cell::Cell;

    thread_local! {
        static CALLS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn note() {
        CALLS.with(|c| c.set(c.get() + 1));
    }

    pub(super) fn reset() {
        CALLS.with(|c| c.set(0));
    }

    pub(super) fn count() -> usize {
        CALLS.with(Cell::get)
    }
}

/// 카탈로그 TOML 트리를 점 키로 평탄화한다 — `[settings.tab] general = "General"` 이
/// `settings.tab.general` 이 된다. **문자열 leaf 만 카탈로그 항목이고**, 그 밖의 leaf
/// (정수·불리언·배열·날짜)는 키를 만들지 않는다.
///
/// **왜 `pub` 인가.** 이 규칙이 "무엇이 번역 키인가" 의 정의이고, 그 정의를 쓰는 곳이
/// 로더 하나가 아니다 — 정합 가드 `tests/i18n_key_parity.rs` 가 세 언어 파일의 키
/// 집합을 비교하려면 같은 규칙으로 펴야 한다. 그 가드는 원래 같은 재귀를 **자기
/// 파일에 복사해** 두고 주석으로 "로더와 같은 규칙" 이라고 적어 두었는데, 둘을 같게
/// 잡아 주는 것이 그 주석뿐이었다. 로더가 leaf 취급을 바꾸면 가드는 옛 규칙으로 펴고,
/// 그 어긋남은 **가드가 초록인 채로** 생긴다(가드가 못 보는 키는 parity 검사에도 안
/// 올라온다). 그래서 사본을 지우고 이 함수를 부르게 했다 — 규칙이 하나면 갈라질 수 없다.
///
/// 노출은 그 호출을 위한 것이지 plugin·CLI 가 쓰라는 API 가 아니다. 다만 `doc(hidden)`
/// 은 붙이지 않는다 — 무엇이 키가 되는지는 번역 파일을 쓰는 사람이 알아야 하는 규칙이다.
///
/// `map` 에 **누적**한다(덮어쓴다). 로더가 base(en) 위에 로케일을 겹치는 데 그대로 쓴다.
pub fn flatten_catalog_toml(prefix: &str, value: &toml::Value, map: &mut HashMap<String, String>) {
    #[cfg(test)]
    flatten_probe::note();
    match value {
        toml::Value::Table(table) => {
            for (key, val) in table {
                let full_key = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };
                flatten_catalog_toml(&full_key, val, map);
            }
        }
        toml::Value::String(s) => {
            map.insert(prefix.to_string(), s.clone());
        }
        // Ignore non-string leaf values
        _ => {}
    }
}

/// Largest a language file may be. Anything bigger is rejected unparsed, with a
/// `tracing::warn!`, and does not appear in the settings combo.
///
/// **Why 2 MiB.** The largest built-in file is `lang/ja.toml` — 89 KiB for ~1,300
/// keys — and a pack is a translation of that same key set, so a real pack sits in
/// that order of magnitude. This limit is ~23× that. The three things that
/// legitimately inflate a pack compound to roughly 12×, still under the limit: a
/// verbose language (~1.5×), heavy author comments (~2×), and the key set growing
/// several-fold over the app's life (~4×).
///
/// The ceiling is also chosen so that hitting it stays cheap. Discovery cost is
/// proportional to file size, so a pack sitting exactly at the limit costs about
/// what scanning twenty ordinary packs costs — tens of milliseconds, not seconds.
/// So even a deliberately maximal pack cannot make the first open of the settings
/// window noticeably slower. (Order of magnitude on purpose: the absolute number
/// is machine- and profile-dependent, and nothing here depends on its precision.)
///
/// The limit exists because the cost of reading a pack is proportional to a file
/// the *user* placed. Without it a 200 MB file is accepted, read whole into
/// memory, and parsed on the render thread the first time the settings window
/// opens. If a real pack ever legitimately approaches this, raise the number here
/// — the point is that the cost is bounded by something tasty chooses, not by
/// whatever happens to be in `~/.tasty/lang/`.
pub const MAX_PACK_BYTES: u64 = 2 * 1024 * 1024;

/// Read a language file, refusing anything over [`MAX_PACK_BYTES`].
///
/// Enforced with `Read::take` rather than a `metadata()` size check, so an
/// oversized file is never read past the limit and the decision cannot race a
/// file that grows between the check and the read. The size in the error is
/// best-effort (`metadata`, for the message only) — the rejection already stands.
fn read_capped(path: &Path) -> Result<String, PackError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| PackError::Read(e.to_string()))?;
    // 빠른 거부 — 100 MB 짜리를 상한만큼 읽고 나서 버리지 않는다. 판정의 근거는
    // 아래 `take` 이고 이건 순수 최적화다: 파일이 커진 뒤 줄어드는 경합에서
    // 이 검사를 통과하더라도 `take` 가 다시 잡는다.
    if let Ok(meta) = file.metadata()
        && meta.len() > MAX_PACK_BYTES
    {
        return Err(PackError::TooLarge {
            bytes: meta.len(),
            max: MAX_PACK_BYTES,
        });
    }
    let mut text = String::new();
    // 판정 본체 — `metadata()` 크기를 믿지 않고 실제로 읽은 바이트로 자른다.
    // 검사와 읽기 사이에 자라는 파일에도 상한이 성립하고, 크기를 보고하지 않는
    // 파일(procfs 류)도 여기서 걸린다.
    let read = file
        .take(MAX_PACK_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|e| PackError::Read(e.to_string()))?;
    if read as u64 > MAX_PACK_BYTES {
        return Err(PackError::TooLarge {
            bytes: read as u64,
            max: MAX_PACK_BYTES,
        });
    }
    Ok(text)
}

/// Read `path` and parse it as a TOML table, within [`MAX_PACK_BYTES`].
fn parse_manifest(path: &Path) -> Result<toml::Table, PackError> {
    let text = read_capped(path)?;
    let value: toml::Value = text
        .parse()
        .map_err(|e: toml::de::Error| PackError::Parse(e.to_string()))?;
    match value {
        toml::Value::Table(table) => Ok(table),
        _ => Err(PackError::Parse("root is not a table".to_string())),
    }
}

/// What discovery needs from a pack manifest: the combo label and the font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackHead {
    /// `[meta] name`, if present and non-empty.
    pub display_name: Option<String>,
    pub font: FontDecl,
}

/// Extract the two things **discovery** needs from a pack manifest — `[meta] name`
/// and `[font]`. Validation is identical to [`load_pack`], so "listed in the combo"
/// still means "will load".
///
/// This is not a partial read: the file is read whole (under [`MAX_PACK_BYTES`])
/// and the TOML is parsed to the end, exactly as [`load_pack`] does. What it skips
/// is the flatten — walking every string leaf into an owned `HashMap`.
///
/// Kept separate from [`load_pack`] because the settings combo scans every pack in
/// `~/.tasty/lang/` synchronously on the render thread the first time the window
/// opens, and it needs two values out of a file that may hold thousands of keys.
/// Going through `load_pack` there flattened every string leaf into an owned
/// `HashMap` and then dropped it — work proportional to the pack, thrown away.
pub fn load_pack_head(path: &Path) -> Result<PackHead, PackError> {
    let table = parse_manifest(path)?;
    let font = match table.get("font") {
        None => return Err(PackError::MissingFont),
        Some(font) => parse_font_decl(font)?,
    };
    Ok(PackHead {
        display_name: meta_name(&table),
        font,
    })
}

/// Read and validate `path` as the pack manifest of `code`. The `[font]` section
/// is mandatory; `[meta] name` is optional. Every other string leaf becomes a
/// translation key, overlaid on the English base at load time.
///
/// This is the **load** path — it produces the string table. Discovery needs only
/// the two header values and must use [`load_pack_head`] instead.
///
/// A **blank value counts as "not translated"** and is dropped, so the English
/// base shows through instead of a label-less button. A pack author leaving a key
/// empty is the common case and a blank string on screen is hard to trace back to
/// a key; deliberately blank text is rare and can still be written with a
/// zero-width character (U+200B), which the trim does not eat — a no-break space
/// would not do, since `str::trim` takes every Unicode `White_Space` char
/// including U+00A0. Same rule as [`meta_name`] and [`non_empty_str`], which
/// already reject blanks, and as the user override in [`Translations::apply_user_override`].
pub fn load_pack(path: &Path, code: &str) -> Result<LanguagePack, PackError> {
    let mut table = parse_manifest(path)?;
    let font = match table.get("font") {
        None => return Err(PackError::MissingFont),
        Some(font) => parse_font_decl(font)?,
    };
    let display_name = meta_name(&table);
    // 파싱한 테이블을 그대로 소비한다 — 예전에는 `table.clone()` 으로 문서 전체를
    // 한 번 더 복제한 뒤 그 사본에서 `font` 만 지웠다.
    table.remove("font");
    let mut strings = HashMap::new();
    flatten_catalog_toml("", &toml::Value::Table(table), &mut strings);
    drop_blank_values_warned(
        &mut strings,
        &format!("language pack '{code}' at {}", path.display()),
        "fall back to English",
    );
    Ok(LanguagePack {
        code: code.to_string(),
        path: path.to_path_buf(),
        display_name,
        font,
        strings,
    })
}

/// Remove keys whose value is blank (empty or whitespace only) and return how many
/// were dropped. Used by **both** user-authored overlays — a pack ([`load_pack`])
/// and a built-in override ([`Translations::builtin_strings`]) — so the same input
/// means the same thing whichever file it was written in. The dropped key then
/// resolves on the layer below: the English base for a pack, the built-in language
/// for an override. See [`load_pack`] for why blank means "not translated".
fn drop_blank_values(strings: &mut HashMap<String, String>) -> usize {
    let before = strings.len();
    strings.retain(|_, v| !v.trim().is_empty());
    before - strings.len()
}

/// [`drop_blank_values`] plus the one warn line the two load paths owe the user.
///
/// `origin` names the file, `fallback` what shows through instead — that is the
/// only thing that differs between a pack (English base) and a user override (the
/// built-in language). Keeping both callers on one helper is why the rule cannot
/// drift apart again the way it did once.
fn drop_blank_values_warned(map: &mut HashMap<String, String>, origin: &str, fallback: &str) {
    let blank = drop_blank_values(map);
    if blank > 0 {
        tracing::warn!("i18n: {origin} has {blank} blank value(s) — those keys {fallback}");
    }
}

/// `[meta] name`, trimmed; `None` when absent, not a string, or blank.
fn meta_name(table: &toml::Table) -> Option<String> {
    table
        .get("meta")?
        .get("name")?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Precedence when several keys are present: `builtin = true` > `file` >
/// `family` > `candidates`. A `[font]` table with none of them is invalid.
fn parse_font_decl(value: &toml::Value) -> Result<FontDecl, PackError> {
    let Some(table) = value.as_table() else {
        return Err(PackError::InvalidFont("[font] must be a table".to_string()));
    };
    if table.get("builtin").and_then(toml::Value::as_bool) == Some(true) {
        return Ok(FontDecl::Builtin);
    }
    if let Some(file) = table.get("file") {
        return non_empty_str(file, "file").map(FontDecl::File);
    }
    if let Some(family) = table.get("family") {
        return non_empty_str(family, "family").map(FontDecl::Family);
    }
    if let Some(candidates) = table.get("candidates") {
        return parse_font_candidates(candidates).map(FontDecl::Candidates);
    }
    Err(PackError::InvalidFont(
        "expected one of builtin = true, file, family, candidates".to_string(),
    ))
}

fn non_empty_str(value: &toml::Value, key: &str) -> Result<String, PackError> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| PackError::InvalidFont(format!("{key} must be a non-empty string")))
}

fn parse_font_candidates(value: &toml::Value) -> Result<Vec<String>, PackError> {
    let Some(items) = value.as_array() else {
        return Err(PackError::InvalidFont(
            "candidates must be an array of strings".to_string(),
        ));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(non_empty_str(item, "candidates[]")?);
    }
    if out.is_empty() {
        return Err(PackError::InvalidFont("candidates is empty".to_string()));
    }
    Ok(out)
}

// ── load report ─────────────────────────────────────────────────────────────

/// How the requested language was resolved at load time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome {
    /// Built-in language, no user override file.
    Builtin,
    /// Built-in language with `<code>.toml` overlaid.
    BuiltinOverridden { path: PathBuf },
    /// User language pack loaded.
    Pack { path: PathBuf, font: FontDecl },
    /// Non-built-in code with no `<code>/pack.toml` — English was used instead.
    PackMissing { expected: PathBuf },
    /// `pack.toml` exists but was refused — English was used instead.
    PackInvalid { path: PathBuf, error: PackError },
}

/// Result of [`init`]: what was asked for, what is actually active, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadReport {
    /// The code from `general.language` (already normalized by the caller).
    pub requested: String,
    /// The language the table was built for — equals `requested` unless the
    /// pack was missing/invalid, in which case it is `"en"`.
    pub effective: String,
    pub outcome: LoadOutcome,
}

impl LoadReport {
    /// `true` when the requested language could not be loaded and English is
    /// active instead. The setting value is untouched either way.
    pub fn fell_back(&self) -> bool {
        self.requested != self.effective
    }

    /// User-facing warning for a fallback (`None` when the requested language is
    /// active). Rendered through the *effective* table, so it reads in English —
    /// the requested language had no strings to say it in.
    pub fn user_warning(&self) -> Option<String> {
        match &self.outcome {
            LoadOutcome::PackMissing { expected } => Some(fit_path(expected, |p| {
                t_fmt2("i18n.warn.pack_missing", &self.requested, p)
            })),
            LoadOutcome::PackInvalid { path, error } => {
                let reason = summarize_reason(error);
                Some(fit_path(path, |p| {
                    t_args("i18n.warn.pack_invalid", &[&self.requested, p, &reason])
                }))
            }
            LoadOutcome::Builtin
            | LoadOutcome::BuiltinOverridden { .. }
            | LoadOutcome::Pack { .. } => None,
        }
    }
}

// ── translation store ───────────────────────────────────────────────────────

/// `namespaces` RwLock 의 poison 을 보고했는가(첫 1 회만).
///
/// 임계구역이 `HashMap` insert/remove/조회뿐이라 락을 든 채 죽은 스레드가 불변식을
/// 깨지 않는다 — 복구가 맞다. 조용히 삼키면(`if let Ok`) read 쪽은 번역 조회가 통째로
/// 건너뛰어져 사용자가 키를 그대로 보고, write 쪽은 namespace 등록/해제가 무음으로
/// 유실돼 그 plugin 의 문자열이 전부 키로 노출된다.
static NAMESPACES_POISONED: AtomicBool = AtomicBool::new(false);
const NAMESPACES_WHAT: &str = "i18n plugin namespace overlays";

pub struct Translations {
    /// Built-in + user override strings. Frozen after `init`.
    base: HashMap<String, &'static str>,
    /// Per-plugin namespace overlays. Looked up after `base` misses.
    /// Iteration order is not stable; collisions across namespaces resolve
    /// to whichever shows up first in the iteration — plugins should prefix
    /// their keys with their plugin id to avoid collisions in practice.
    namespaces: RwLock<HashMap<String, HashMap<String, &'static str>>>,
    /// Active language code; used by `register_namespace` to decide which
    /// language file to load from a plugin's lang dir.
    language: String,
}

impl Translations {
    /// Load translations for the given language code. Built-in codes overlay
    /// their embedded file (and a user `<code>.toml` override) on the English
    /// base; any other code needs a pack at `~/.tasty/lang/<code>/pack.toml`
    /// and falls back to English when it is missing or invalid — the returned
    /// [`LoadReport`] says which happened.
    fn load(language: &str) -> (Self, LoadReport) {
        Self::load_from(language, user_lang_dir().as_deref())
    }

    fn load_from(requested: &str, lang_dir: Option<&Path>) -> (Self, LoadReport) {
        if is_builtin_code(requested) {
            let (strings, outcome) = Self::builtin_strings(requested, lang_dir);
            return (
                Self::from_strings(strings, requested),
                LoadReport {
                    requested: requested.to_string(),
                    effective: requested.to_string(),
                    outcome,
                },
            );
        }
        match Self::pack_strings(requested, lang_dir) {
            Ok((strings, outcome)) => (
                Self::from_strings(strings, requested),
                LoadReport {
                    requested: requested.to_string(),
                    effective: requested.to_string(),
                    outcome,
                },
            ),
            Err(outcome) => {
                Self::warn_fallback(requested, lang_dir, &outcome);
                let (strings, _) = Self::builtin_strings("en", lang_dir);
                (
                    Self::from_strings(strings, "en"),
                    LoadReport {
                        requested: requested.to_string(),
                        effective: "en".to_string(),
                        outcome,
                    },
                )
            }
        }
    }

    /// The one diagnostic line for a fallback. This is the whole notification on
    /// headless/CLI paths; the GUI adds a toast from [`LoadReport::user_warning`].
    fn warn_fallback(requested: &str, lang_dir: Option<&Path>, outcome: &LoadOutcome) {
        match outcome {
            LoadOutcome::PackMissing { expected } => {
                Self::warn_pack_missing(requested, lang_dir, expected);
            }
            LoadOutcome::PackInvalid { path, error } => tracing::warn!(
                "i18n: language pack '{requested}' at {} rejected ({error}) — falling back to English (setting left unchanged)",
                path.display()
            ),
            LoadOutcome::Builtin
            | LoadOutcome::BuiltinOverridden { .. }
            | LoadOutcome::Pack { .. } => {}
        }
    }

    /// A stray single `<code>.toml` next to the missing pack is the likeliest
    /// mistake (the old single-file layout), so the line points at it.
    fn warn_pack_missing(requested: &str, lang_dir: Option<&Path>, expected: &Path) {
        let stray = lang_dir
            .map(|d| d.join(format!("{requested}.toml")))
            .filter(|p| p.is_file());
        match stray {
            Some(stray) => tracing::warn!(
                "i18n: language '{requested}' has no language pack at {} — falling back to English (setting left unchanged). {} is a single file, which only overrides a built-in language; a new language needs <code>/{PACK_FILE_NAME}",
                expected.display(),
                stray.display()
            ),
            None => tracing::warn!(
                "i18n: language '{requested}' has no language pack at {} — falling back to English (setting left unchanged)",
                expected.display()
            ),
        }
    }

    /// English base → embedded `code` overlay → user `<code>.toml` override.
    fn builtin_strings(
        code: &str,
        lang_dir: Option<&Path>,
    ) -> (HashMap<String, String>, LoadOutcome) {
        let mut strings = Self::english_base();
        if code != "en"
            && let Some(toml_str) = builtin_toml(code)
        {
            Self::parse_toml_into(&mut strings, toml_str);
        }
        let outcome =
            match lang_dir.and_then(|dir| Self::apply_user_override(&mut strings, dir, code)) {
                Some(path) => LoadOutcome::BuiltinOverridden { path },
                None => LoadOutcome::Builtin,
            };
        (strings, outcome)
    }

    /// Overlay `<dir>/<code>.toml` onto `strings`. Returns the path when the file
    /// was there and parsed — a missing or broken file leaves `strings` untouched.
    ///
    /// The overlay obeys the same **blank means "not translated"** rule as a pack
    /// ([`load_pack`]) — the reason is the layer below, not the file shape: a blank
    /// value that reaches the table paints a label-less button, and nothing on
    /// screen says which key emptied it. What shows through differs, though. A pack
    /// sits on the English base, so a dropped key falls back to English; an
    /// override sits on the built-in `code` overlay, so a dropped key keeps **that
    /// language's** own text.
    fn apply_user_override(
        strings: &mut HashMap<String, String>,
        dir: &Path,
        code: &str,
    ) -> Option<PathBuf> {
        let path = dir.join(format!("{code}.toml"));
        let value = match read_user_toml(&path) {
            Ok(Some(value)) => value,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!(
                    "i18n: user override {} ignored — {e} (built-in '{code}' strings stay active)",
                    path.display()
                );
                return None;
            }
        };
        // 오버레이를 따로 모아 빈 값을 걷어낸 뒤 합친다 — `strings` 에 바로 flatten
        // 하면 빈 값이 내장 문자열을 이미 덮어써서 되돌릴 수 없다.
        let mut overlay = HashMap::new();
        flatten_catalog_toml("", &value, &mut overlay);
        drop_blank_values_warned(
            &mut overlay,
            &format!("user override {}", path.display()),
            &format!("keep the built-in '{code}' text"),
        );
        strings.extend(overlay);
        tracing::info!("loaded user translations from {}", path.display());
        Some(path)
    }

    /// English base → pack strings. `Err` carries the fallback reason.
    fn pack_strings(
        code: &str,
        lang_dir: Option<&Path>,
    ) -> Result<(HashMap<String, String>, LoadOutcome), LoadOutcome> {
        let Some(dir) = lang_dir else {
            // 홈(데이터 루트)을 못 구하면 사용자 언어팩도 없다. 프로세스 CWD 의 `lang/`
            // 을 읽으면 tasty 를 어디서 띄웠는지에 따라 UI 문자열이 바뀌고, CWD 를 보지
            // 않는 `scan_languages` 와 규칙이 갈려 "목록에 있으면 로드된다" 가 깨진다.
            return Err(LoadOutcome::PackMissing {
                expected: pack_path(Path::new(UNKNOWN_LANG_DIR), code),
            });
        };
        let path = pack_path(dir, code);
        if !is_valid_code(code) || !path.is_file() {
            return Err(LoadOutcome::PackMissing { expected: path });
        }
        match load_pack(&path, code) {
            Ok(pack) => {
                let mut strings = Self::english_base();
                strings.extend(pack.strings);
                tracing::info!(
                    "i18n: loaded language pack '{code}' from {} (font: {:?})",
                    path.display(),
                    pack.font
                );
                Ok((
                    strings,
                    LoadOutcome::Pack {
                        path,
                        font: pack.font,
                    },
                ))
            }
            Err(error) => Err(LoadOutcome::PackInvalid { path, error }),
        }
    }

    fn english_base() -> HashMap<String, String> {
        let mut strings = HashMap::new();
        if let Some(en) = builtin_toml("en") {
            Self::parse_toml_into(&mut strings, en);
        }
        strings
    }

    fn from_strings(strings: HashMap<String, String>, language: &str) -> Self {
        tracing::info!(
            "i18n: loaded {} strings for language '{}'",
            strings.len(),
            language
        );
        let base: HashMap<String, &'static str> =
            strings.into_iter().map(|(k, v)| (k, leak_str(v))).collect();
        Self {
            base,
            namespaces: RwLock::new(HashMap::new()),
            language: language.to_string(),
        }
    }

    /// Parse a TOML string with nested tables into flat dotted keys.
    /// e.g., [settings.tab] general = "General" -> "settings.tab.general" = "General"
    fn parse_toml_into(map: &mut HashMap<String, String>, toml_str: &str) {
        if let Ok(value) = toml_str.parse::<toml::Value>() {
            flatten_catalog_toml("", &value, map);
        }
    }

    /// Get a translated string by key. Falls back to the key itself if not found.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(s) = self.base.get(key) {
            return s;
        }
        let ns = tasty_utils::poison::recover_read(
            self.namespaces.read(),
            NAMESPACES_WHAT,
            &NAMESPACES_POISONED,
        );
        for map in ns.values() {
            if let Some(s) = map.get(key) {
                return s;
            }
        }
        drop(ns);
        key
    }

    /// Get a translated string with a format argument replacing `{}`.
    pub fn get_fmt(&self, key: &str, arg: &str) -> String {
        let template = self.get(key);
        template.replace("{}", arg)
    }

    /// Replace `{}` with `arg1`, `arg2` in order (one occurrence each).
    /// Unlike `get_fmt`, this uses `replacen(_, 1)` so only the first two `{}` placeholders are replaced.
    pub fn get_fmt2(&self, key: &str, arg1: &str, arg2: &str) -> String {
        let template = self.get(key);
        let first = template.replacen("{}", arg1, 1);
        first.replacen("{}", arg2, 1)
    }

    /// Replace each `{}` placeholder with the corresponding entry in `args`,
    /// in order (one occurrence per arg). Generalizes [`get_fmt`]/[`get_fmt2`]
    /// for strings with three or more interpolated values — common in CLI
    /// output (e.g. `remote check` alive lines carrying host/port/version).
    pub fn get_args(&self, key: &str, args: &[&str]) -> String {
        let mut out = self.get(key).to_string();
        for arg in args {
            out = out.replacen("{}", arg, 1);
        }
        out
    }

    /// Register a plugin namespace. `lang_dir` is expected to contain
    /// `<lang>.toml` files — `en.toml` is loaded as the base, then the
    /// active language file is overlaid on top.
    ///
    /// If `namespace` was previously registered, its entries are replaced.
    pub fn register_namespace(&self, namespace: &str, lang_dir: &Path) {
        let mut strings: HashMap<String, String> = HashMap::new();

        // English fallback first.
        let en_path = lang_dir.join("en.toml");
        if let Ok(s) = std::fs::read_to_string(&en_path) {
            Self::parse_toml_into(&mut strings, &s);
        }

        if self.language != "en" {
            let lang_path = lang_dir.join(format!("{}.toml", self.language));
            if let Ok(s) = std::fs::read_to_string(&lang_path) {
                // 활성 언어는 **덮어쓰기**라 빈 값을 그대로 얹으면 바로 위 en 문자열을
                // 지운다. 팩·내장 오버라이드와 같은 규칙을 여기에도 건다(ADR-0124):
                // 빈 값은 번역 없음이고, 아래 층인 그 plugin 의 영어가 보인다.
                let mut overlay = HashMap::new();
                Self::parse_toml_into(&mut overlay, &s);
                drop_blank_values_warned(
                    &mut overlay,
                    &format!("plugin lang file {}", lang_path.display()),
                    "fall back to the plugin's English string",
                );
                strings.extend(overlay);
            }
        }

        let leaked: HashMap<String, &'static str> =
            strings.into_iter().map(|(k, v)| (k, leak_str(v))).collect();

        let count = leaked.len();
        tasty_utils::poison::recover_write(
            self.namespaces.write(),
            NAMESPACES_WHAT,
            &NAMESPACES_POISONED,
        )
        .insert(namespace.to_string(), leaked);
        tracing::info!(
            "i18n: registered namespace '{}' with {} strings (lang_dir={})",
            namespace,
            count,
            lang_dir.display()
        );
    }

    /// Remove a previously registered namespace. Strings remain in memory
    /// (`Box::leak`) but are no longer reachable through `get`.
    pub fn unregister_namespace(&self, namespace: &str) {
        tasty_utils::poison::recover_write(
            self.namespaces.write(),
            NAMESPACES_WHAT,
            &NAMESPACES_POISONED,
        )
        .remove(namespace);
        tracing::info!("i18n: unregistered namespace '{}'", namespace);
    }
}

/// Read a user TOML file. `Ok(None)` when it does not exist; `Err` for any
/// other I/O failure or a parse error (the caller decides how loud to be).
///
/// Subject to the same [`MAX_PACK_BYTES`] limit as a pack — a built-in override
/// (`~/.tasty/lang/<builtin>.toml`) is a user-placed file of the same shape, so
/// leaving it uncapped would just move the unbounded read one path over.
fn read_user_toml(path: &Path) -> Result<Option<toml::Value>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = match read_capped(path) {
        Ok(text) => text,
        // `exists()` 와 열기 사이에 사라졌다면 없는 것과 같게 다룬다.
        Err(PackError::Read(_)) if !path.exists() => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    text.parse::<toml::Value>()
        .map(Some)
        .map_err(|e| format!("invalid TOML: {e}"))
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

// ── discovery ───────────────────────────────────────────────────────────────

/// Languages the settings combo can offer: the built-ins (marking those with a
/// user override) followed by every valid pack under `~/.tasty/lang/`, sorted by
/// code. Same rules as [`init`], so what is listed is what will load.
pub fn available_languages() -> Vec<LanguageEntry> {
    scan_languages(user_lang_dir().as_deref())
}

/// [`available_languages`] over an explicit directory (`None` = no user dir).
///
/// - a directory with `pack.toml` is a pack; parse failure or a missing/invalid
///   `[font]` excludes it with a `tracing::warn!`;
/// - a directory named after a built-in code is not a pack (built-ins are
///   overridden by a single file) — excluded with a warning;
/// - a single `<code>.toml` marks a built-in as overridden; for any other code
///   it is not a pack and is ignored with a warning.
pub fn scan_languages(lang_dir: Option<&Path>) -> Vec<LanguageEntry> {
    let mut out: Vec<LanguageEntry> = BUILTIN_CODES
        .iter()
        .map(|code| builtin_entry(code, lang_dir))
        .collect();
    if let Some(dir) = lang_dir {
        out.extend(scan_packs(dir));
    }
    out
}

fn builtin_entry(code: &str, lang_dir: Option<&Path>) -> LanguageEntry {
    let mut entry = LanguageEntry {
        code: code.to_string(),
        display_name: builtin_toml(code)
            .and_then(|s| s.parse::<toml::Value>().ok())
            .and_then(|v| v.as_table().and_then(meta_name)),
        source: LanguageSource::Builtin,
        font: None,
        path: None,
    };
    let Some(dir) = lang_dir else {
        return entry;
    };
    let path = dir.join(format!("{code}.toml"));
    match read_user_toml(&path) {
        Ok(Some(value)) => {
            if let Some(name) = value.as_table().and_then(meta_name) {
                entry.display_name = Some(name);
            }
            entry.source = LanguageSource::BuiltinOverridden;
            entry.path = Some(path);
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(
            "i18n: user override {} ignored — {e} (listed as plain built-in '{code}')",
            path.display()
        ),
    }
    entry
}

fn scan_packs(dir: &Path) -> Vec<LanguageEntry> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("i18n: cannot scan {}: {e}", dir.display());
            }
            return Vec::new();
        }
    };
    let mut packs = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => scan_dir_entry(dir, &entry.path(), &mut packs),
            Err(e) => {
                tracing::warn!("i18n: skipping unreadable entry in {}: {e}", dir.display());
            }
        }
    }
    packs.sort_by(|a, b| a.code.cmp(&b.code));
    packs
}

/// One `~/.tasty/lang/` entry: a directory may be a pack; a single `<code>.toml`
/// is only meaningful for built-in codes (anything else gets a warning).
fn scan_dir_entry(dir: &Path, path: &Path, packs: &mut Vec<LanguageEntry>) {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    if path.is_dir() {
        if let Some(pack) = pack_entry(path, name) {
            packs.push(pack);
        }
    } else if let Some(stem) = single_file_code(path)
        && !is_builtin_code(stem)
    {
        tracing::warn!(
            "i18n: {} ignored — a single <code>.toml only overrides a built-in language; a new language '{stem}' needs {}",
            path.display(),
            pack_path(dir, stem).display()
        );
    }
}

/// `<code>` for a `<code>.toml` file path, else `None`.
fn single_file_code(path: &Path) -> Option<&str> {
    if path.extension()? != "toml" {
        return None;
    }
    path.file_stem()?.to_str()
}

/// Why a `<code>/pack.toml` directory cannot be listed as a pack, if any.
fn pack_dir_rejection(code: &str) -> Option<String> {
    if !is_valid_code(code) {
        Some(format!("'{code}' is not a usable language code"))
    } else if is_builtin_code(code) {
        Some(format!(
            "built-in '{code}' is overridden by {code}.toml, not by a pack"
        ))
    } else {
        None
    }
}

fn pack_entry(dir: &Path, code: &str) -> Option<LanguageEntry> {
    let path = dir.join(PACK_FILE_NAME);
    if !path.is_file() {
        return None;
    }
    if let Some(reason) = pack_dir_rejection(code) {
        tracing::warn!("i18n: {} ignored — {reason}", path.display());
        return None;
    }
    // 발견은 `[meta]`/`[font]` 만 필요하다 — 문자열 키를 flatten 하지 않는다.
    match load_pack_head(&path) {
        Ok(head) => Some(LanguageEntry {
            code: code.to_string(),
            display_name: head.display_name,
            source: LanguageSource::Pack,
            font: Some(head.font),
            path: Some(path),
        }),
        Err(e) => {
            tracing::warn!(
                "i18n: language pack {} rejected ({e}) — not listed",
                path.display()
            );
            None
        }
    }
}

// ── global API ──────────────────────────────────────────────────────────────

/// Initialize the global translation store. Call once at startup. Returns what
/// was loaded; on a missing/invalid language pack the table is English and the
/// report says so ([`LoadReport::fell_back`]). A second call is a no-op that
/// returns the first report.
pub fn init(language: &str) -> LoadReport {
    let (translations, report) = Translations::load(language);
    // OnceLock::set은 이미 set된 경우만 Err. i18n은 부팅 시 1회만 호출되는 시드
    // 데이터라 두 번째 호출은 의도적 no-op — Err를 panic이나 로그 없이 그대로 무시.
    let _already_set: Result<_, _> = TRANSLATIONS.set(translations);
    LOAD_REPORT.get_or_init(|| report).clone()
}

/// The report [`init`] produced at boot; `None` before `init`.
pub fn load_report() -> Option<&'static LoadReport> {
    LOAD_REPORT.get()
}

/// Get a translated string by key.
/// Shorthand for accessing the global store.
pub fn t(key: &str) -> &str {
    TRANSLATIONS.get().map(|tr| tr.get(key)).unwrap_or(key)
}

/// 활성 language code — [`init`] 이 실제로 적용한 언어(언어팩 부재로 영어 폴백이 일어났으면
/// `"en"`, 요청 코드는 [`load_report`]). 미초기화면 `"en"` fallback.
/// 호스트가 plugin spawn 시 `TASTY_LOCALE` 환경변수로 전달하는 등에 사용.
pub fn current_language() -> &'static str {
    TRANSLATIONS
        .get()
        .map(|tr| tr.language.as_str())
        .unwrap_or("en")
}

/// Get a translated string with a format argument.
pub fn t_fmt(key: &str, arg: &str) -> String {
    TRANSLATIONS
        .get()
        .map(|tr| tr.get_fmt(key, arg))
        .unwrap_or_else(|| key.replace("{}", arg))
}

/// Get a translated string with two format arguments replacing the first two `{}` placeholders in order.
pub fn t_fmt2(key: &str, arg1: &str, arg2: &str) -> String {
    TRANSLATIONS
        .get()
        .map(|tr| tr.get_fmt2(key, arg1, arg2))
        .unwrap_or_else(|| key.replacen("{}", arg1, 1).replacen("{}", arg2, 1))
}

/// Get a translated string with N format arguments replacing `{}` placeholders in order.
/// Falls back to substituting into the raw key if the store is not initialized.
pub fn t_args(key: &str, args: &[&str]) -> String {
    TRANSLATIONS
        .get()
        .map(|tr| tr.get_args(key, args))
        .unwrap_or_else(|| {
            let mut out = key.to_string();
            for arg in args {
                out = out.replacen("{}", arg, 1);
            }
            out
        })
}

/// Register a plugin's translation namespace. No-op if `init` has not been
/// called yet (translations not initialized).
pub fn register_namespace(namespace: &str, lang_dir: &Path) {
    if let Some(tr) = TRANSLATIONS.get() {
        tr.register_namespace(namespace, lang_dir);
    } else {
        tracing::warn!(
            "i18n: register_namespace('{}') called before init — ignored",
            namespace
        );
    }
}

/// Unregister a plugin's translation namespace.
pub fn unregister_namespace(namespace: &str) {
    if let Some(tr) = TRANSLATIONS.get() {
        tr.unregister_namespace(namespace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트 전용 임시 디렉토리 (프로세스 id + nanos 로 유일).
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tasty-i18n-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: PathBuf, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    const XX_PACK: &str =
        "[meta]\nname = \"Xx Lang\"\n\n[font]\nbuiltin = true\n\n[button]\nok = \"OKAY-XX\"\n";
    const YY_PACK_NO_FONT: &str = "[meta]\nname = \"Yy\"\n\n[button]\nok = \"YY\"\n";
    const BAD_PACK: &str = "[font\nbuiltin = true\n";

    /// 완료 확인 시나리오: xx(정상 팩) · yy(font 없음) · ko.toml(내장 오버라이드) ·
    /// zz.toml(단일 파일 새 코드) · bad(깨진 TOML) → 목록에는 xx 만 팩으로 오르고 ko 는
    /// builtin+override, yy/zz/bad 는 제외.
    fn scenario_dir() -> PathBuf {
        let dir = temp_dir("scenario");
        write(dir.join("xx").join("pack.toml"), XX_PACK);
        write(dir.join("yy").join("pack.toml"), YY_PACK_NO_FONT);
        write(
            dir.join("ko.toml"),
            "[meta]\nname = \"한국어 (custom)\"\n\n[button]\nok = \"확인!\"\n",
        );
        write(dir.join("zz.toml"), "[button]\nok = \"ZZ\"\n");
        write(dir.join("bad").join("pack.toml"), BAD_PACK);
        dir
    }

    #[test]
    fn scan_lists_builtins_overrides_and_valid_packs_only() {
        let dir = scenario_dir();
        let list = scan_languages(Some(&dir));
        let codes: Vec<&str> = list.iter().map(|l| l.code.as_str()).collect();
        assert_eq!(codes, ["en", "ko", "ja", "xx"]);

        let en = &list[0];
        assert_eq!(en.source, LanguageSource::Builtin);
        assert_eq!(en.display_name.as_deref(), Some("English"));
        assert_eq!(en.font, None);
        assert_eq!(en.path, None);

        let ko = &list[1];
        assert_eq!(ko.source, LanguageSource::BuiltinOverridden);
        assert_eq!(ko.display_name.as_deref(), Some("한국어 (custom)"));
        assert_eq!(ko.path.as_deref(), Some(dir.join("ko.toml").as_path()));
        assert_eq!(ko.font, None);

        let xx = &list[3];
        assert_eq!(xx.source, LanguageSource::Pack);
        assert_eq!(xx.label(), "Xx Lang");
        assert_eq!(xx.font, Some(FontDecl::Builtin));
        assert_eq!(xx.path.as_deref(), Some(pack_path(&dir, "xx").as_path()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_without_a_data_root_never_reads_a_cwd_relative_pack() {
        // 데이터 루트를 못 구하면 사용자 팩도 없다. 프로세스 CWD 의 `lang/<code>/pack.toml`
        // 을 읽으면 실행 위치가 UI 문자열을 바꾸고, CWD 를 보지 않는 목록 API 와 규칙이
        // 갈려 "목록에 있으면 로드된다" 가 깨진다.
        let (tr, report) = Translations::load_from("qq", None);
        assert_eq!(tr.language, "en");
        let LoadOutcome::PackMissing { expected } = &report.outcome else {
            panic!("expected PackMissing, got {:?}", report.outcome);
        };
        assert!(expected.starts_with(UNKNOWN_LANG_DIR), "{expected:?}");
        // 목록도 같은 규칙 — 내장 외에는 아무것도 보이지 않는다.
        assert!(scan_languages(None).iter().all(|l| l.path.is_none()));
    }

    #[test]
    fn toast_reason_is_a_single_short_line() {
        // toml 파서 원문은 여러 줄 ASCII 아트라 그대로 토스트에 넣으면 200자 상한에서
        // 행동 지시가 잘린다. 요약은 첫 줄만, 상세는 로그(`warn_fallback`)로 간다.
        let dir = scenario_dir();
        let (_, report) = Translations::load_from("bad", Some(&dir));
        let LoadOutcome::PackInvalid { error, .. } = &report.outcome else {
            panic!("expected PackInvalid, got {:?}", report.outcome);
        };
        assert!(error.to_string().lines().count() > 1);
        let summary = summarize_reason(error);
        assert!(!summary.contains('\n'), "{summary}");
        assert!(summary.chars().count() <= 61, "{summary}");
        assert!(summary.starts_with("invalid TOML:"), "{summary}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn elide_middle_keeps_both_ends_and_marks_the_cut() {
        assert_eq!(elide_middle("short", 10), "short");
        let out = elide_middle("/home/someone/.tasty/lang/zz/pack.toml", 20);
        assert_eq!(out.chars().count(), 20);
        assert!(out.contains('\u{2026}'));
        assert!(
            out.starts_with("/home"),
            "머리(어느 루트인지)가 남아야 한다: {out}"
        );
        assert!(
            out.ends_with("zz/pack.toml"),
            "꼬리(어느 팩인지)가 남아야 한다: {out}"
        );
    }

    /// 토스트가 200자 캡에 잘리면 호스트가 "(character limit)" 접미를 붙이고 **경로가
    /// 사라진다**. 경로만 가운데를 생략해 캡 안에 들어가야 한다.
    #[test]
    fn warning_with_a_very_long_path_stays_under_the_toast_cap() {
        let deep: String = std::iter::repeat_n("verylongsegment", 40)
            .collect::<Vec<_>>()
            .join("/");
        let path = PathBuf::from(format!("/{deep}/zz/pack.toml"));
        assert!(path.display().to_string().chars().count() > 500);

        let msg = fit_path(&path, |p| {
            format!(
                "Language 'zz' is not installed — showing English. Put a language pack at {p}, then restart."
            )
        });
        assert!(
            msg.chars().count() <= TOAST_MAX_CHARS,
            "{} chars: {msg}",
            msg.chars().count()
        );
        assert!(
            msg.contains("zz/pack.toml"),
            "어느 팩인지는 남아야 한다: {msg}"
        );
        assert!(
            msg.ends_with("then restart."),
            "문장 끝(할 일)이 살아 있어야 한다"
        );
    }

    /// 실패 사유는 경로가 아니라 **경로를 품은 문장**이다. 호스트의 잘림은 꼬리를
    /// 버리는데(`truncate_message`), 사유의 꼬리가 바로 OS 에러 — 왜 실패했는지다.
    /// 가운데 생략은 어느 파일인지(머리)와 왜인지(꼬리)를 둘 다 남긴다.
    #[test]
    fn a_long_failure_reason_keeps_both_the_target_and_the_os_error() {
        let deep: String = std::iter::repeat_n("verylongsegment", 40)
            .collect::<Vec<_>>()
            .join("/");
        let reason = format!("write /{deep}/.tasty/bashrc.user: Access is denied. (os error 5)");
        assert!(reason.chars().count() > 500);

        let msg = fit_fragment(&reason, |r| format!("Could not save the edit: {r}"));
        assert!(
            msg.chars().count() <= TOAST_MAX_CHARS,
            "{} chars: {msg}",
            msg.chars().count()
        );
        assert!(msg.starts_with("Could not save the edit: write /"), "{msg}");
        assert!(msg.ends_with("(os error 5)"), "왜인지가 남아야 한다: {msg}");
    }

    #[test]
    fn short_path_is_not_elided() {
        let path = PathBuf::from("/home/u/.tasty/lang/zz/pack.toml");
        let msg = fit_path(&path, |p| format!("pack at {p}"));
        assert_eq!(msg, "pack at /home/u/.tasty/lang/zz/pack.toml");
    }

    #[test]
    fn scan_without_user_dir_lists_builtins_with_meta_names() {
        let list = scan_languages(None);
        let labels: Vec<&str> = list.iter().map(LanguageEntry::label).collect();
        assert_eq!(labels, ["English", "한국어", "日本語"]);
        assert!(list.iter().all(|l| l.source == LanguageSource::Builtin));
    }

    #[test]
    fn scan_pack_without_meta_name_labels_by_code() {
        let dir = temp_dir("nometa");
        write(
            dir.join("qq").join("pack.toml"),
            "[font]\nfamily = \"Noto Sans\"\n\n[button]\nok = \"Q\"\n",
        );
        // 내장 코드 이름의 디렉토리는 팩이 아니다 — 목록에 오르지 않는다.
        write(dir.join("ko").join("pack.toml"), XX_PACK);
        let list = scan_languages(Some(&dir));
        let qq = list.iter().find(|l| l.code == "qq").expect("qq listed");
        assert_eq!(qq.display_name, None);
        assert_eq!(qq.label(), "qq");
        assert_eq!(qq.font, Some(FontDecl::Family("Noto Sans".to_string())));
        assert_eq!(list.iter().filter(|l| l.code == "ko").count(), 1);
        assert_eq!(
            list.iter().find(|l| l.code == "ko").unwrap().source,
            LanguageSource::Builtin
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_pack_accepts_every_font_shape_and_rejects_the_rest() {
        let dir = temp_dir("font");
        let cases: &[(&str, Result<FontDecl, PackError>)] = &[
            ("[font]\nbuiltin = true\n", Ok(FontDecl::Builtin)),
            (
                "[font]\nfile = \"fonts/x.ttf\"\n",
                Ok(FontDecl::File("fonts/x.ttf".to_string())),
            ),
            (
                "[font]\nfamily = \"Noto Sans\"\n",
                Ok(FontDecl::Family("Noto Sans".to_string())),
            ),
            (
                "[font]\ncandidates = [\"fonts/x.ttf\", \"Noto Sans\"]\n",
                Ok(FontDecl::Candidates(vec![
                    "fonts/x.ttf".to_string(),
                    "Noto Sans".to_string(),
                ])),
            ),
            // builtin = true 가 다른 키보다 우선한다.
            (
                "[font]\nbuiltin = true\nfile = \"fonts/x.ttf\"\n",
                Ok(FontDecl::Builtin),
            ),
            ("[button]\nok = \"x\"\n", Err(PackError::MissingFont)),
            (
                "[font]\nbuiltin = false\n",
                Err(PackError::InvalidFont(
                    "expected one of builtin = true, file, family, candidates".to_string(),
                )),
            ),
            (
                "[font]\nfile = \"\"\n",
                Err(PackError::InvalidFont(
                    "file must be a non-empty string".to_string(),
                )),
            ),
            (
                "[font]\ncandidates = []\n",
                Err(PackError::InvalidFont("candidates is empty".to_string())),
            ),
            (
                "font = \"x\"\n",
                Err(PackError::InvalidFont("[font] must be a table".to_string())),
            ),
        ];
        for (i, (content, expected)) in cases.iter().enumerate() {
            let path = dir.join(format!("c{i}")).join("pack.toml");
            write(path.clone(), content);
            let got = load_pack(&path, "c").map(|p| p.font);
            assert_eq!(&got, expected, "case {i}: {content:?}");
        }
        assert!(matches!(
            load_pack(&dir.join("nope").join("pack.toml"), "nope"),
            Err(PackError::Read(_))
        ));
        write(dir.join("bad").join("pack.toml"), BAD_PACK);
        assert!(matches!(
            load_pack(&dir.join("bad").join("pack.toml"), "bad"),
            Err(PackError::Parse(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_pack_overlays_strings_on_english_base() {
        let dir = scenario_dir();
        let (tr, report) = Translations::load_from("xx", Some(&dir));
        assert_eq!(tr.get("button.ok"), "OKAY-XX");
        // 팩에 없는 키는 영어 베이스.
        assert_eq!(tr.get("app.name"), "Tasty");
        // [meta] 는 문자열 키로도 조회된다, [font] 는 문자열 테이블에 들어가지 않는다.
        assert_eq!(tr.get("meta.name"), "Xx Lang");
        assert_eq!(tr.get("font.builtin"), "font.builtin");
        assert_eq!(tr.language, "xx");
        assert_eq!(report.requested, "xx");
        assert_eq!(report.effective, "xx");
        assert!(!report.fell_back());
        assert_eq!(
            report.outcome,
            LoadOutcome::Pack {
                path: pack_path(&dir, "xx"),
                font: FontDecl::Builtin,
            }
        );
        assert_eq!(report.user_warning(), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 같은 입력을 **두 로드 경로에 각각** 넣어 결과가 갈리지 않는지 본다.
    ///
    /// 한쪽만 검사하면 이 결함을 못 잡는다 — 실제로 팩 쪽에는 가드가 있었는데
    /// override 쪽에는 없어서 같은 `key = ""` 가 한쪽에선 폴백, 다른 쪽에선 화면의
    /// 빈칸이 됐다. 두 경로의 **폴백 대상은 다르다**(팩=영어 베이스, override=그 내장
    /// 언어). 같아야 하는 것은 "빈 값이 문자열 테이블에 도달하지 않는다" 쪽이다.
    #[test]
    fn a_blank_value_means_the_same_thing_in_a_pack_and_in_an_override() {
        let dir = temp_dir("blank-symmetry");
        // 같은 본문: ok 는 채우고, cancel·save 는 비운다.
        let body = "[button]\nok = \"OK-EDIT\"\ncancel = \"\"\nsave = \"   \"\n";
        write(
            pack_path(&dir, "bl"),
            &format!("[font]\nbuiltin = true\n\n{body}"),
        );
        write(dir.join("ko.toml"), body);

        let (pack_tr, _) = Translations::load_from("bl", Some(&dir));
        let (ovr_tr, ovr_report) = Translations::load_from("ko", Some(&dir));
        assert_eq!(
            ovr_report.outcome,
            LoadOutcome::BuiltinOverridden {
                path: dir.join("ko.toml"),
            },
            "override 경로를 실제로 지나야 이 테스트가 의미가 있다"
        );

        // 채운 값은 양쪽 다 그대로 반영된다(가드가 멀쩡한 값까지 걷어내지 않는다).
        for (label, tr) in [("pack", &pack_tr), ("override", &ovr_tr)] {
            assert_eq!(tr.get("button.ok"), "OK-EDIT", "{label}");
        }
        // 비운 값은 양쪽 다 화면에 도달하지 않는다 — 이것이 통일된 규칙이다.
        for (label, tr) in [("pack", &pack_tr), ("override", &ovr_tr)] {
            for key in ["button.cancel", "button.save"] {
                assert!(
                    !tr.get(key).trim().is_empty(),
                    "{label} 경로에서 {key} 가 빈 문자열로 나왔다 — 라벨 없는 UI 가 된다"
                );
            }
        }
        // 아래 층이 다르므로 보이는 문자열도 다르다: 팩은 영어, override 는 그 내장 언어.
        assert_eq!(pack_tr.get("button.cancel"), "Cancel");
        assert_eq!(ovr_tr.get("button.cancel"), "취소");
        assert_eq!(ovr_tr.get("button.save"), "저장");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 문서가 안내하는 탈출구가 **실제로 trim 을 통과하는지.**
    ///
    /// `str::trim` 은 유니코드 `White_Space` 를 먹는다. NBSP(U+00A0)와 FIGURE
    /// SPACE(U+2007)는 그 속성을 가지므로 "공백처럼 보이지만 안 먹힌다" 가 아니다 —
    /// 예전 문서가 NBSP 를 예로 들었는데 그대로 따라 하면 값이 그냥 사라진다.
    /// 실제로 남는 것은 `White_Space` 가 아닌 zero-width 계열이다.
    #[test]
    fn the_documented_escape_hatch_survives_the_trim() {
        for eaten in ["", " ", "\t", "\u{00A0}", "\u{2007}", "\u{3000}"] {
            let mut m = HashMap::from([("k".to_string(), eaten.to_string())]);
            assert_eq!(drop_blank_values(&mut m), 1, "{eaten:?} 는 걷힌다");
        }
        for kept in ["\u{200B}", "\u{2060}"] {
            let mut m = HashMap::from([("k".to_string(), kept.to_string())]);
            assert_eq!(drop_blank_values(&mut m), 0, "{kept:?} 는 남아야 한다");
            assert_eq!(m.get("k").map(String::as_str), Some(kept));
        }
    }

    #[test]
    fn blank_pack_values_fall_back_to_english_instead_of_showing_empty() {
        let dir = temp_dir("blank");
        write(
            pack_path(&dir, "bl"),
            "[font]\nbuiltin = true\n\n[button]\nok = \"OK-BL\"\ncancel = \"\"\nsave = \"   \"\n",
        );
        let pack = load_pack(&pack_path(&dir, "bl"), "bl").unwrap();
        // 빈/공백 값은 팩 오버레이에서 빠진다.
        assert_eq!(
            pack.strings.get("button.ok").map(String::as_str),
            Some("OK-BL")
        );
        assert!(!pack.strings.contains_key("button.cancel"));
        assert!(!pack.strings.contains_key("button.save"));

        let (tr, report) = Translations::load_from("bl", Some(&dir));
        assert_eq!(tr.get("button.ok"), "OK-BL");
        // 화면에 빈 라벨이 아니라 영어가 나온다.
        assert_eq!(tr.get("button.cancel"), "Cancel");
        assert_eq!(tr.get("button.save"), "Save");
        assert!(!report.fell_back());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_missing_pack_falls_back_to_english_and_reports() {
        let dir = scenario_dir();
        // zz.toml(단일 파일)만 있고 zz/pack.toml 은 없다 → 새 코드에 단일 파일은 팩이 아니다.
        let (tr, report) = Translations::load_from("zz", Some(&dir));
        assert_eq!(tr.language, "en");
        assert_eq!(tr.get("app.name"), "Tasty");
        assert_eq!(tr.get("button.ok"), "OK");
        assert_eq!(report.requested, "zz");
        assert_eq!(report.effective, "en");
        assert!(report.fell_back());
        assert_eq!(
            report.outcome,
            LoadOutcome::PackMissing {
                expected: pack_path(&dir, "zz"),
            }
        );
        assert!(report.user_warning().is_some());

        // 파일이 아예 없는 코드도 같은 경로.
        let (_, report) = Translations::load_from("nn", Some(&dir));
        assert_eq!(
            report.outcome,
            LoadOutcome::PackMissing {
                expected: pack_path(&dir, "nn"),
            }
        );
        // 사용자 디렉토리를 알 수 없어도 폴백은 성립한다.
        let (tr, report) = Translations::load_from("nn", None);
        assert_eq!(tr.language, "en");
        assert!(matches!(report.outcome, LoadOutcome::PackMissing { .. }));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_invalid_pack_falls_back_to_english_and_reports_reason() {
        let dir = scenario_dir();
        let (tr, report) = Translations::load_from("yy", Some(&dir));
        assert_eq!(tr.language, "en");
        assert_eq!(tr.get("button.ok"), "OK");
        assert_eq!(report.effective, "en");
        assert_eq!(
            report.outcome,
            LoadOutcome::PackInvalid {
                path: pack_path(&dir, "yy"),
                error: PackError::MissingFont,
            }
        );
        assert!(report.user_warning().is_some());

        let (_, report) = Translations::load_from("bad", Some(&dir));
        assert!(matches!(
            report.outcome,
            LoadOutcome::PackInvalid {
                error: PackError::Parse(_),
                ..
            }
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_from_builtin_uses_single_file_override_only() {
        let dir = scenario_dir();
        let (tr, report) = Translations::load_from("ko", Some(&dir));
        assert_eq!(tr.language, "ko");
        assert_eq!(tr.get("button.ok"), "확인!");
        assert_eq!(
            report.outcome,
            LoadOutcome::BuiltinOverridden {
                path: dir.join("ko.toml"),
            }
        );
        assert!(!report.fell_back());
        assert_eq!(report.user_warning(), None);

        // 오버라이드 파일이 깨졌으면 내장 문자열 그대로 + Builtin 판정.
        write(dir.join("ja.toml"), BAD_PACK);
        let (tr, report) = Translations::load_from("ja", Some(&dir));
        assert_eq!(tr.language, "ja");
        assert_eq!(report.outcome, LoadOutcome::Builtin);
        assert_eq!(tr.get("button.ok"), "OK");

        // 내장 코드에는 디렉토리 팩이 적용되지 않는다.
        write(dir.join("en").join("pack.toml"), XX_PACK);
        let (tr, report) = Translations::load_from("en", Some(&dir));
        assert_eq!(tr.get("button.ok"), "OK");
        assert_eq!(report.outcome, LoadOutcome::Builtin);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn namespace_register_lookup() {
        // 직접 Translations 인스턴스로 테스트 (전역 OnceLock와 격리)
        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "en".to_string(),
        };
        // 임시 lang dir 없이 직접 namespace 삽입해 lookup만 확인
        {
            let mut ns = tr.namespaces.write().unwrap();
            let mut m: HashMap<String, &'static str> = HashMap::new();
            m.insert("plugin.x.title".to_string(), "Refresh");
            ns.insert("com.example.x".to_string(), m);
        }
        assert_eq!(tr.get("plugin.x.title"), "Refresh");
        assert_eq!(tr.get("missing.key"), "missing.key");
    }

    #[test]
    fn base_takes_precedence_over_namespace() {
        let mut base: HashMap<String, &'static str> = HashMap::new();
        base.insert("shared.key".to_string(), "FromBase");
        let tr = Translations {
            base,
            namespaces: RwLock::new(HashMap::new()),
            language: "en".to_string(),
        };
        {
            let mut ns = tr.namespaces.write().unwrap();
            let mut m: HashMap<String, &'static str> = HashMap::new();
            m.insert("shared.key".to_string(), "FromPlugin");
            ns.insert("com.example.x".to_string(), m);
        }
        assert_eq!(tr.get("shared.key"), "FromBase");
    }

    #[test]
    fn unregister_namespace_removes_strings() {
        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "en".to_string(),
        };
        {
            let mut ns = tr.namespaces.write().unwrap();
            let mut m: HashMap<String, &'static str> = HashMap::new();
            m.insert("plugin.x.title".to_string(), "Refresh");
            ns.insert("com.example.x".to_string(), m);
        }
        assert_eq!(tr.get("plugin.x.title"), "Refresh");
        tr.unregister_namespace("com.example.x");
        assert_eq!(tr.get("plugin.x.title"), "plugin.x.title");
    }

    #[test]
    fn get_args_replaces_in_order() {
        let mut base: HashMap<String, &'static str> = HashMap::new();
        base.insert("cli.x".to_string(), "alive: {} (port {}, version {})");
        let tr = Translations {
            base,
            namespaces: RwLock::new(HashMap::new()),
            language: "en".to_string(),
        };
        assert_eq!(
            tr.get_args("cli.x", &["host", "1234", "9.9.9"]),
            "alive: host (port 1234, version 9.9.9)"
        );
    }

    #[test]
    fn register_namespace_from_lang_dir() {
        // 임시 디렉토리에 en.toml/ko.toml 만들어서 register_namespace 검증
        let tmp = temp_dir("ns");
        std::fs::write(
            tmp.join("en.toml"),
            "[plugin.x]\ntitle = \"Refresh\"\nbody = \"OnlyEn\"\n",
        )
        .unwrap();
        std::fs::write(tmp.join("ko.toml"), "[plugin.x]\ntitle = \"새로고침\"\n").unwrap();

        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "ko".to_string(),
        };
        tr.register_namespace("com.example.x", &tmp);
        // ko에서 정의된 키
        assert_eq!(tr.get("plugin.x.title"), "새로고침");
        // en에만 있는 키는 fallback으로 노출
        assert_eq!(tr.get("plugin.x.body"), "OnlyEn");

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// plugin 네임스페이스의 빈 값도 **번역 없음**이다 — 아래 층인 그 plugin 의 영어
    /// 문자열이 보인다.
    ///
    /// 팩과 내장 오버라이드는 이미 이 규칙을 따른다(ADR-0124). 네임스페이스만 빠져
    /// 있으면 같은 실수가 plugin 에서만 라벨 없는 버튼이 되고, 규칙을 한 줄로 적을 수
    /// 없어 다음 사람은 먼저 읽은 쪽을 믿는다.
    #[test]
    fn blank_namespace_values_fall_back_to_the_plugin_english_string() {
        let tmp = temp_dir("ns-blank");
        std::fs::write(
            tmp.join("en.toml"),
            "[plugin.x]\ntitle = \"Refresh\"\nbody = \"Reload the view\"\n",
        )
        .unwrap();
        // ko 가 body 를 비워 뒀다 — 번역을 잊은 흔한 형태.
        std::fs::write(
            tmp.join("ko.toml"),
            "[plugin.x]\ntitle = \"새로고침\"\nbody = \"\"\n",
        )
        .unwrap();

        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "ko".to_string(),
        };
        tr.register_namespace("com.example.x", &tmp);

        assert_eq!(tr.get("plugin.x.title"), "새로고침");
        assert_eq!(
            tr.get("plugin.x.body"),
            "Reload the view",
            "빈 값이 영어를 덮으면 화면에 라벨 없는 자리가 남는다"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// 폭 0 문자는 빈 값이 아니다 — 일부러 비운 텍스트의 탈출구(ADR-0124)가
    /// 네임스페이스에서도 같아야 한다.
    #[test]
    fn zero_width_namespace_values_survive_the_blank_rule() {
        let tmp = temp_dir("ns-zw");
        std::fs::write(tmp.join("en.toml"), "[plugin.x]\nsuffix = \"EN\"\n").unwrap();
        std::fs::write(tmp.join("ko.toml"), "[plugin.x]\nsuffix = \"\u{2060}\"\n").unwrap();

        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "ko".to_string(),
        };
        tr.register_namespace("com.example.x", &tmp);

        assert_eq!(tr.get("plugin.x.suffix"), "\u{2060}");

        std::fs::remove_dir_all(&tmp).ok();
    }
}

#[cfg(test)]
mod pack_size_and_discovery_tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tasty-i18n-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: PathBuf, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    const HEADER: &str = "[meta]\nname = \"Big\"\n\n[font]\nbuiltin = true\n\n[filler]\n";

    /// `[font]`/`[meta]` 를 갖춘 유효한 팩을 만들되 본문을 `bytes` 근처까지 채운다.
    fn pack_of_size(bytes: usize) -> String {
        let mut s = String::from(HEADER);
        let mut i = 0usize;
        while s.len() < bytes {
            s.push_str(&format!("k{i} = \"{}\"\n", "x".repeat(64)));
            i += 1;
        }
        s
    }

    #[test]
    fn a_pack_over_the_limit_is_rejected_before_it_is_parsed() {
        let dir = temp_dir("toobig");
        let path = dir.join("big").join("pack.toml");
        write(path.clone(), &pack_of_size(MAX_PACK_BYTES as usize + 4096));

        match load_pack(&path, "big") {
            Err(PackError::TooLarge { bytes, max }) => {
                assert_eq!(max, MAX_PACK_BYTES);
                assert!(bytes > MAX_PACK_BYTES, "실제 크기를 보고해야 한다: {bytes}");
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
        assert!(matches!(
            load_pack_head(&path),
            Err(PackError::TooLarge { .. })
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_oversized_pack_does_not_appear_in_the_language_list() {
        let dir = temp_dir("toobig-scan");
        write(
            dir.join("big").join("pack.toml"),
            &pack_of_size(MAX_PACK_BYTES as usize + 4096),
        );
        write(
            dir.join("ok").join("pack.toml"),
            "[meta]\nname = \"Ok\"\n\n[font]\nbuiltin = true\n",
        );
        let codes: Vec<String> = scan_languages(Some(&dir))
            .into_iter()
            .map(|e| e.code)
            .collect();
        assert!(codes.contains(&"ok".to_string()));
        assert!(
            !codes.contains(&"big".to_string()),
            "상한 초과 팩이 목록에 올랐다: {codes:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn selecting_an_oversized_pack_falls_back_to_english_without_touching_the_setting() {
        let dir = temp_dir("toobig-load");
        write(
            dir.join("big").join("pack.toml"),
            &pack_of_size(MAX_PACK_BYTES as usize + 4096),
        );
        let (tr, report) = Translations::load_from("big", Some(&dir));
        assert_eq!(tr.language, "en");
        assert_eq!(tr.get("button.ok"), "OK");
        // 요청 코드는 그대로 보고된다 — 설정값을 덮어쓰지 않는다는 계약의 관측점.
        assert_eq!(report.requested, "big");
        assert_eq!(report.effective, "en");
        assert!(report.fell_back());
        assert!(matches!(
            report.outcome,
            LoadOutcome::PackInvalid {
                error: PackError::TooLarge { .. },
                ..
            }
        ));
        assert!(report.user_warning().is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_pack_just_under_the_limit_still_loads() {
        // 상한이 정상 팩을 자르지 않는다는 반대편 — 가장 큰 내장 파일의 23 배까지 통과.
        let dir = temp_dir("justunder");
        let path = dir.join("ok").join("pack.toml");
        write(
            path.clone(),
            &pack_of_size(MAX_PACK_BYTES as usize - 65_536),
        );
        let pack = load_pack(&path, "ok").expect("상한 아래 팩은 통과해야 한다");
        assert_eq!(pack.font, FontDecl::Builtin);
        assert_eq!(pack.display_name.as_deref(), Some("Big"));
        assert!(!pack.strings.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_builtin_override_file_is_capped_too() {
        let dir = temp_dir("override-big");
        // `<builtin>.toml` 도 사용자가 놓는 파일이다 — 여기만 열어두면 상한이 무의미하다.
        write(
            dir.join("ko.toml"),
            &pack_of_size(MAX_PACK_BYTES as usize + 4096),
        );
        let ko = scan_languages(Some(&dir))
            .into_iter()
            .find(|e| e.code == "ko")
            .expect("내장 ko 는 항상 목록에 있다");
        assert_eq!(
            ko.source,
            LanguageSource::Builtin,
            "상한 초과 오버라이드는 적용된 것으로 표시되면 안 된다"
        );
        let (tr, _) = Translations::load_from("ko", Some(&dir));
        assert_eq!(tr.language, "ko");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// "목록에 오르면 로드된다" — 발견(head)과 로드(full)의 수락/거절 판정이 같아야 한다.
    /// 경량 경로를 따로 두면서 이 등가성이 깨지는 것이 가장 실질적인 회귀다.
    #[test]
    fn the_discovery_path_accepts_and_rejects_exactly_what_the_load_path_does() {
        let dir = temp_dir("parity");
        let cases: &[(&str, &str)] = &[
            (
                "good",
                "[meta]\nname = \"G\"\n\n[font]\nbuiltin = true\n\n[a]\nb = \"c\"\n",
            ),
            ("nofont", "[meta]\nname = \"N\"\n\n[a]\nb = \"c\"\n"),
            ("badfont", "[font]\nfile = \"\"\n"),
            ("badtoml", "[font\nbuiltin = true\n"),
            ("notatable", "42\n"),
        ];
        for (code, body) in cases {
            let path = dir.join(code).join("pack.toml");
            write(path.clone(), body);
            let full = load_pack(&path, code);
            let head = load_pack_head(&path);
            assert_eq!(
                full.is_ok(),
                head.is_ok(),
                "'{code}' 에서 발견과 로드의 판정이 갈렸다: full={full:?} head={head:?}"
            );
            if let (Ok(full), Ok(head)) = (full, head) {
                assert_eq!(full.font, head.font);
                assert_eq!(full.display_name, head.display_name);
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 발견이 문자열 키를 **하나도** flatten 하지 않는다 — 이 티켓의 비용 축 자체.
    /// 시간이 아니라 호출 사실로 본다(시간 비교는 부하에 흔들려 회귀 신호가 못 된다).
    #[test]
    fn discovery_never_flattens_the_string_table() {
        let dir = temp_dir("cost");
        // 팩 3 개 — 스캔이 전부를 훑는데도 flatten 이 0 이어야 한다.
        for code in ["aa", "bb", "cc"] {
            write(dir.join(code).join("pack.toml"), &pack_of_size(256 * 1024));
        }

        flatten_probe::reset();
        let listed = scan_languages(Some(&dir));
        let scan_flattens = flatten_probe::count();
        assert!(
            listed.iter().any(|e| e.code == "aa"),
            "팩이 목록에 올라야 이 측정이 의미가 있다"
        );
        assert_eq!(
            scan_flattens, 0,
            "발견이 문자열을 flatten 했다 — `pack_entry` 가 `load_pack` 으로 되돌아갔다"
        );

        // 반대편: 실제 로드는 flatten 한다(계수기가 살아 있다는 것도 함께 확인).
        flatten_probe::reset();
        load_pack(&dir.join("aa").join("pack.toml"), "aa").unwrap();
        assert!(flatten_probe::count() > 0);

        flatten_probe::reset();
        load_pack_head(&dir.join("aa").join("pack.toml")).unwrap();
        assert_eq!(flatten_probe::count(), 0);

        std::fs::remove_dir_all(&dir).ok();
    }
}
