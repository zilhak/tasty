//! 한 파일 안의 test 전용 블록을 줄 구조를 유지하며 가린다.
//! 별도 자식 파일 전체의 출하 여부는 shipping_scope에서 분류하므로 두 검사는 서로 대체할 수 없다.
//! 조건식은 공용 implies로 판단한다. not(test)나 any(test, unix)를 test 전용으로 지우면 안 된다.

use tasty_doc_guards::cfg_predicate::implies;

/// test를 요구하는 cfg 속성의 끝 다음 바이트 위치.
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

/// 블록 내용은 줄바꿈을 보존해 가린다. 중괄호 없는 mod name;의 별도 파일은 shipping_scope에서 처리한다.
pub(super) fn blank_test_modules(masked: &str) -> String {
    let bytes: Vec<char> = masked.chars().collect();
    let mut out: String = masked.to_string();
    for from in cfg_test_attr_ends(masked) {
        let Some(open) = masked[from..].find('{').map(|o| from + o) else {
            continue;
        };
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

    #[test]
    fn a_composite_cfg_that_implies_test_is_a_gate() {
        assert!(
            !blank_test_modules("#[cfg(all(test, feature = \"gui\"))]\nmod b {\n    X\n}\n")
                .contains('X')
        );
    }

    /// test 전용이 아닌 조건은 출하 코드를 포함할 수 있어 지우지 않는다.
    #[test]
    fn a_cfg_that_does_not_imply_test_is_left_alone() {
        assert!(
            blank_test_modules("#[cfg(not(test))]\n{\n    RELEASE\n}\n").contains("RELEASE"),
            "부정을 게이트로 읽어 프로덕션 블록을 지웠다"
        );
        assert!(
            blank_test_modules("#[cfg(any(test, unix))]\nmod b {\n    X\n}\n").contains('X'),
            "다른 조건으로도 활성화되는 any를 test 전용으로 판단했다"
        );
    }

    #[test]
    fn it_blanks_without_moving_line_numbers() {
        let src = "a\n#[cfg(test)]\nmod t {\n    X\n}\nb\n";
        let out = blank_test_modules(src);
        assert_eq!(out.lines().count(), src.lines().count());
        assert!(!out.contains('X'));
        assert_eq!(out.lines().next_back(), Some("b"));
    }
}
