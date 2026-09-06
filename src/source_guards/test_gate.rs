//! **한 파일 안에서** 테스트 게이트 뒤에 있는 구간을 지운다.
//!
//! # 이것은 "이 파일이 출하되는가" 가 아니다
//!
//! 그 물음의 판정기는 [`tasty_doc_guards::shipping_scope`] 다 — 선언 간선의 **전이
//! 폐쇄**를 따르고 `#[path = "..."]` 도 푼다. 여기 있는 것은 그보다 좁은 물음이다:
//!
//! | 물음 | 판정기 | 답의 모양 |
//! |---|---|---|
//! | 이 **파일**이 출하되나 | `shipping_scope::test_only_files` | 파일 집합 |
//! | 이 파일 **안의 어느 구간**이 게이트 뒤인가 | 여기 [`blank_test_modules`] | 같은 파일의 사본 |
//!
//! 둘은 서로를 대신하지 못한다. **출하되는 파일도 안에 `#[cfg(test)] mod tests { … }`
//! 를 품는다** — 파일 단위 판정은 그 구간을 못 지우고, 구간 판정은 자식 파일을 못 찾는다.
//! 그래서 파일 물음은 위임하고 여기서는 구간만 답한다.
//!
//! # 극성은 직접 안 센다
//!
//! 조건이 test 를 **함의하는가**는 [`tasty_doc_guards::cfg_predicate::implies`] 가
//! 판정한다. 처음엔 조건 안에 `test` 토큰이 있는지만 봤는데 그것은 두 방향으로 틀린다 —
//! `not(test)` 는 **반대**인데 게이트로 셌고(실측: `src/fullscreen_stages.rs` 의
//! `RELEASE_METAS` 블록이 스캔에서 지워지고 있었다), `any(test, unix)` 는 다른 조건으로도
//! 컴파일되는데 test 전용으로 셌다. 이쪽 오판은 **거짓 음성**이다 — 출하되는 코드를
//! 지워 놓고 위반이 없다고 말한다.

use tasty_doc_guards::cfg_predicate::implies;

/// 테스트 게이트 attribute 의 **끝 다음** 바이트 위치들. 조건이 test 를 **함의할 때만**
/// 게이트로 본다 — 판정은 [`implies`] 에 위임한다(모듈 문서 "극성은 직접 안 센다").
pub(super) fn cfg_test_attr_ends(masked: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = masked[from..].find("#[cfg(") {
        let at = from + rel;
        let body_start = at + "#[cfg(".len();
        from = body_start;
        let mut depth = 1usize;
        let mut end = None;
        for (i, c) in masked[body_start..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(body_start + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = end else { continue };
        if implies(&masked[body_start..close], "test") {
            // 닫는 `)` 다음의 `]` 까지 건너뛴다.
            let after = masked[close..]
                .char_indices()
                .find(|(_, c)| *c == ']')
                .map_or(close, |(i, c)| close + i + c.len_utf8());
            out.push(after);
        }
        from = close;
    }
    out
}

/// 테스트 게이트가 걸린 블록(`mod ... { … }` · 게이트가 붙은 식 블록)을 **줄 구조를
/// 보존한 채** 지운다. 줄 번호가 밀리면 이 사본을 쓰는 가드가 엉뚱한 줄을 가리킨다.
///
/// 중괄호가 없는 `mod name;` 은 **별도 파일**이라 여기 대상이 아니다 — 그 파일을 통째로
/// 빼는 것은 `shipping_scope::test_only_files` 의 일이다(모듈 문서의 표).
pub(super) fn blank_test_modules(masked: &str) -> String {
    let bytes: Vec<char> = masked.chars().collect();
    let mut out: String = masked.to_string();
    for from in cfg_test_attr_ends(masked) {
        let Some(open) = masked[from..].find('{').map(|o| from + o) else {
            continue;
        };
        // 여는 중괄호 앞에 세미콜론이 있으면 그 attribute 는 블록이 아니다.
        if masked[from..open].contains(';') {
            continue;
        }
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in bytes
            .iter()
            .enumerate()
            .skip(masked[..open].chars().count())
        {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close_char) = close else { continue };
        let start_byte = open;
        let end_byte = masked
            .char_indices()
            .nth(close_char)
            .map_or(masked.len(), |(b, c)| b + c.len_utf8());
        let region: String = masked[start_byte..end_byte]
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out.replace_range(start_byte..end_byte, &region);
    }
    out
}

#[cfg(test)]
mod detector {
    use super::*;

    /// 합성 조건도 테스트 게이트다. 문자열 비교로 판정하면 이 형태가 통째로 샌다 —
    /// 실측 자리는 `src/state.rs` 의 `fullscreen_stage_tests` 였다.
    #[test]
    fn a_composite_cfg_that_implies_test_is_a_gate() {
        assert!(
            !blank_test_modules("#[cfg(all(test, feature = \"gui\"))]\nmod b {\n    X\n}\n")
                .contains('X')
        );
    }

    /// ★ 극성. `not(test)` 는 **프로덕션 전용**이라 지우면 안 된다 — 지우면 출하되는
    /// 코드를 스캔에서 없애 놓고 "위반 없음" 이라고 말한다(거짓 음성). `any(test, …)` 도
    /// test 전용이 아니다.
    #[test]
    fn a_cfg_that_does_not_imply_test_is_left_alone() {
        assert!(
            blank_test_modules("#[cfg(not(test))]\n{\n    RELEASE\n}\n").contains("RELEASE"),
            "부정을 게이트로 읽어 프로덕션 블록을 지웠다"
        );
        assert!(
            blank_test_modules("#[cfg(any(test, unix))]\nmod b {\n    X\n}\n").contains('X'),
            "선언을 함의로 읽었다"
        );
    }

    /// 줄 번호가 밀리면 이 사본을 쓰는 가드가 엉뚱한 줄을 가리킨다.
    #[test]
    fn it_blanks_without_moving_line_numbers() {
        let src = "a\n#[cfg(test)]\nmod t {\n    X\n}\nb\n";
        let out = blank_test_modules(src);
        assert_eq!(out.lines().count(), src.lines().count());
        assert!(!out.contains('X'));
        assert_eq!(out.lines().next_back(), Some("b"));
    }
}
