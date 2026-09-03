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
//! `Box::leak`ed to satisfy `&'static str` lookup contract — the per-plugin
//! string set is small and bounded so the leak is acceptable.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
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
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "cannot read: {e}"),
            Self::Parse(e) => write!(f, "invalid TOML: {e}"),
            Self::MissingFont => write!(f, "missing [font] section"),
            Self::InvalidFont(e) => write!(f, "invalid [font]: {e}"),
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
const TOAST_MAX_CHARS: usize = 200;

/// Shortest path fragment worth showing. Below this the path says nothing, so the
/// message keeps the ellipsis and the log carries the real path.
const MIN_PATH_CHARS: usize = 12;

/// Render a warning that carries a path, shrinking **the path only** so the whole
/// message fits [`TOAST_MAX_CHARS`].
///
/// `render` must interpolate its argument exactly once, which lets the budget be
/// exact: everything except the path is rendered first (with an empty path) and
/// whatever is left of the cap goes to the path.
fn fit_path(path: &Path, render: impl Fn(&str) -> String) -> String {
    let full = path.display().to_string();
    let skeleton = render("").chars().count();
    let budget = TOAST_MAX_CHARS.saturating_sub(skeleton).max(MIN_PATH_CHARS);
    render(&elide_middle(&full, budget))
}

/// Drop the middle of `s` so it fits `max` chars, keeping both ends. The tail is
/// the half that matters here — it carries `<code>/pack.toml`, i.e. *which* pack —
/// so it keeps roughly two thirds of the budget.
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

/// Read and validate `path` as the pack manifest of `code`. The `[font]` section
/// is mandatory; `[meta] name` is optional. Every other string leaf becomes a
/// translation key, overlaid on the English base at load time.
///
/// A **blank value counts as "not translated"** and is dropped, so the English
/// base shows through instead of a label-less button. A pack author leaving a key
/// empty is the common case and a blank string on screen is hard to trace back to
/// a key; deliberately blank text is rare and can still be written as whitespace
/// that the trim does not eat (e.g. a non-breaking space). Same rule as
/// [`meta_name`] and [`non_empty_str`], which already reject blanks.
pub fn load_pack(path: &Path, code: &str) -> Result<LanguagePack, PackError> {
    let text = std::fs::read_to_string(path).map_err(|e| PackError::Read(e.to_string()))?;
    let value: toml::Value = text
        .parse()
        .map_err(|e: toml::de::Error| PackError::Parse(e.to_string()))?;
    let Some(table) = value.as_table() else {
        return Err(PackError::Parse("root is not a table".to_string()));
    };
    let font = match table.get("font") {
        None => return Err(PackError::MissingFont),
        Some(font) => parse_font_decl(font)?,
    };
    let display_name = meta_name(table);
    let mut body = table.clone();
    body.remove("font");
    let mut strings = HashMap::new();
    Translations::flatten_toml("", &toml::Value::Table(body), &mut strings);
    let blank = drop_blank_values(&mut strings);
    if blank > 0 {
        tracing::warn!(
            "i18n: language pack '{code}' at {} has {blank} blank value(s) — those keys fall back to English",
            path.display()
        );
    }
    Ok(LanguagePack {
        code: code.to_string(),
        path: path.to_path_buf(),
        display_name,
        font,
        strings,
    })
}

/// Remove keys whose value is blank (empty or whitespace only) and return how many
/// were dropped. Those keys then miss the pack overlay and resolve on the English
/// base — see [`load_pack`] for why blank means "not translated".
fn drop_blank_values(strings: &mut HashMap<String, String>) -> usize {
    let before = strings.len();
    strings.retain(|_, v| !v.trim().is_empty());
    before - strings.len()
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
        let mut outcome = LoadOutcome::Builtin;
        if let Some(dir) = lang_dir {
            let path = dir.join(format!("{code}.toml"));
            match read_user_toml(&path) {
                Ok(Some(value)) => {
                    Self::flatten_toml("", &value, &mut strings);
                    tracing::info!("loaded user translations from {}", path.display());
                    outcome = LoadOutcome::BuiltinOverridden { path };
                }
                Ok(None) => {}
                Err(e) => tracing::warn!(
                    "i18n: user override {} ignored — {e} (built-in '{code}' strings stay active)",
                    path.display()
                ),
            }
        }
        (strings, outcome)
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
            Self::flatten_toml("", &value, map);
        }
    }

    fn flatten_toml(prefix: &str, value: &toml::Value, map: &mut HashMap<String, String>) {
        match value {
            toml::Value::Table(table) => {
                for (key, val) in table {
                    let full_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };
                    Self::flatten_toml(&full_key, val, map);
                }
            }
            toml::Value::String(s) => {
                map.insert(prefix.to_string(), s.clone());
            }
            // Ignore non-string leaf values
            _ => {}
        }
    }

    /// Get a translated string by key. Falls back to the key itself if not found.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(s) = self.base.get(key) {
            return s;
        }
        if let Ok(ns) = self.namespaces.read() {
            for map in ns.values() {
                if let Some(s) = map.get(key) {
                    return s;
                }
            }
        }
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
                Self::parse_toml_into(&mut strings, &s);
            }
        }

        let leaked: HashMap<String, &'static str> =
            strings.into_iter().map(|(k, v)| (k, leak_str(v))).collect();

        let count = leaked.len();
        if let Ok(mut ns) = self.namespaces.write() {
            ns.insert(namespace.to_string(), leaked);
        }
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
        if let Ok(mut ns) = self.namespaces.write() {
            ns.remove(namespace);
        }
        tracing::info!("i18n: unregistered namespace '{}'", namespace);
    }
}

/// Read a user TOML file. `Ok(None)` when it does not exist; `Err` for any
/// other I/O failure or a parse error (the caller decides how loud to be).
fn read_user_toml(path: &Path) -> Result<Option<toml::Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .parse::<toml::Value>()
            .map(Some)
            .map_err(|e| format!("invalid TOML: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read: {e}")),
    }
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
    match load_pack(&path, code) {
        Ok(pack) => Some(LanguageEntry {
            code: code.to_string(),
            display_name: pack.display_name,
            source: LanguageSource::Pack,
            font: Some(pack.font),
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
}
