//! 번역 카탈로그 정합 가드 — `lang/{en,ko,ja}.toml`(루트) 과 각 번들 plugin 의
//! `lang/{en,ko,ja}.toml` 이 서로 어긋나면 fail 한다.
//!
//! 배경: `CLAUDE.md` "국제화" 는 새 문자열마다 세 파일에 같은 키를 넣으라고 하지만,
//! 지금까지 그 정합은 손으로 지켜 왔다. 키를 못 찾으면 `t()` 가 키 문자열을 그대로
//! 돌려주므로(`docs/dev-guide/i18n.md`) 누락은 화면에 `settings.foo.bar` 같은 글자로
//! 드러나는데, 리뷰가 놓치면 그대로 배포된다. 이 테스트가 `cargo test --workspace`
//! (`.github/workflows/test.yml`) 에서 그 정합을 집행한다.
//!
//! 검사 4 종:
//! - **키 집합** — ko/ja 가 en 과 정확히 같은 키 집합을 가진다(누락·잉여 0).
//! - **placeholder** — 키마다 `{}` 개수와 `{name}` 이름 집합이 en 과 같다. `t_fmt` 계열은
//!   `{}` 를 순서대로 치환하므로 개수가 다르면 인자가 새거나 남고, 이름 있는 placeholder
//!   (`{current}`/`{secs}` 등)는 호출자가 `.replace("{name}", ..)` 로 채우므로 이름이 다르면
//!   그대로 화면에 남는다.
//! - **en 과 같은 값** — ko/ja 값이 en 과 글자까지 같으면 미번역으로 본다. 고유명사·
//!   약어·기호·경로처럼 같아야 정상인 값은 형태로 자동 예외 처리하고, 판단이 필요한 것은
//!   [`SAME_AS_ENGLISH_ALLOWLIST`] 에 이유와 함께 등록한다.
//! - **소스의 리터럴 키** — `t("…")` / `t_fmt("…")` / `t_fmt2("…")` / `t_args("…")` 의
//!   리터럴 첫 인자가 카탈로그(루트 + plugin en)에 존재한다. `format!` 으로 만드는 동적
//!   키는 대상이 아니다.
//!
//! 등록 방법과 예외의 근거는 `docs/dev-guide/i18n.md` "강제 테스트" 절.
//! 선례: `tests/native_surface_labels_i18n.rs`(키 존재) ·
//! `tests/plugin_manifest_version_parity.rs`(디렉토리 순회 parity) ·
//! `tests/no_todo_file_citation.rs`(소스 스캔 + allowlist).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const LANGS: &[&str] = &["en", "ko", "ja"];

/// ko/ja 값이 en 과 같아도 되는 키 — (키, 적용 언어, 이유). 이유가 없는 항목은 넣지
/// 않는다. 형태로 자동 예외되는 값(기호만·약어·경로·명령·수식키 이름)은 등록이 필요 없다.
const SAME_AS_ENGLISH_ALLOWLIST: &[(&str, &[&str], &str)] = &[
    // 고유명사 · 제품명 — 번역하면 다른 것을 가리킨다(i18n.md "하드코딩 허용 예외" 와 같은 근거).
    ("app.name", &["ko", "ja"], "제품명"),
    ("settings.appearance.subtab.tasty", &["ko", "ja"], "제품명"),
    (
        "settings.terminal.shell_mode_tasty",
        &["ko", "ja"],
        "제품명 + rc 파일명",
    ),
    ("settings.misc.subtab.tastyrc", &["ko", "ja"], "rc 파일명"),
    (
        "settings.subtab.claude",
        &["ko", "ja"],
        "제품명 (Claude Code)",
    ),
    ("settings.subtab.codex", &["ko", "ja"], "제품명 (Codex)"),
    ("settings.subtab.markdown", &["ko", "ja"], "포맷명"),
    ("git_viewer.tools_menu_item", &["ko", "ja"], "도구명 (Git)"),
    ("git_viewer.heading", &["ko", "ja"], "도구명 (Git)"),
    (
        "git_viewer.diff_heading",
        &["ko", "ja"],
        "개발 도구 관용 표기 (Diff)",
    ),
    (
        "git_viewer.detached",
        &["ko", "ja"],
        "git HEAD 상태 토큰 — 명령 출력과 같은 말이어야 한다",
    ),
    // 설정 어휘를 그대로 보여 주는 태그 — 설정 파일의 키·값과 같은 말이어야 사용자가 대응시킬 수 있다.
    (
        "remote_tool.attach_tag_inline",
        &["ko", "ja"],
        "attach 프로필 연결 방식 식별자(인라인 host/user)",
    ),
    (
        "remote_tool.attach_tag_profile",
        &["ko", "ja"],
        "attach 프로필 연결 방식 식별자(ssh_ref 참조)",
    ),
    (
        "remote_tool.field_passkey",
        &["ko", "ja"],
        "WebAuthn 용어 — 같은 팝업의 탭명(remote_tool.tab_passkeys)도 Passkey 로 통일",
    ),
    (
        "remote_tool.tab_attach",
        &["ko"],
        "ko 는 attach 를 제품 용어로 유지(remote_attach.* ko 가 'Attach 프로필' 로 부른다)",
    ),
    // file handler 탭 — 같은 탭의 오류·도움말 문구(err_*/bullet_*/field_priority)가 ko/ja 에서도
    // Priority / Action / IPC method / Surface kind 를 영어 용어로 쓴다. 헤더만 번역하면 탭 안에서
    // 용어가 갈라지므로, 탭 전체 용어 정비 없이는 유지한다.
    (
        "settings.file_handler.handlers.col_priority",
        &["ko", "ja"],
        "file handler 탭 용어 통일",
    ),
    (
        "settings.file_handler.handlers.col_action",
        &["ko", "ja"],
        "file handler 탭 용어 통일",
    ),
    (
        "settings.file_handler.handlers.field_action_kind",
        &["ko", "ja"],
        "file handler 탭 용어 통일",
    ),
    (
        "settings.file_handler.handlers.field_ipc_method",
        &["ko", "ja"],
        "file handler 탭 용어 통일",
    ),
    (
        "settings.file_handler.handlers.field_surface_kind",
        &["ko", "ja"],
        "file handler 탭 용어 통일",
    ),
    (
        "settings.file_handler.hook_handlers.prio",
        &["ko", "ja"],
        "file handler 탭 용어 통일 (Priority 약칭)",
    ),
    // 도움말 문구가 태그명을 그대로 인용한다(settings.keybindings.select_preset_label 의 "Active 표시").
    (
        "settings.keybindings.preset_active_tag",
        &["ko", "ja"],
        "도움말이 태그명 'Active' 를 그대로 인용",
    ),
    // CLI 구조 출력 — 스크립트가 파싱하는 결과 행(i18n.md "구조 출력이 쓰는 고정 토큰").
    (
        "cli.remote_check.alive_basic",
        &["ko", "ja"],
        "remote check 결과 행 — 기계 파싱 대상",
    ),
    (
        "cli.remote_check.alive_version",
        &["ko", "ja"],
        "remote check 결과 행 — 기계 파싱 대상",
    ),
    (
        "cli.remote_check.alive_full",
        &["ko", "ja"],
        "remote check 결과 행 — 기계 파싱 대상",
    ),
    // 에러 접두 식별자 — 뒤에 오는 원문 에러와 함께 로그성으로 읽힌다.
    (
        "claude.profile.error_prefix",
        &["ko", "ja"],
        "에러 접두 식별자 (profile:)",
    ),
    (
        "claude.gate.error_prefix",
        &["ko", "ja"],
        "에러 접두 식별자 (gate:)",
    ),
];

/// 수식키·키 이름 — 번역하지 않는다(i18n.md "하드코딩 허용 예외").
const MODIFIER_TOKENS: &[&str] = &[
    "Ctrl", "Alt", "Shift", "Cmd", "Command", "Option", "Super", "Meta", "Win", "Fn",
];

/// 소스가 참조하지만 카탈로그에 없는 키 중 **수정 대기** 로 알려진 것 — (키, 메모).
/// 여기 있는 동안은 실패로 치지 않되, 키가 카탈로그에 생기면(=고쳐지면) 이 항목을
/// 지우라고 fail 한다. 새 위반을 여기에 넣어 덮지 않는다 — 고치는 것이 기본이다.
const PENDING_FIX_MISSING_KEYS: &[(&str, &str)] = &[
    // src/gfx/gpu/shell_setup.rs 가 settings.general.* 로 조회하지만 카탈로그는 이 다섯을
    // [settings.terminal] 아래에 둔다 — 첫 실행 셸 설정 화면에 키 문자열이 그대로 노출된다.
    (
        "settings.general.setup_subtitle",
        "shell_setup.rs: 카탈로그는 settings.terminal.setup_subtitle",
    ),
    (
        "settings.general.shell_not_found",
        "shell_setup.rs: 카탈로그는 settings.terminal.shell_not_found",
    ),
    (
        "settings.general.shell_label",
        "shell_setup.rs: 카탈로그는 settings.terminal.shell_label",
    ),
    (
        "settings.general.shell_invalid_path",
        "shell_setup.rs: 카탈로그는 settings.terminal.shell_invalid_path",
    ),
    (
        "settings.general.shell_valid",
        "shell_setup.rs: 카탈로그는 settings.terminal.shell_valid",
    ),
];

/// 소스 순회에서 통째로 가지치기할 디렉토리명(`tests/no_todo_file_citation.rs` 와 동일).
const PRUNE_DIRS: &[&str] = &[
    "target",
    "dist",
    ".worktree",
    ".git",
    "node_modules",
    ".claude",
    ".claude-workspace",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn rel_of(file: &Path) -> String {
    file.strip_prefix(root())
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 카탈로그 디렉토리 — 루트 `lang/` + `crates/tasty-plugin-*/lang/`(존재하는 것만).
fn lang_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![root().join("lang")];
    let crates = root().join("crates");
    let entries =
        std::fs::read_dir(&crates).unwrap_or_else(|e| panic!("read_dir {}: {e}", crates.display()));
    let mut plugin_dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|dir| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("tasty-plugin-"))
        })
        .map(|dir| dir.join("lang"))
        .filter(|lang| lang.join("en.toml").is_file())
        .collect();
    plugin_dirs.sort();
    dirs.extend(plugin_dirs);
    dirs
}

/// `[a.b] c = "v"` 를 `a.b.c` 점 키로 평탄화한다(`tasty-i18n` 로더와 같은 규칙 —
/// 문자열 leaf 만 카탈로그 항목이다).
fn flatten(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let full = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&full, v, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

fn load(dir: &Path, lang: &str) -> BTreeMap<String, String> {
    let path = dir.join(format!("{lang}.toml"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let value: toml::Value = text
        .parse()
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut out = BTreeMap::new();
    flatten("", &value, &mut out);
    out
}

/// 카탈로그 한 세트 — 디렉토리 + 언어별 평탄 맵.
struct Catalog {
    rel: String,
    by_lang: BTreeMap<&'static str, BTreeMap<String, String>>,
}

fn catalogs() -> Vec<Catalog> {
    lang_dirs()
        .into_iter()
        .map(|dir| Catalog {
            rel: rel_of(&dir),
            by_lang: LANGS.iter().map(|lang| (*lang, load(&dir, lang))).collect(),
        })
        .collect()
}

/// `{}` 개수와 `{name}` 이름 집합. 이름은 `[A-Za-z_][A-Za-z0-9_]*` 만 인정한다 —
/// 그 외(`{0}` 같은 것)는 이 프로젝트의 치환 규약에 없으므로 이름으로 세지 않고
/// 본문 글자로 취급한다.
fn placeholders(value: &str) -> (usize, BTreeSet<String>) {
    let mut positional = 0;
    let mut named = BTreeSet::new();
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let inner = &after[..close];
        if inner.is_empty() {
            positional += 1;
        } else if is_identifier(inner) {
            named.insert(inner.to_string());
        }
        rest = &after[close + 1..];
    }
    (positional, named)
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// placeholder 를 뺀 본문.
fn without_placeholders(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..open]);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// 언어와 무관하게 같아야 정상인 값 — 기호·숫자만 / 약어 / 경로·URL·명령 / 수식키 이름.
fn is_language_neutral(value: &str) -> bool {
    let body = without_placeholders(value);
    let letters: Vec<char> = body.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return true; // `—` / `99+` / `{} · {}` / `%`
    }
    let trimmed = body.trim();
    if trimmed.contains("://")
        || trimmed.starts_with("~/")
        || trimmed.starts_with('/')
        || trimmed.starts_with("tasty ")
        || trimmed.contains('$')
    {
        return true; // 경로 · URL · 셸 명령
    }
    // 약어 — 글자가 전부 대문자이고 다섯 자 이하 (ID / URL / TUI / DAG ID / HTML... / NONE / OK).
    if letters.len() <= 5 && letters.iter().all(|c| c.is_ascii_uppercase()) {
        return true;
    }
    MODIFIER_TOKENS.contains(&trimmed)
}

fn allowlisted_same(key: &str, lang: &str) -> bool {
    SAME_AS_ENGLISH_ALLOWLIST
        .iter()
        .any(|(k, langs, _)| *k == key && langs.contains(&lang))
}

#[test]
fn key_sets_match_english() {
    let mut problems = Vec::new();
    for cat in catalogs() {
        let en: BTreeSet<&String> = cat.by_lang["en"].keys().collect();
        for lang in &LANGS[1..] {
            let other: BTreeSet<&String> = cat.by_lang[lang].keys().collect();
            for key in en.difference(&other) {
                problems.push(format!("  {}/{lang}.toml: missing `{key}`", cat.rel));
            }
            for key in other.difference(&en) {
                problems.push(format!(
                    "  {}/{lang}.toml: extra `{key}` (not in en)",
                    cat.rel
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "translation catalogs diverge from en (CLAUDE.md 국제화: add every key to all three files):\n{}",
        problems.join("\n")
    );
}

#[test]
fn placeholders_match_english() {
    let mut problems = Vec::new();
    for cat in catalogs() {
        let en = &cat.by_lang["en"];
        for lang in &LANGS[1..] {
            for (key, value) in &cat.by_lang[lang] {
                let Some(en_value) = en.get(key) else {
                    continue; // key_sets_match_english 가 보고한다
                };
                let expected = placeholders(en_value);
                let actual = placeholders(value);
                if expected != actual {
                    problems.push(format!(
                        "  {}/{lang}.toml `{key}`: en has {} `{{}}` + {:?}, {lang} has {} `{{}}` + {:?}\n      en: {en_value:?}\n      {lang}: {value:?}",
                        cat.rel, expected.0, expected.1, actual.0, actual.1
                    ));
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "placeholder mismatch — `{{}}` count and `{{name}}` set must equal en for every key:\n{}",
        problems.join("\n")
    );
}

#[test]
fn same_as_english_values_are_allowlisted() {
    let mut problems = Vec::new();
    for cat in catalogs() {
        let en = &cat.by_lang["en"];
        for lang in &LANGS[1..] {
            for (key, value) in &cat.by_lang[lang] {
                let Some(en_value) = en.get(key) else {
                    continue;
                };
                if value != en_value || is_language_neutral(value) || allowlisted_same(key, lang) {
                    continue;
                }
                problems.push(format!("  {}/{lang}.toml `{key}` = {value:?}", cat.rel));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "values identical to en look untranslated — translate them, or if the value must stay \
         (proper noun, fixed identifier, machine-parsed output) add the key to \
         SAME_AS_ENGLISH_ALLOWLIST with a reason (docs/dev-guide/i18n.md 강제 테스트):\n{}",
        problems.join("\n")
    );
}

// ── 소스의 리터럴 키 ───────────────────────────────────────────────────

fn is_source_target(rel: &str) -> bool {
    if !rel.ends_with(".rs") {
        return false;
    }
    let in_crate_src = rel
        .strip_prefix("crates/")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(_, after)| after.starts_with("src/"));
    if !(rel.starts_with("src/") || in_crate_src) {
        return false;
    }
    // 테스트 코드는 대상이 아니다 — 픽스처 키(`ns.hello` 등)를 쓴다.
    !(rel.contains("/tests/") || rel.ends_with("/tests.rs") || rel.ends_with("_tests.rs"))
}

fn gather(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if is_source_target(&rel_of(path)) {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if PRUNE_DIRS.contains(&name) {
                continue;
            }
        }
        gather(&p, out);
    }
}

/// `#[cfg(test)]` / `#[test]` 가 붙은 아이템(모듈·함수·impl)의 본문을 건너뛴다. 파일 끝까지
/// 끊지 않으므로 테스트 모듈 **뒤에** 오는 아이템도 스캔한다 — clippy `items_after_test_module`
/// 이 경고만 하는 배치가 실제로 있어, 첫 `#[cfg(test)]` 에서 멈추면 그 뒤가 사각지대가 된다.
#[derive(Default)]
struct TestRegion {
    skipping: bool,
    depth: i32,
    opened: bool,
}

impl TestRegion {
    /// 이 줄이 테스트 영역이면 true.
    fn skip(&mut self, line: &str) -> bool {
        let trimmed = line.trim_start();
        if !self.skipping {
            if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[test]") {
                self.skipping = true;
                self.depth = 0;
                self.opened = false;
                return true;
            }
            return false;
        }
        let (opens, closes) = brace_counts(line);
        if opens > 0 {
            self.opened = true;
        }
        self.depth += opens - closes;
        if self.opened {
            if self.depth <= 0 {
                self.skipping = false; // 본문 끝 — 이 줄까지 테스트 영역
            }
        } else if trimmed.ends_with(';') {
            self.skipping = false; // `#[cfg(test)] mod tests;` / `use …;` 한 줄 아이템
        }
        true
    }
}

/// 문자열·문자 리터럴·`//` 주석 밖의 `{` / `}` 개수. raw string(`r#"…"#`) 안의 중괄호는
/// 구분하지 않는다 — 테스트 픽스처에 드물고, 어긋나면 위반이 드러나는 쪽(오탐)으로 기운다.
fn brace_counts(line: &str) -> (i32, i32) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_str = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            match c {
                '\\' => {
                    chars.next();
                }
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '\'' => {
                // 문자 리터럴 `'{'` / `'}'` — 라이프타임 `'a` 는 뒤에 중괄호가 오지 않아 무해.
                if let Some(&next) = chars.peek()
                    && (next == '{' || next == '}')
                {
                    chars.next();
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'/') => break,
            '{' => opens += 1,
            '}' => closes += 1,
            _ => {}
        }
    }
    (opens, closes)
}

/// 한 줄에서 `t("k")` / `t_fmt("k", ..)` / `t_fmt2("k", ..)` / `t_args("k", ..)` 의
/// 리터럴 키를 전부 뽑는다. 함수명 앞은 식별자 문자가 아니어야 한다(`fmt(` / `not(` 배제).
fn literal_keys(line: &str) -> Vec<String> {
    const CALLS: &[&str] = &["t_fmt2(", "t_fmt(", "t_args(", "t("];
    let mut keys = Vec::new();
    for call in CALLS {
        let mut from = 0;
        while let Some(pos) = line[from..].find(call) {
            let start = from + pos;
            from = start + call.len();
            let preceded_by_ident = line[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if preceded_by_ident {
                continue;
            }
            let rest = line[from..].trim_start();
            let Some(after_quote) = rest.strip_prefix('"') else {
                continue; // 변수·format! — 동적 키
            };
            let Some(end) = after_quote.find('"') else {
                continue;
            };
            keys.push(after_quote[..end].to_string());
        }
    }
    keys
}

#[test]
fn literal_translation_keys_exist_in_catalog() {
    let mut known: BTreeSet<String> = BTreeSet::new();
    for dir in lang_dirs() {
        known.extend(load(&dir, "en").into_keys());
    }
    let mut files = Vec::new();
    gather(root(), &mut files);
    files.sort();

    let mut problems = Vec::new();
    let mut pending_seen: BTreeSet<&str> = BTreeSet::new();
    for file in &files {
        let Ok(contents) = std::fs::read_to_string(file) else {
            continue;
        };
        let rel = rel_of(file);
        let mut tests = TestRegion::default();
        for (idx, line) in contents.lines().enumerate() {
            if tests.skip(line) {
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            for key in literal_keys(line) {
                if known.contains(&key) {
                    continue;
                }
                if let Some((pending, _)) = PENDING_FIX_MISSING_KEYS.iter().find(|(k, _)| *k == key)
                {
                    pending_seen.insert(pending);
                    continue;
                }
                problems.push(format!("  {rel}:{}: `{key}`", idx + 1));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "t() keys not found in any lang/en.toml (typo, or the key was never added — t() would \
         render the key itself):\n{}",
        problems.join("\n")
    );
    let stale: Vec<String> = PENDING_FIX_MISSING_KEYS
        .iter()
        .filter(|(k, _)| !pending_seen.contains(k))
        .map(|(k, note)| format!("  `{k}` ({note})"))
        .collect();
    assert!(
        stale.is_empty(),
        "PENDING_FIX_MISSING_KEYS entries are no longer missing — remove them:\n{}",
        stale.join("\n")
    );
}
