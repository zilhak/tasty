//! 번역 카탈로그 정합 가드 — `lang/{en,ko,ja}.toml`(루트) 과 각 번들 plugin 의
//! `lang/{en,ko,ja}.toml` 이 서로 어긋나면 fail 한다.
//!
//! 배경: `CLAUDE.md` "국제화" 는 새 문자열마다 세 파일에 같은 키를 넣으라고 하지만,
//! 지금까지 그 정합은 손으로 지켜 왔다. 키를 못 찾으면 `t()` 가 키 문자열을 그대로
//! 돌려주므로(`docs/dev-guide/i18n.md`) 누락은 화면에 `settings.foo.bar` 같은 글자로
//! 드러나는데, 리뷰가 놓치면 그대로 배포된다. 이 테스트가 그 정합을 집행한다 — 단
//! 자동 실행은 **헤드리스 조합**(`check-headless` 의 전체 스위트)에서만 일어난다
//! (기본 조합 잡은 `--lib --bins` 라 통합 타깃을 못 본다 — `docs/dev-guide/ci-gates.md`).
//! 자동 잡은 push 된 커밋만 보므로 번역 문자열을 건드렸으면 커밋 전에 직접 돌려라.
//!
//! 검사 5 종:
//! - **키 집합** — ko/ja 가 en 과 정확히 같은 키 집합을 가진다(누락·잉여 0).
//! - **placeholder** — 키마다 `{}` 개수와 `{name}` 이름 집합이 en 과 같다. `t_fmt` 계열은
//!   `{}` 를 순서대로 치환하므로 개수가 다르면 인자가 새거나 남고, 이름 있는 placeholder
//!   (`{current}`/`{secs}` 등)는 호출자가 `.replace("{name}", ..)` 로 채우므로 이름이 다르면
//!   그대로 화면에 남는다.
//! - **en 과 같은 값** — ko/ja 값이 en 과 글자까지 같으면 미번역으로 본다. 고유명사·
//!   약어·기호·경로처럼 같아야 정상인 값은 형태로 자동 예외 처리하고, 판단이 필요한 것은
//!   [`SAME_AS_ENGLISH_ALLOWLIST`] 에 이유와 함께 등록한다.
//! - **소스의 리터럴 키**(순방향, 소스 → 카탈로그) — `t("…")` / `t_fmt("…")` /
//!   `t_fmt2("…")` / `t_args("…")` 의 리터럴 첫 인자가 카탈로그(루트 + plugin en)에
//!   존재한다. `format!` 으로 만드는 동적 키는 대상이 아니다.
//! - **카탈로그 키의 소비자**(역방향, 카탈로그 → 소스) — 카탈로그의 키를 드는 자리가
//!   추적 파일 어딘가에 있다. 없으면 [`ORPHAN_KEYS`] 명부와 정확히 대조한다.
//!
//! **왜 두 방향을 다 보는가.** 한 방향짜리 집합 차분은 **이동과 손실을 못 가른다.**
//! 유일한 호출부가 지워져 키가 고아가 되는 형태에서 순방향은 초록이다 — 소스에 그 키가
//! 더는 없으니 "소스가 드는 키가 카탈로그에 있다" 는 여전히 참이다. 반대로 호출부가
//! 네임스페이스를 잘못 부르면 순방향은 "카탈로그에 없는 키" 로, 역방향은 "아무도 안 쓰는
//! 키" 로 **같은 사건을 양쪽에서** 신고한다. 실제로 그런 것이 있었다 —
//! `src/gfx/gpu/shell_setup.rs` 가 `settings.general.*` 로 부르는데 카탈로그는 그 다섯을
//! `[settings.terminal]` 아래 두고 있었고, 순방향은 그것을 [`PENDING_FIX_MISSING_KEYS`]
//! 로 유예해 두고 있었다. 역방향을 켜자 같은 사건의 반대쪽 절반(고아가 된 카탈로그 키)이
//! 나왔고, 그래서 호출부를 고쳐 양쪽이 함께 비었다.
//!
//! 등록 방법과 예외의 근거는 `docs/dev-guide/i18n.md` "강제 테스트" 절.
//! 선례: `tests/native_surface_labels_i18n.rs`(키 존재) ·
//! `tests/plugin_manifest_version_parity.rs`(디렉토리 순회 parity) ·
//! `crates/tasty-doc-guards/tests/no_todo_file_citation.rs`(소스 스캔 + allowlist).

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
    // 비어 있는 것이 정상이다 — 유예는 예외이고 고치는 것이 기본이다.
];

/// 소스 순회에서 통째로 가지치기할 디렉토리명(`crates/tasty-doc-guards/tests/no_todo_file_citation.rs` 와 동일).
const PRUNE_DIRS: &[&str] = &["target", "dist", ".worktree", ".git", "node_modules"];

/// gitignored 로컬 폴더 이름의 조각. 리터럴로 두면 이 파일이 비-git 경로 참조 금지
/// (`docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md`) 를 어긴다 — 인용이
/// 아니라 순회 입력이지만, 조각으로 조립하면 예외 등록 없이 규칙을 지킬 수 있다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

/// 가지치기 대상 디렉토리인지 — 빌드 산출물 + gitignored 로컬 폴더(선행 `.`).
fn is_pruned(name: &str) -> bool {
    PRUNE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == format!("{LOCAL_HEAD}{LOCAL_TAIL}"))
}

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
            if is_pruned(name) {
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

/// 번역 진입점 이름을 **소스에서 도출한다** — host 자유 함수(`tasty-i18n`)와 plugin 쪽
/// `Translator` 메서드(`tasty-plugin-sdk`) 양쪽.
///
/// 손으로 적은 목록으로 두면 **진입점을 하나 더 만드는 커밋이 이 스캔을 조용히 눈멀게
/// 한다.** 실제로 그랬다: 목록이 `t` / `t_fmt` / `t_fmt2` / `t_args` 넷이라 SDK 의
/// `t_replace` 와 host 의 `t_fmt_fit` 로 적힌 키는 이 검사를 통과했고, 그 자리에
/// 오타를 넣어도 초록이었다(변이로 확인). 오타난 키는 `Translator` 가 **키 문자열을
/// 그대로 돌려주므로** 사용자 화면에 `codex.reboot.screen_unreadableX` 가 나간다.
fn translation_entry_points() -> BTreeSet<String> {
    const SOURCES: &[&str] = &[
        "crates/tasty-i18n/src/lib.rs",
        "crates/tasty-plugin-sdk/src/i18n.rs",
    ];
    let mut out = BTreeSet::new();
    for rel in SOURCES {
        let path = root().join(rel);
        let text =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} 을 못 읽었다: {e}"));
        for line in text.lines() {
            let Some(after) = line.trim_start().strip_prefix("pub fn ") else {
                continue;
            };
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            // 번역 진입점은 전부 `t` 로 시작하고 첫 인자가 key 다.
            if name == "t" || name.starts_with("t_") {
                out.insert(name);
            }
        }
    }
    out
}

/// 진입점 수의 하한 — 도출이 0 건이면 아래 스캔이 아무 키도 못 찾고 조용히 통과한다.
/// 값의 근거: 2026-09-05 실측 6(`t` · `t_args` · `t_fmt` · `t_fmt2` · `t_fmt_fit` ·
/// `t_replace`).
const MIN_ENTRY_POINTS: usize = 5;

/// 도출이 살아 있다 — 진입점을 하나도 못 찾으면 이 파일의 소스 스캔 전체가 무의미하다.
#[test]
fn the_entry_point_derivation_is_alive() {
    let found = translation_entry_points();
    assert!(
        found.len() >= MIN_ENTRY_POINTS,
        "번역 진입점을 {} 개밖에 못 찾았다(하한 {MIN_ENTRY_POINTS}, 2026-09-05 실측 6): \
         {found:?} — `pub fn t…` 의 형태가 바뀌었으면 도출기를 고쳐라",
        found.len()
    );
    for expected in ["t", "t_fmt", "t_args", "t_replace"] {
        assert!(
            found.contains(expected),
            "`{expected}` 가 도출에서 빠졌다: {found:?}"
        );
    }
}

/// 한 줄에서 번역 진입점의 **리터럴 키**를 전부 뽑는다. 함수명 앞은 식별자 문자가
/// 아니어야 한다(`fmt(` / `not(` 배제).
///
/// `next` 는 **다음 줄**이다 — 인자가 여럿인 호출은 rustfmt 가 여는 괄호에서 줄을
/// 끊어 키를 다음 줄로 밀어낸다(`tr.t_replace(` 뒤 개행). 한 줄만 보던 판정은 그
/// 형태를 전부 "동적 키" 로 흘려보냈다. 실측(2026-09-05): 키가 같은 줄에 있는 자리
/// 1344 · 다음 줄로 밀린 자리 102 — 전체의 7 % 가 안 보이던 셈이다.
fn literal_keys(line: &str, next: Option<&str>) -> Vec<String> {
    // 긴 이름부터 본다 — `t(` 를 먼저 찾으면 `t_fmt(` 안의 `t` 를 집는 일은 없지만,
    // 순서를 고정해 두는 편이 나중에 형태가 늘어도 안전하다.
    let mut names: Vec<String> = translation_entry_points().into_iter().collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let calls: Vec<String> = names.iter().map(|n| format!("{n}(")).collect();
    let mut keys = Vec::new();
    for call in &calls {
        let mut from = 0;
        while let Some(pos) = line[from..].find(call.as_str()) {
            let start = from + pos;
            from = start + call.len();
            let preceded_by_ident = line[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
            if preceded_by_ident {
                continue;
            }
            let same_line = line[from..].trim_start();
            // 여는 괄호에서 줄이 끊겼으면 키는 다음 줄에 있다.
            let rest = if same_line.is_empty() {
                next.map(str::trim_start).unwrap_or("")
            } else {
                same_line
            };
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

/// 줄바꿈 뒤로 밀린 키를 본다 — 그리고 **동적 키를 리터럴로 오인하지 않는다.**
///
/// 뒤엣것이 이 판정의 위험한 쪽이다: 다음 줄을 무조건 읽으면 `t(key_var)` 같은 자리에서
/// 엉뚱한 줄의 문자열을 키로 집어 존재하지 않는 키를 신고한다(거짓 양성). 그래서
/// 앞 줄의 인자 자리가 **비었을 때만** 다음 줄을 본다.
#[test]
fn a_key_wrapped_to_the_next_line_is_still_seen() {
    let found = literal_keys(
        "        return Err(IpcMethodError::new(tr.t_replace(",
        Some("            \"codex.reboot.screen_unreadable\","),
    );
    assert_eq!(
        found,
        vec!["codex.reboot.screen_unreadable".to_string()],
        "여는 괄호에서 줄이 끊긴 호출의 키를 못 봤다"
    );

    // 같은 줄에 인자가 이미 있으면 다음 줄은 보지 않는다.
    let dynamic = literal_keys("    tr.t(key_var);", Some("    \"not.a.key\","));
    assert!(
        dynamic.is_empty(),
        "동적 키인데 다음 줄의 문자열을 키로 집었다: {dynamic:?}"
    );

    // 인자 자리가 비었는데 다음 줄도 리터럴이 아니면 여전히 동적 키다.
    let still_dynamic = literal_keys("    tr.t(", Some("        key_var,"));
    assert!(
        still_dynamic.is_empty(),
        "다음 줄이 변수인데 키를 만들어 냈다: {still_dynamic:?}"
    );
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
        let lines: Vec<&str> = contents.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            if tests.skip(line) {
                continue;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            for key in literal_keys(line, lines.get(idx + 1).copied()) {
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

/// 면제가 가리키는 **번역 키가 실재하는가** — 참조 무결성.
///
/// 가리키는 것의 갈래가 경로 겹과 달라서 판정도 다르다. 경로 면제는 파일시스템을 묻지만
/// 이 겹은 카탈로그를 묻는다 — `tasty_doc_guards::missing_referents` 로 덮을 수 없는
/// 자리다. 한 검사로 뭉개면 어느 쪽도 제대로 안 본다.
///
/// **초록은 "이 면제가 아직 필요하다" 가 아니다**(ADR-0150). 키가 실재해도 그 번역이
/// 더 이상 영어와 같지 않아 면제가 놀고 있을 수 있고, 그것은 결함이 아니다.
///
/// 키가 썩으면(오탈자·키 개명) 면제가 조용히 아무것도 안 가리키게 되고, 정작 그 키의
/// 번역이 영어와 같아지면 사유가 등록돼 있는데도 빨개진다.
#[test]
fn same_as_english_allowlist_points_at_keys_that_exist() {
    let cats = catalogs();
    let mut known: BTreeSet<&String> = BTreeSet::new();
    for cat in &cats {
        known.extend(cat.by_lang["en"].keys());
    }
    assert!(
        !known.is_empty(),
        "카탈로그가 비었다 — 모수가 0 이면 아래 단정이 언제나 통과한다"
    );

    let mut problems = Vec::new();
    for (key, langs, _) in SAME_AS_ENGLISH_ALLOWLIST {
        if !known.iter().any(|k| k.as_str() == *key) {
            problems.push(format!("  없는 키: `{key}`"));
        }
        for lang in *langs {
            if !LANGS.contains(lang) {
                problems.push(format!("  없는 언어: `{key}` → `{lang}`"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "면제가 실재하지 않는 것을 가리킨다 — 키가 개명됐으면 항목도 고치고, 사라졌으면 \
         지워라:\n{}",
        problems.join("\n")
    );
}

// ── 역방향: 카탈로그의 키를 쓰는 자리가 있는가 ─────────────────────────

/// 위 순방향 검사([`literal_translation_keys_exist_in_catalog`])는 **소스 → 카탈로그**
/// 한 방향만 본다. 이 절이 반대 방향을 본다 — **카탈로그에 있는데 아무도 안 쓰는 키.**
///
/// 왜 두 방향이 다 필요한가: 한 방향짜리 집합 차분은 **이동과 손실을 못 가른다.** 유일한
/// 호출부가 지워져 키가 고아가 되는 형태에서 순방향은 초록이다(소스에 그 키가 더는
/// 없으니까). 실제로 그 형태가 있었다 — `src/view/settings/ui/tabs/accessibility.rs` 의
/// 주석이 탭 제목을 L2 사이드바로 옮긴 결정을 적고 있고, 그 탭들의 `…heading` 키 일곱은
/// 카탈로그에만 남았다.
///
/// **소비자가 `.rs` 만이 아니다.** plugin 매니페스트의 `description_i18n_key` 가 키를
/// 문자열로 든다. 그래서 이 검사는 순방향처럼 소스 리터럴을 파싱하지 않고, **추적 파일
/// 전량에서 키 문자열이 언급되는지**를 본다 — 판정을 느슨한 쪽으로 몰아, 고아라고
/// 말하는 것에 대해서만 확신을 갖는다.
///
/// **동적 조립은 소스에서 도출한다.** `format!("convert_popup.{kind}")` 처럼 조립되는
/// 키는 완성형이 소스에 없다. 허용목록으로 적지 않고 [`dynamic_key_templates`] 가
/// 소스에서 그 형태를 찾아낸다 — 새 동적 키 갈래가 생겨도 이 검사가 거짓 양성을 내지
/// 않는다.

/// 카탈로그에 있는데 **아무 추적 파일도 문자열로 안 드는** 키. 뒷값은 지금까지 짚은
/// 사유다. 이 명부는 **정확히 같아야** 한다 — 새 고아가 생기면 늘어서 실패하고, 고아가
/// 아니게 되면 남아서 실패한다(줄어드는 쪽도 실패시키는 이유: 남는 여유가 곧 안 보는
/// 구간이다).
///
/// 지우지 않고 명부에 둔 이유: 키를 지우는 것은 되돌리기 쉽지만, **정말 그 UI 가
/// 없어졌는지**는 이 정적 스캔이 답하지 못한다. 항목마다 화면에서 확인한 뒤 지운다.
const ORPHAN_KEYS: &[(&str, &str)] = &[
    (
        "attach.held_body",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "attach.held_title",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "attach.held_workspace_body",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "attach.held_workspace_title",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    ("badge.overflow", "호출부 0 — 사라진 시점은 아직 안 짚었다"),
    ("button.open", "호출부 0 — 사라진 시점은 아직 안 짚었다"),
    (
        "cli.terminal.desc",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "clipboard_viewer.popup.title",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "dialog.error.file_not_found",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "dialog.error.invalid_format",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "dialog.html.title",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "dialog.html.url_label",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "dialog.recent_files",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "explorer.hide_preview",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "explorer.popup.add_favorite.path",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "explorer.popup.rename.path",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "explorer.popup.rename.rename",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "explorer.select_file",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "explorer.show_preview",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "explorer.unsupported_format",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "git_viewer.error",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "html.settings.note",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "markdown.addr.no_recent",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "pane_context_menu.new_explorer",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "port_scanner.hint",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "port_scanner.no_ports",
        "호출부 0 — 형제 `…no_ports_*` 넷만 쓰인다",
    ),
    (
        "quit_modal.cancel_button",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "remote_tool.hide",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "remote_tool.refresh",
        "호출부 0 — `remote_tool.refresh_tooltip` 만 쓰인다",
    ),
    (
        "remote_tool.reveal",
        "호출부 0 — `remote_tool.reveal_tooltip` 만 쓰인다",
    ),
    (
        "settings.accessibility.heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.appearance.colors.bg_label",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.appearance.colors.fg_label",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.appearance.colors.focused_heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.appearance.colors.unfocused_heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.appearance.heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.appearance.sidebar_width_label",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.appearance.tab_font_size_label",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.appearance.tab_width_label",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.appearance.theme.dark",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.appearance.theme.light",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.file_handler.coming_soon",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.file_handler.common.disabled",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.file_handler.common.enabled",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "settings.general.heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.keybindings.heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.notifications.heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.performance.heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "settings.terminal.heading",
        "탭 제목을 L2 사이드바가 그리게 되면서 호출부가 사라졌다",
    ),
    (
        "git_viewer.diff_heading",
        "호출부 0 — 이 파일의 SAME_AS_ENGLISH_ALLOWLIST 가 유일한 언급이다",
    ),
    (
        "toast.copied_files",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    ("toast.cut_files", "호출부 0 — 사라진 시점은 아직 안 짚었다"),
    (
        "toast.vi_copy_entered",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "workspace_category.collapse_all",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
    (
        "workspace_category.expand_all",
        "호출부 0 — 사라진 시점은 아직 안 짚었다",
    ),
];

/// 소스에서 도출한 동적 키 템플릿 — `("정적 앞부분.", "정적 뒷부분")`. 리터럴 안에서
/// `"a.b.{"` 로 시작하는 것만 센다(placeholder 가 **그 문자열 안에** 있어야 한다) —
/// `json!({})` 처럼 리터럴 **밖의** 중괄호를 세면 살아 있는 키까지 전부 동적으로
/// 오분류한다(실측으로 걸렸다: 그 느슨한 판정은 51 건을 17 건으로 줄여 보였다).
fn dynamic_key_templates() -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let mut files = Vec::new();
    gather(root(), &mut files);
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for lit in string_literals(&text) {
            let Some(open) = lit.find('{') else { continue };
            let Some(close) = lit[open..].find('}') else {
                continue;
            };
            let (head, tail) = (&lit[..open], &lit[open + close + 1..]);
            let inner = &lit[open + 1..open + close];
            // placeholder 안은 비었거나 식별자여야 한다(`{}` · `{kind}`).
            if !(inner.is_empty() || is_identifier(inner)) {
                continue;
            }
            // 정적 앞부분이 점으로 끝나는 키 모양이어야 한다.
            if !head.ends_with('.') || head.len() < 2 {
                continue;
            }
            if !head
                .trim_end_matches('.')
                .split('.')
                .all(|seg| !seg.is_empty() && is_identifier(seg))
            {
                continue;
            }
            if !tail
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            {
                continue;
            }
            out.insert((head.to_string(), tail.to_string()));
        }
    }
    out
}

/// `text` 안의 큰따옴표 문자열 리터럴들. 이스케이프(`\"`)를 건너뛴다.
fn string_literals(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j] != b'"' {
            if bytes[j] == b'\\' {
                j += 1;
            }
            j += 1;
        }
        if j >= bytes.len() {
            break;
        }
        if let Ok(s) = std::str::from_utf8(&bytes[start..j]) {
            out.push(s.to_string());
        }
        i = j + 1;
    }
    out
}

/// 키가 이 템플릿들 중 하나로 조립될 수 있는가. 채워지는 자리에 점이 없어야 한다 —
/// 점을 허용하면 `"a."` 하나가 `a` 아래 전부를 설명해 버려 판정이 공허해진다.
fn is_dynamically_assembled(key: &str, templates: &BTreeSet<(String, String)>) -> bool {
    templates.iter().any(|(head, tail)| {
        key.len() > head.len() + tail.len()
            && key.starts_with(head.as_str())
            && key.ends_with(tail.as_str())
            && !key[head.len()..key.len() - tail.len()].contains('.')
    })
}

/// 카탈로그 키를 문자열로 들 수 있는 추적 파일 전량 — `lang/{en,ko,ja}.toml` 자신만 뺀다.
/// 확장자로 바이너리를 거르고, 빌드 산출물·git 내부·gitignore 된 로컬 폴더는 건너뛴다.
fn consumer_files() -> Vec<PathBuf> {
    const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules"];
    const SKIP_EXT: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "webp", "ico", "ttf", "otf", "woff", "woff2", "wasm", "pdf",
        "bin", "zip", "gz",
    ];
    let mut out = Vec::new();
    let mut stack = vec![root().to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                // 점으로 시작하는 디렉토리는 추적 대상이 아니다(로컬 작업 폴더 포함).
                if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let rel = rel_of(&path);
            if LANGS
                .iter()
                .any(|l| rel.ends_with(&format!("lang/{l}.toml")))
            {
                continue;
            }
            // **이 파일 자신은 소비자가 아니다.** [`ORPHAN_KEYS`] 가 고아 키를 이름으로
            // 들기 때문에, 자기를 말뭉치에 넣으면 명부에 적는 순간 그 키가 "쓰이는 키"
            // 가 되어 판정이 자기를 만족시킨다.
            if rel == "tests/i18n_key_parity.rs" {
                continue;
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if SKIP_EXT.contains(&ext.as_str()) {
                continue;
            }
            out.push(path);
        }
    }
    out
}

/// en 카탈로그 전체의 키(루트 + plugin).
fn all_english_keys() -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for dir in lang_dirs() {
        keys.extend(load(&dir, "en").into_keys());
    }
    keys
}

#[test]
fn every_catalog_key_has_a_consumer() {
    let keys = all_english_keys();
    // 말뭉치를 **키 모양 토큰**으로 미리 쪼갠다 — 키마다 파일 전량을 훑으면
    // (키 수 × 말뭉치 바이트)라 분 단위가 된다. 토큰은 `[A-Za-z0-9_.]` 의 최대 연속이라
    // 따옴표·괄호 안의 키는 그대로 토큰 하나가 된다.
    let mut files = 0usize;
    let mut tokens: BTreeSet<String> = BTreeSet::new();
    for path in consumer_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        files += 1;
        // 하이픈도 토큰 글자다 — `file_handler.host.markdown-viewer` 처럼 plugin id 를
        // 담는 키가 있어서, 빼면 그 키가 쪼개져 거짓 고아가 된다(실측으로 걸렸다).
        for token in
            text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'))
        {
            if token.contains('.') && token.len() > 2 {
                tokens.insert(token.to_string());
            }
        }
    }

    // 자기-공허 검사 — 순회나 쪼개기가 죽으면 이 검사는 "전부 고아" 를 외치게 되므로
    // 조용히 초록이 되지는 않는다. 그래도 말뭉치가 살아 있는지는 먼저 못박는다.
    assert!(
        files > 100,
        "소비자 파일이 {files} 개뿐이다 — 순회가 깨졌다"
    );
    assert!(
        tokens.len() > 10_000,
        "키 모양 토큰이 {} 개뿐이다 — 쪼개기가 깨졌다",
        tokens.len()
    );
    assert!(keys.len() > 1000, "카탈로그 키가 {} 개뿐이다", keys.len());

    let templates = dynamic_key_templates();
    assert!(
        !templates.is_empty(),
        "동적 키 템플릿을 하나도 못 찾았다 — 리터럴 스캔이 죽었다"
    );

    let mut orphans = BTreeSet::new();
    for key in &keys {
        // **토큰과 정확히 같아야** 쓰인 것으로 본다. 더 긴 토큰 안에 들어 있는 것까지
        // 쳐 주면(`a.b` 를 `a.b_suffix` 가 덮는다) 형제 키가 형을 가린다 — 실측으로
        // 셋이 그렇게 숨어 있었다(`port_scanner.no_ports` 를 `…no_ports_tasty_empty` 가,
        // `remote_tool.refresh`·`…reveal` 을 각자의 `…_tooltip` 이 가렸다). 느슨한 쪽은
        // 거짓 고발을 안 하는 대신 **진짜 고아를 못 본다** — 이 검사가 있는 이유가
        // 그 방향이라, 정확 일치를 쓴다.
        if tokens.contains(key.as_str()) {
            continue;
        }
        if is_dynamically_assembled(key, &templates) {
            continue;
        }
        orphans.insert(key.clone());
    }

    let listed: BTreeSet<String> = ORPHAN_KEYS.iter().map(|(k, _)| (*k).to_string()).collect();
    assert_eq!(
        ORPHAN_KEYS.len(),
        listed.len(),
        "ORPHAN_KEYS 에 같은 키가 두 번 있다"
    );

    let new: Vec<&String> = orphans.difference(&listed).collect();
    let gone: Vec<&String> = listed.difference(&orphans).collect();
    assert!(
        new.is_empty() && gone.is_empty(),
        "카탈로그 키의 소비자 판정이 명부와 다르다.\n         새로 고아가 된 키({}) — 호출부를 지웠으면 키도 지워라:\n{}\n         이제 고아가 아닌 키({}) — ORPHAN_KEYS 에서 빼라:\n{}",
        new.len(),
        new.iter()
            .map(|k| format!("  {k}"))
            .collect::<Vec<_>>()
            .join("\n"),
        gone.len(),
        gone.iter()
            .map(|k| format!("  {k}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

/// 명부의 항목이 **실재하는 카탈로그 키**여야 한다. 키를 지우면서 명부를 안 지우면
/// 위 검사는 그 항목을 `gone` 으로 잡지만, 이 검사는 그 사실을 따로 이름 붙여 낸다.
#[test]
fn every_orphan_entry_names_a_catalog_key() {
    let keys = all_english_keys();
    let missing: Vec<&str> = ORPHAN_KEYS
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !keys.contains(*k))
        .collect();
    assert!(
        missing.is_empty(),
        "ORPHAN_KEYS 가 없는 키를 든다: {missing:?}"
    );
    let no_reason: Vec<&str> = ORPHAN_KEYS
        .iter()
        .filter(|(_, why)| why.trim().is_empty())
        .map(|(k, _)| *k)
        .collect();
    assert!(no_reason.is_empty(), "사유 없는 항목: {no_reason:?}");
}

#[test]
fn a_dynamically_assembled_key_is_not_called_an_orphan() {
    let mut templates = BTreeSet::new();
    templates.insert(("convert_popup.".to_string(), String::new()));
    assert!(is_dynamically_assembled("convert_popup.html", &templates));
    // 채워지는 자리에 점이 있으면 그 템플릿의 산물이 아니다 — 아니면 접두사 하나가
    // 그 아래 전부를 설명해 판정이 공허해진다.
    assert!(!is_dynamically_assembled("convert_popup.a.b", &templates));
    // 접두사만으로 끝나는 키는 채울 자리가 없다.
    assert!(!is_dynamically_assembled("convert_popup.", &templates));
}

#[test]
fn a_brace_outside_the_literal_is_not_a_template() {
    // `probe("attach.list", json!({}))` — 중괄호가 리터럴 **밖에** 있다. 이것을 템플릿으로
    // 세면 `attach.` 아래 살아 있는 키까지 전부 동적으로 오분류된다.
    let lits = string_literals(r#"probe(port, "attach.list", json!({}))"#);
    assert!(lits.contains(&"attach.list".to_string()), "got {lits:?}");
    assert!(
        lits.iter().all(|l| !l.contains('{')),
        "리터럴 밖 중괄호를 안으로 끌어들였다: {lits:?}"
    );
}
