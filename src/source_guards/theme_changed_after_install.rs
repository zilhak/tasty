//! `theme.changed` 구독자는 수신 후 `theme.query`로 전역 Theme를 되읽는다. 발행은 새 Theme 설치 뒤여야 한다.
//! `install_theme_then`의 시험은 헬퍼 안에서 설치가 클로저 실행보다 먼저인지만 확인하므로,
//! 이 검사는 제품 소스에서 발행 코드가 그 클로저 안에서만 실행되는 형태인지 확인한다.
//!
//! 본체 `src/`(검사 디렉터리 제외)를 읽어 두 조건을 텍스트로 비교한다.
//! `"theme.changed"` 리터럴은 모두 `fn announce_settings_change` 안에 있어야 하고,
//! `announce_settings_change(` 호출은 모두 `install_theme_then(` 호출의 두 번째 인자 안에 있어야 한다.
//! 발행 리터럴은 `if appearance_changed {` 블록 안에도 있어야 한다. 테마 ID만 비교하는 옛 조건으로
//! 돌아가면 기본 색·색 override·라이트 여부·UI 배율 변경 때 발행하지 않는데, 판정 함수의 시험은 발행부가 그 판정을
//! 쓰는지까지는 보지 못하기 때문이다. 그 값이 `theme_order::appearance_changed`에서 왔는지는 보지 않는다.
//! 클로저를 변수로 받아 넘기는 형태, 상수·매크로로 키를 감춘 발행, `crates/`의 발행은 보지 못한다.
//! 클로저 안의 실행 여부·횟수와 실제 실행 순서를 증명하지 않으며 실행 순서는 theme_order 시험이 맡는다.

use super::{fn_spans, line_of, matching_delim, rust_sources};

const EVENT_LITERAL: &str = "\"theme.changed\"";
const ANNOUNCER: &str = "announce_settings_change";
const GATE: &str = "if appearance_changed {";
const INSTALLER: &str = "install_theme_then";
const GUARD_DIR: &str = "src/source_guards/";

/// 한 파일에서 찾은 위반과 검사한 발행 리터럴·announce 호출 수.
#[derive(Debug, Default, PartialEq)]
struct Verdict {
    violations: Vec<String>,
    emits: usize,
    announces: usize,
}

/// 식별자 바로 뒤에 `(`가 오는 호출 위치. 정의(`fn name(`)와 다른 이름의 일부는 제외한다.
fn call_sites(code: &str, name: &str) -> Vec<usize> {
    let needle = format!("{name}(");
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    code.match_indices(&needle)
        .map(|(at, _)| at)
        .filter(|&at| {
            let before = &code[..at];
            if before.chars().next_back().is_some_and(ident) {
                return false;
            }
            let head = before.trim_end();
            let is_def = head.ends_with("fn")
                && !head[..head.len() - 2]
                    .chars()
                    .next_back()
                    .is_some_and(ident);
            !is_def
        })
        .collect()
}

/// `install_theme_then(` 호출의 두 번째 인자 바이트 범위. 인자가 둘이 아니면 None.
fn second_arg_span(code: &str, call_at: usize) -> Option<(usize, usize)> {
    let open = code[call_at..].find('(')? + call_at;
    let close = matching_delim(code, open)?;
    let mut depth = 0usize;
    let mut comma = None;
    for (offset, c) in code[open..close].char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 1 => {
                if comma.is_some() {
                    return None;
                }
                comma = Some(open + offset);
            }
            _ => {}
        }
    }
    comma.map(|c| (c + 1, close))
}

/// 원문을 받아 판정한다. 구조는 리터럴까지 가린 사본에서, 이벤트 키는 주석만 가린 사본에서 찾는다.
fn judge(rel: &str, src: &str) -> Verdict {
    let code = tasty_doc_guards::source_text::mask_non_code_aligned(src);
    let with_literals = tasty_doc_guards::source_text::mask_comments_aligned(src);
    let spans = fn_spans(&code);
    let gates: Vec<(usize, usize)> = code
        .match_indices(GATE)
        .filter_map(|(at, _)| {
            let open = at + GATE.len() - 1;
            matching_delim(&code, open).map(|close| (open, close))
        })
        .collect();
    let mut verdict = Verdict::default();

    for (at, _) in with_literals.match_indices(EVENT_LITERAL) {
        verdict.emits += 1;
        let inside = spans
            .iter()
            .filter(|(_, open, close, _)| *open < at && at < *close)
            .any(|(name, ..)| name == ANNOUNCER);
        if !inside {
            verdict.violations.push(format!(
                "{rel}:{}: `{EVENT_LITERAL}`가 `fn {ANNOUNCER}` 밖에 있다. 이 이벤트는 `{INSTALLER}`가 새 Theme를 설치한 뒤 실행하는 `{ANNOUNCER}` 안에서만 발행한다.",
                line_of(&code, at)
            ));
        }
        if !gates.iter().any(|(open, close)| *open < at && at < *close) {
            verdict.violations.push(format!(
                "{rel}:{}: `{EVENT_LITERAL}`가 `{GATE}` 블록 밖에 있다. `appearance_changed` 가 참이면(테마 ID·기본 색·색 override·라이트 여부·UI 배율 중 하나라도 바뀌면) 발행한다.",
                line_of(&code, at)
            ));
        }
    }

    let installs: Vec<(usize, usize)> = call_sites(&code, INSTALLER)
        .into_iter()
        .filter_map(|at| second_arg_span(&code, at))
        .collect();
    for at in call_sites(&code, ANNOUNCER) {
        verdict.announces += 1;
        if !installs.iter().any(|(lo, hi)| *lo < at && at < *hi) {
            verdict.violations.push(format!(
                "{rel}:{}: `{ANNOUNCER}(` 호출이 `{INSTALLER}(..)`의 클로저 인자 밖에 있다. 설치 전에 발행하면 `theme.changed` 구독자가 `theme.query`로 직전 테마를 읽는다.",
                line_of(&code, at)
            ));
        }
    }
    verdict
}

fn product_sources() -> Vec<(String, String)> {
    rust_sources()
        .into_iter()
        .map(|(rel, src)| (rel.to_string_lossy().replace('\\', "/"), src))
        .filter(|(rel, _)| rel.starts_with("src/") && !rel.starts_with(GUARD_DIR))
        .collect()
}

#[test]
fn theme_changed_is_emitted_only_inside_the_install_closure() {
    let mut violations = Vec::new();
    let (mut emits, mut announces) = (0usize, 0usize);
    for (rel, src) in product_sources() {
        let verdict = judge(&rel, &src);
        violations.extend(verdict.violations);
        emits += verdict.emits;
        announces += verdict.announces;
    }
    assert!(
        emits > 0 && announces > 0,
        "검사 대상이 없다(`{EVENT_LITERAL}` {emits}개, `{ANNOUNCER}(` 호출 {announces}개). 이름이 바뀌었으면 이 검사의 상수와 범위를 함께 고친다."
    );
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[cfg(test)]
mod judge_tests {
    use super::*;

    const GOOD: &str = r#"
impl App {
    fn apply(&mut self) {
        theme_order::install_theme_then(&s, || {
            self.announce_settings_change(&s)
        });
    }
    fn announce_settings_change(&mut self, s: &Settings) {
        // "theme.changed" in a comment is not an emit
        if appearance_changed {
            mgr.emit_host_event("theme.changed", &p, scope);
        }
    }
}
"#;

    #[test]
    fn the_current_shape_passes() {
        let verdict = judge("good.rs", GOOD);
        assert_eq!(
            verdict,
            Verdict {
                violations: Vec::new(),
                emits: 1,
                announces: 1,
            }
        );
    }

    /// 발행을 먼저 하고 빈 클로저로 설치하는 변이.
    #[test]
    fn announcing_before_install_is_rejected() {
        let src = GOOD.replace(
            "theme_order::install_theme_then(&s, || {\n            self.announce_settings_change(&s)\n        });",
            "self.announce_settings_change(&s);\n        theme_order::install_theme_then(&s, || ());",
        );
        assert_ne!(src, GOOD, "변이 치환이 적용되지 않았다");
        let verdict = judge("mutant.rs", &src);
        assert_eq!(verdict.violations.len(), 1, "{:?}", verdict.violations);
    }

    /// 발행 키를 announce 밖에서 쓰는 변이.
    #[test]
    fn an_emit_outside_the_announcer_is_rejected() {
        let src = format!(
            "{GOOD}\nfn elsewhere() {{ if appearance_changed {{ mgr.emit_host_event(\"theme.changed\", &p, scope); }} }}\n"
        );
        let verdict = judge("mutant.rs", &src);
        assert_eq!(verdict.emits, 2);
        assert_eq!(verdict.violations.len(), 1, "{:?}", verdict.violations);
    }

    /// 테마 ID만 비교하던 옛 발행 조건으로 되돌리는 변이.
    #[test]
    fn gating_on_the_theme_id_alone_is_rejected() {
        let src = GOOD.replace(
            "if appearance_changed {",
            "if prev_theme.as_deref() != Some(s.appearance.theme.as_str()) {",
        );
        assert_ne!(src, GOOD, "변이 치환이 적용되지 않았다");
        let verdict = judge("mutant.rs", &src);
        assert_eq!(verdict.violations.len(), 1, "{:?}", verdict.violations);
    }

    /// 설치 호출의 첫 인자 안에 숨긴 announce는 클로저 안으로 보지 않는다.
    #[test]
    fn an_announce_in_the_first_argument_is_rejected() {
        let src = GOOD.replace(
            "install_theme_then(&s, || {\n            self.announce_settings_change(&s)\n        });",
            "install_theme_then({ self.announce_settings_change(&s); &s }, || ());",
        );
        assert_ne!(src, GOOD, "변이 치환이 적용되지 않았다");
        let verdict = judge("mutant.rs", &src);
        assert_eq!(verdict.violations.len(), 1, "{:?}", verdict.violations);
    }
}
