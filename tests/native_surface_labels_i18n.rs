//! OS 네이티브 표면(macOS NSMenu / 시스템 트레이 / Windows Jump List)이 조회하는
//! 번역 키가 `lang/{en,ko,ja}.toml` 세 파일 모두에 존재하는지 검증한다.
//!
//! 배경: 이 표면들은 플랫폼 전용 파일(`#[cfg]`)에서 `t()` 를 호출하므로 Linux CI 는
//! macOS/Windows 경로를 컴파일하지 않고, 런타임 키 누락은 화면에 키 문자열이 그대로
//! 노출되는 형태로만 드러난다(`docs/dev-guide/i18n.md` — 키 미존재 시 키 반환).
//! lang 파일은 플랫폼 무관이므로 여기서 키 존재를 세 언어에 대해 한 번에 고정한다.
//! 앱 이름을 결합하는 macOS 항목은 `t_fmt` 로 `{}` 하나를 채우므로 placeholder 개수도
//! 함께 검증한다.

use std::collections::BTreeMap;

/// 플랫폼 소스가 조회하는 키 전체. 소스의 `t("…")` / `t_fmt("…", …)` 리터럴과 동기.
const REQUIRED_KEYS: &[&str] = &[
    // src/platform/macos_delegate.rs
    "menu.macos.about",
    "menu.macos.hide",
    "menu.macos.hide_others",
    "menu.macos.show_all",
    "menu.macos.quit",
    "menu.macos.file",
    "menu.macos.new_window",
    "menu.macos.window",
    "menu.macos.minimize",
    "menu.macos.zoom",
    "menu.macos.close_window",
    // src/platform/system_tray.rs
    "tray.show_window",
    "tray.new_window",
    "tray.quit",
    "tray.tooltip",
    // src/platform/jump_list.rs
    "jump_list.new_window",
    "jump_list.new_window_desc",
];

/// `t_fmt(key, &process_name)` 로 앱 이름을 끼워 넣는 키 — `{}` 가 정확히 1 개여야 한다.
const APP_NAME_FMT_KEYS: &[&str] = &["menu.macos.about", "menu.macos.hide", "menu.macos.quit"];

const LANG_FILES: &[(&str, &str)] = &[
    ("en", include_str!("../lang/en.toml")),
    ("ko", include_str!("../lang/ko.toml")),
    ("ja", include_str!("../lang/ja.toml")),
];

/// `[a.b] c = "v"` 를 `a.b.c` 점 키로 평탄화한다(`tasty-i18n` 의 로더와 같은 규칙).
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

fn load(lang: &str, toml_str: &str) -> BTreeMap<String, String> {
    let value: toml::Value = toml_str
        .parse()
        .unwrap_or_else(|e| panic!("lang/{lang}.toml 파싱 실패: {e}"));
    let mut out = BTreeMap::new();
    flatten("", &value, &mut out);
    out
}

#[test]
fn native_surface_keys_exist_in_all_languages() {
    for (lang, toml_str) in LANG_FILES {
        let table = load(lang, toml_str);
        let missing: Vec<&str> = REQUIRED_KEYS
            .iter()
            .copied()
            .filter(|k| table.get(*k).is_none_or(|v| v.trim().is_empty()))
            .collect();
        assert!(
            missing.is_empty(),
            "lang/{lang}.toml 에 네이티브 표면 키가 없거나 비어 있음: {missing:?}"
        );
    }
}

#[test]
fn app_name_keys_have_exactly_one_placeholder() {
    for (lang, toml_str) in LANG_FILES {
        let table = load(lang, toml_str);
        for key in APP_NAME_FMT_KEYS {
            let value = &table[*key];
            assert_eq!(
                value.matches("{}").count(),
                1,
                "lang/{lang}.toml `{key}` = {value:?} — 앱 이름 placeholder `{{}}` 는 정확히 1 개"
            );
        }
    }
}

#[test]
fn non_fmt_keys_have_no_placeholder() {
    for (lang, toml_str) in LANG_FILES {
        let table = load(lang, toml_str);
        for key in REQUIRED_KEYS
            .iter()
            .filter(|k| !APP_NAME_FMT_KEYS.contains(k))
        {
            let value = &table[*key];
            assert!(
                !value.contains("{}"),
                "lang/{lang}.toml `{key}` = {value:?} — `t()` 로 조회하는 키에 placeholder 가 있음"
            );
        }
    }
}

// ========== 목록이 소스와 어긋나지 않는가 ==========
//
// 위 검사들의 모수는 `REQUIRED_KEYS` 다 — **소스에 박힌 상수**라, 걷기가 깨져서 비는
// 경로가 없는 대신 **아무도 소스와 대조하지 않는다.** 목록 머리말은 "소스의 `t("…")` /
// `t_fmt("…", …)` 리터럴과 동기" 라고 선언하는데 그 동기를 검사하는 것이 없었고,
// 그래서 목록을 통째로 비우면 위 셋이 전부 통과했다(검사할 것이 없으므로).
//
// 여기서부터가 그 대조다. 플랫폼 소스에서 실제로 호출되는 키를 뽑아 목록과 **양방향**
// 으로 맞춘다 — 한 방향만 보면 stale 항목이 영원히 남는다. 모수 고정의 등급은 하한이
// 아니라 집합 동등이다(`docs/adr/0133-guard-scan-population-is-pinned-not-enumerated.md`
// 의 ③).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 이 가드가 읽는 소스 트리. 개별 파일이 아니라 디렉터리다 — 나중에 생기는 파일이
/// 기본 제외가 되지 않게(ADR-0133 ①).
const PLATFORM_DIR: &str = "src/platform";

/// 스캔이 걷어 온 `.rs` 파일 수의 하한. **연기 검사 용도**다(ADR-0133 ③) — 경로가 틀리면
/// 예외가 아니라 조용한 0 이 되고, 0 인 모수는 언제나 초록이기 때문이다.
///
/// 아래 집합 동등이 이미 대부분을 덮는다(걷기가 비면 추출 집합이 비어 목록과 안 맞는다).
/// 이 하한이 따로 필요한 경우는 하나뿐이다 — **목록과 걷기가 동시에 비는 것.** 그때는 양쪽
/// 다 빈 집합이라 동등이 성립해 버린다.
///
/// 값의 근거: 2026-09-05 실측 `src/platform/**/*.rs` **16 개**. 성장 추적이 아니라 걷기가
/// 깨진 것을 잡는 값이라 여유를 크게 둔다.
const MIN_PLATFORM_FILES: usize = 10;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn gather_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            gather_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 한 호출의 종류. `t_fmt` 는 인자를 끼워 넣으므로 placeholder 규칙이 다르다.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Call {
    Plain,
    Fmt,
}

/// 소스 한 편에서 `t("키")` / `t_fmt("키", …)` 의 키를 뽑는다.
///
/// 바늘이 좁아야 하는 이유가 실측으로 있다. `t(` 를 그냥 찾으면 `insert("` · `expect("` ·
/// `from_str(` 처럼 **t 로 끝나는 다른 이름**이 전부 걸린다(`src/platform/debug_info.rs`
/// 한 파일에만 18 줄). 그래서 앞 글자가 식별자 문자가 아닐 것을 요구한다.
///
/// 줄 주석은 건너뛴다. 이 세 파일의 머리말이 `t("menu.macos.*")` 같은 **산문**을 담고
/// 있어, 안 걸러내면 `menu.macos.*` 가 키로 잡힌다.
fn calls_in(src: &str) -> Vec<(String, Call)> {
    let mut out = Vec::new();
    for line in src.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let b = line.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if b[i] == b't' && !i.checked_sub(1).is_some_and(|p| is_ident_byte(b[p])) {
                if let Some((key, call, next)) = call_at(b, i) {
                    out.push((key, call));
                    i = next;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `b[at..]` 가 `t("…"` 또는 `t_fmt("…"` 이면 키와 종류, 그리고 닫는 따옴표 다음 위치.
fn call_at(b: &[u8], at: usize) -> Option<(String, Call, usize)> {
    let (call, open) = if b[at..].starts_with(br#"t(""#) {
        (Call::Plain, at + 3)
    } else if b[at..].starts_with(br#"t_fmt(""#) {
        (Call::Fmt, at + 7)
    } else {
        return None;
    };
    let close = open + b[open..].iter().position(|c| *c == b'"')?;
    let key = std::str::from_utf8(&b[open..close]).ok()?;
    Some((key.to_string(), call, close + 1))
}

fn scan_platform_calls() -> (usize, Vec<(String, Call)>) {
    let root = repo_root().join(PLATFORM_DIR);
    let mut files = Vec::new();
    gather_rs(&root, &mut files);
    files.sort();
    let mut calls = Vec::new();
    for file in &files {
        let src = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("{} 읽기 실패: {e}", file.display()));
        calls.extend(calls_in(&src));
    }
    (files.len(), calls)
}

/// `REQUIRED_KEYS` 가 플랫폼 소스가 실제로 부르는 키 집합과 **정확히** 같다.
///
/// 양방향이다. "소스에 있는데 목록에 없다" 는 번역이 없는 키가 화면에 키 문자열로
/// 노출되는 경로이고(이 파일 머리말의 원래 동기), "목록에 있는데 소스에 없다" 는
/// 지워진 표면의 항목이 목록에 남아 영원히 검토받는 형태다. 한쪽만 보면 뒤가 남는다.
#[test]
fn the_required_key_list_matches_what_the_platform_sources_ask_for() {
    let (file_count, calls) = scan_platform_calls();
    assert!(
        file_count >= MIN_PLATFORM_FILES,
        "{PLATFORM_DIR} 에서 .rs 를 {file_count} 개만 걷었다 — 2026-09-05 실측 16 개. \
         걷기가 깨졌다면 아래 대조는 양쪽이 비어 성립해 버린다\n\
           (2026-09-05 실측 16 · 2026-09-06 실측 17).\n\
           ★ 판별 — 이 모수는 한 디렉토리라 밖에서 세는 값이 정확히 같아야 한다:\n\
               git ls-files 'src/platform/*.rs' | wc -l\n\
           **여기서는 두 수가 같지 않으면 그 자체가 답이다** — 가지치기도 확장자 분기도 없어서 차이가 \
           날 이유가 없다. 두 수가 같은데 둘 다 하한 아래면 플랫폼 코드가 정말 줄어든 것이고, git 쪽만 \
           크면 걷기가 도중에 멈춘 것이다.\n\
           ★ 이 하한을 내려서 통과시키지 마라 — 이 값이 막는 사고는 단 하나, **목록과 걷기가 동시에 \
           비어 아래 집합 동등이 공짜로 성립하는 것**이다. 내리면 그 하나가 사라진다.\n\
           플랫폼 파일이 정말 줄었으면 위 명령으로 다시 세고 근거 날짜를 함께 갱신하라."
    );

    let found: BTreeSet<&str> = calls.iter().map(|(k, _)| k.as_str()).collect();
    let listed: BTreeSet<&str> = REQUIRED_KEYS.iter().copied().collect();

    let unlisted: Vec<&&str> = found.difference(&listed).collect();
    let stale: Vec<&&str> = listed.difference(&found).collect();

    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "REQUIRED_KEYS 가 {PLATFORM_DIR} 의 실제 호출과 어긋난다.\n\
         \x20 소스가 부르는데 목록에 없음: {unlisted:?}\n\
         \x20 목록에 있는데 소스가 안 부름: {stale:?}\n\
         전자는 세 언어에 번역이 없어도 아무도 모르는 키다. 후자는 사라진 표면의 \
         잔재이므로 목록에서 지운다."
    );
}

/// `t_fmt` 로 부르는 키의 집합도 소스에서 온다 — `APP_NAME_FMT_KEYS` 를 손으로 맞추지 않는다.
///
/// placeholder 규칙(`t` 는 `{}` 없음 / `t_fmt` 는 하나)이 이 구분에 걸려 있으므로,
/// 구분이 소스와 어긋나면 placeholder 검사가 엉뚱한 키를 본다.
#[test]
fn the_fmt_key_list_matches_which_call_the_source_uses() {
    let (_, calls) = scan_platform_calls();
    let found_fmt: BTreeSet<&str> = calls
        .iter()
        .filter(|(_, c)| *c == Call::Fmt)
        .map(|(k, _)| k.as_str())
        .collect();
    let listed_fmt: BTreeSet<&str> = APP_NAME_FMT_KEYS.iter().copied().collect();
    assert_eq!(
        found_fmt, listed_fmt,
        "APP_NAME_FMT_KEYS 가 소스의 t_fmt 호출과 다르다"
    );

    // 한 키를 두 방식으로 부르면 placeholder 규칙이 모순된다 — 그 자리를 먼저 정리해야 한다.
    let found_plain: BTreeSet<&str> = calls
        .iter()
        .filter(|(_, c)| *c == Call::Plain)
        .map(|(k, _)| k.as_str())
        .collect();
    let both: Vec<&&str> = found_fmt.intersection(&found_plain).collect();
    assert!(
        both.is_empty(),
        "같은 키를 t 와 t_fmt 로 모두 부른다 — placeholder 규칙이 갈린다: {both:?}"
    );
}

/// 추출기 자신의 극성 — 무엇을 잡고 무엇을 안 잡는지.
///
/// 이 픽스처가 없으면 위 두 테스트는 "추출기가 아무것도 안 잡는다" 여도 목록을 함께
/// 비우는 순간 통과한다. 극성을 여기서 단정한다.
#[test]
fn the_extractor_sees_calls_and_not_lookalikes() {
    let fixture = concat!(
        "let a = ",
        "t(\"real.plain\");\n",
        "let b = ",
        "t_fmt(\"real.fmt\", &name);\n",
        "info.insert(\"not.a.key\".into(), v);\n",
        "let c = x.expect(\"not.a.key.either\");\n",
        "// ",
        "t(\"in.a.comment\")\n",
        "/// ",
        "t_fmt(\"in.a.doc.comment\", x)\n",
        "let d = wide_null(",
        "t(\"nested.call\"));\n",
    );
    let got: Vec<(String, Call)> = calls_in(fixture);
    assert_eq!(
        got,
        vec![
            ("real.plain".to_string(), Call::Plain),
            ("real.fmt".to_string(), Call::Fmt),
            ("nested.call".to_string(), Call::Plain),
        ],
        "추출기의 극성이 달라졌다"
    );
}

/// 스캔 루트가 이 가드 자신을 포함하지 않는다.
///
/// 위 픽스처는 이 판정기가 찾는 패턴을 **자기 소스에 담고 있다.** 스캔 루트가 언젠가
/// 넓어져 이 파일을 삼키면 픽스처의 `real.plain` 같은 것이 실제 키로 집계되고, 그러면
/// 위 대조가 자기 자신을 상대로 성립한다. 면제를 두지 않고 루트를 좁게 유지하는 것으로
/// 막으며, 그 조건을 여기서 못박는다.
#[test]
fn the_scan_root_does_not_contain_this_guard() {
    let root = repo_root().join(PLATFORM_DIR);
    let me = Path::new(file!());
    assert!(
        !me.starts_with(PLATFORM_DIR),
        "이 가드({}) 가 스캔 루트({}) 안에 있다 — 자기 픽스처를 실제 키로 센다",
        me.display(),
        root.display()
    );
}
