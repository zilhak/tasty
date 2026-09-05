//! `#[cfg(...)]` 술어를 **읽는다** — 문자열로 어림잡지 않는다.
//!
//! 어림잡으면 두 방향으로 틀리고, 틀림의 결과는 위반이 아니라 **침묵**이다: 술어가
//! 게이트로 읽히면 그 줄들이 스캔 모수에서 빠진다.

/// cfg 술어가 `needle` 을 **함의**하는가.
///
/// 문자열 포함으로 어림잡으면 두 방향으로 틀린다 — `not(test)` 는 **반대**이고
/// `any(test, feature = "test-support")` 는 다른 조건으로도 컴파일된다.
pub fn implies(pred: &str, needle: &str) -> bool {
    let p: String = pred.chars().filter(|c| !c.is_whitespace()).collect();
    if p == needle {
        return true;
    }
    let Some(inner) = p.strip_prefix("all(").and_then(|s| s.strip_suffix(')')) else {
        return false;
    };
    let mut depth = 0usize;
    let mut cur = String::new();
    let mut parts = Vec::new();
    for ch in inner.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    parts.iter().any(|part| implies(part, needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_predicate_implies_only_what_it_forces() {
        assert!(implies("test", "test"));
        assert!(implies("all(test, unix)", "test"));
        assert!(implies("all(unix, all(test, windows))", "test"));
        // 반대 극성 — 이 셋이 참이 되면 출하 코드가 스캔에서 조용히 사라진다.
        assert!(!implies("not(test)", "test"));
        assert!(!implies("any(test, feature = \"test-support\")", "test"));
        assert!(!implies("feature = \"test-support\"", "test"));
        // needle 은 인자다 — `test` 전용이 아니다.
        assert!(implies("all(debug_assertions, unix)", "debug_assertions"));
        assert!(!implies("not(debug_assertions)", "debug_assertions"));
    }
}
