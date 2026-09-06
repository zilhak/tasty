//! 토스트로 나가는 번역 문구가 **세 로케일 모두** 토스트 캡 안에 들어가는지 강제한다.
//!
//! 배경: `src/adapters/ui/toast.rs` 의 `MAX_MESSAGE_CHARS`(200자)를 넘으면 뒤가 잘리는
//! 데서 그치지 않는다 — host 가 `toast.char_limit_notice` 접미를 붙이므로 넘친 사실이
//! 화면에 남고, **잘려나가는 것은 문장 끝의 조치 안내**다. 안내가 없으면 그 문구가
//! 존재할 이유가 사라진다.
//!
//! 실제로 그랬다: `persistence.warn.*_blocked` 두 개가 영어에서만 캡을 넘어(248자 ·
//! 202자) 기본 언어에서 안내가 통째로 사라졌고, 앞 200자가 같은 형제 키와 화면상
//! 구별조차 되지 않았다. ko/ja 는 통과했기 때문에 세 파일을 눈으로 봐서는 드러나지
//! 않는다 — 길이는 언어마다 달라지므로 손으로 지킬 수 없다.
//!
//! `{}` 를 담은 문구는 [`tasty_i18n::t_fmt_fit`] 가 **끼워넣는 값만** 줄여 캡에 맞춘다.
//! 다만 그 축약에도 하한([`tasty_i18n::MIN_FRAGMENT_CHARS`])이 있어, 골격이 그만큼을 남기지
//! 못하면 여전히 넘친다. 그래서 판정은 "골격 + 경로 최소분 ≤ 캡" 이다.
//!
//! 선례: `tests/i18n_key_parity.rs`(같은 lang 파일 순회 · 평탄화). 언어 목록은 그 선례를
//! 따라 `tasty_i18n::BUILTIN_CODES` 를 가리킨다 — 여기서 다시 적지 않는다.

use std::collections::BTreeMap;
use std::path::Path;

/// 지원 언어 — 정본을 가리킨다. 여기에 목록을 다시 적으면 그것을 정본과 같게 유지하는
/// 것이 아무것도 없고, 어긋난 쪽은 빨강이 아니라 **조용한 축소**가 된다: 목록에서 빠진
/// 언어는 실패를 만드는 것이 아니라 이 검사를 안 받는다.
const LANGS: &[&str] = &tasty_i18n::BUILTIN_CODES;

/// 토스트로 나가는 키의 접두사와 그 근거. 여기 없는 키는 검사하지 않는다 — 모달 본문
/// 처럼 길어도 되는 문구까지 200자로 묶으면 캡의 의미가 엉뚱한 곳으로 번진다.
///
/// 새 문구를 토스트로 띄우기 시작하면 그 접두사를 여기에 추가한다.
const TOAST_KEY_PREFIXES: &[(&str, &str)] = &[(
    "persistence.warn.",
    "부팅 시 설정·레이아웃을 읽지 못한 사실을 알리는 Warning 토스트 \
     (`src/app/boot_machine.rs::report_persistence_incidents`)",
)];

#[test]
fn toast_strings_fit_the_toast_cap_in_every_locale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
    let cap = tasty_i18n::TOAST_MAX_CHARS;
    let mut problems = Vec::new();
    let mut checked = 0usize;

    for lang in LANGS {
        for (key, value) in load(&root, lang) {
            if !TOAST_KEY_PREFIXES.iter().any(|(p, _)| key.starts_with(p)) {
                continue;
            }
            checked += 1;
            let skeleton = value.replace("{}", "").chars().count();
            // `{}` 가 있으면 `t_fmt_fit` 가 그 값을 줄이지만 하한 아래로는 못 줄인다.
            let needed = skeleton
                + if value.contains("{}") {
                    tasty_i18n::MIN_FRAGMENT_CHARS
                } else {
                    0
                };
            if needed > cap {
                problems.push(format!(
                    "  lang/{lang}.toml `{key}`: 최악 {needed}자 (골격 {skeleton} + 경로 하한) > 캡 {cap}\n      {value:?}"
                ));
            }
        }
    }

    assert!(
        checked > 0,
        "토스트 키를 하나도 찾지 못했다 — TOAST_KEY_PREFIXES 가 실제 키와 어긋났다"
    );
    assert!(
        problems.is_empty(),
        "토스트 문구가 캡을 넘는다 — 넘치면 문장 끝의 조치 안내가 잘린다.\n\
         줄일 때는 **안내를 남기고 상황 설명을 줄인다**(안내를 문장 앞으로 옮긴다).\n{}",
        problems.join("\n")
    );
}

/// 검사 대상 접두사가 실제로 존재하는 키를 가리키는지 — 오타나 키 이름 변경으로
/// 위 테스트가 아무것도 검사하지 않은 채 초록이 되는 것을 막는다.
#[test]
fn every_declared_toast_prefix_matches_at_least_one_key() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
    let en = load(&root, "en");
    for (prefix, reason) in TOAST_KEY_PREFIXES {
        assert!(
            en.keys().any(|k| k.starts_with(prefix)),
            "lang/en.toml 에 `{prefix}` 로 시작하는 키가 없다 ({reason}) — \
             키를 옮겼으면 TOAST_KEY_PREFIXES 도 함께 고쳐라"
        );
    }
}

/// 캡 초과 안내 접미(`toast.char_limit_notice`)는 캡 값을 **인자로** 받는다 — 번역문에
/// 숫자를 적지 않는다.
///
/// 위 두 테스트는 문구가 캡 *안에 드는지* 를 보고, 이것은 캡 *값의 출처* 를 본다.
/// 접미가 자기 숫자를 들고 있으면 [`tasty_i18n::TOAST_MAX_CHARS`] 를 조정해도 화면
/// 문구는 옛 값을 계속 말한다 — 사용자가 보는 유일한 캡 설명이 세 언어에서 동시에
/// 틀리고, 그 거짓말은 컴파일도 위 테스트들도 통과한다.
#[test]
fn the_char_limit_notice_takes_the_cap_as_an_argument_in_every_locale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
    for lang in LANGS {
        let notice = load(&root, lang)
            .remove("toast.char_limit_notice")
            .unwrap_or_else(|| panic!("lang/{lang}.toml 에 toast.char_limit_notice 가 없다"));
        assert_eq!(
            notice.matches("{}").count(),
            1,
            "lang/{lang}.toml `toast.char_limit_notice` 는 캡 값을 받을 `{{}}` 를 정확히 \
             하나 가져야 한다 (현재: {notice:?})"
        );
        assert!(
            !notice.chars().any(|c| c.is_ascii_digit()),
            "lang/{lang}.toml `toast.char_limit_notice` 에 숫자가 박혀 있다 — 캡을 바꾸면 \
             이 문구가 거짓이 된다. `{{}}` 로 받아라 (현재: {notice:?})"
        );
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

fn flatten(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

// ── 호출부 초크포인트 ────────────────────────────────────────────────────────
//
// 위 테스트는 **문구가** 캡 안에 들어가는지만 본다. 경로가 붙는 순간 그 보장이
// 깨지므로, 경로를 담는 문구는 `tasty_i18n::t_fmt_fit`(= 끼우는 값만 줄이는 렌더)로만
// 나가야 한다. 그런데 `t_fmt` 로 바꿔 써도 어떤 테스트도 깨지지 않았다 — 문구 길이는
// lang 파일에서 오고 경로 길이는 런타임에서 오므로, 둘을 함께 보는 실행 경로가
// 단위 테스트에 없다. 그래서 호출부를 소스로 고정한다.
//
// 방식은 레포에 이미 있는 것을 따랐다: `tests/file_log_host_only_chokepoint.rs` 의
// 중괄호 깊이 기반 함수 본문 추출(`fn_span`). 문자열 검색으로 파일 전체를 훑으면
// 무관한 헬퍼나 doc 주석에 걸려 무해한 리팩터에도 깨진다.

/// 경로를 담는 persistence 경고를 만드는 함수. 여기 안에서는 `t_fmt` 금지.
const PATH_WARNING_FN: &str = "report_persistence_incidents";

#[test]
fn the_path_bearing_boot_warning_renders_through_the_eliding_helper() {
    let src_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/boot_machine.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", src_path.display()));
    let lines: Vec<&str> = src.lines().collect();
    let (start, end) = fn_span(&lines, PATH_WARNING_FN).unwrap_or_else(|| {
        panic!(
            "{}::{PATH_WARNING_FN} 를 못 찾았다 — 이름이 바뀌었으면 이 가드도 함께 옮겨라",
            src_path.display()
        )
    });

    let body = &lines[start..=end];
    let offenders: Vec<String> = body
        .iter()
        .enumerate()
        .filter(|(_, l)| !is_comment_line(l))
        .filter(|(_, l)| strip_line_comment(l).contains("t_fmt("))
        .map(|(i, l)| format!("  {}:{}: {}", src_path.display(), start + i + 1, l.trim()))
        .collect();

    assert!(
        offenders.is_empty(),
        "{PATH_WARNING_FN} 안에서 `t_fmt` 를 직접 불렀다. 경로가 길면 문구가 토스트 캡\n\
         (200자)을 넘고 **잘려나가는 것은 문장 끝의 조치 안내**다. `t_fmt_fit` 를 써라\n\
         (끼우는 값만 줄인다).\n{}",
        offenders.join("\n")
    );

    // 스캔이 실제로 무언가를 보고 있는지 — 본문이 비면 위 단정은 공허하게 통과한다.
    assert!(
        body.iter().any(|l| l.contains("t_fmt_fit(")),
        "{PATH_WARNING_FN} 본문에 `t_fmt_fit` 호출이 없다 — 가드가 헛돌고 있다"
    );
}

/// `fn <name>(` 의 본문 범위(시작 줄, 끝 줄). 중괄호 깊이로 닫는 줄을 찾는다.
fn fn_span(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    let header = format!("fn {name}(");
    let start = lines
        .iter()
        .position(|l| !is_comment_line(l) && l.contains(&header))?;
    let (mut depth, mut opened) = (0i32, false);
    for (i, line) in lines.iter().enumerate().skip(start) {
        for ch in strip_line_comment(line).chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return Some((start, i));
        }
    }
    None
}

/// 주석 줄인가 — 문서에서 이름을 언급하는 것은 호출이 아니다.
fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// 줄 끝 주석을 떼어낸다. 문자열 리터럴 안의 `//` 는 이 파일들에 없다.
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}
