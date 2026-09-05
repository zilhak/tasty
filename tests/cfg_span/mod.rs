//! `#[cfg(...)]` 가 실제로 덮는 줄 범위 — 줄 위치로 근사하지 않는다.
//!
//! 소스를 훑는 가드가 같은 함정에 세 번 빠졌다. 셋 다 **속성이 어디에 붙었는가**를
//! 줄 번호로 어림잡은 결과다:
//!
//! - 블록에 붙은 `#[cfg(debug_assertions)]` 를 못 보고 그 안의 팔을 release 로 셌다.
//! - `#[cfg(test)] mod x;`(선언 한 줄)을 모듈 **시작**으로 읽어 그 뒤를 통째로 잃었다.
//! - 속성 **앞**의 doc 주석을 그 항목 **밖**으로 봤다.
//!
//! 판정 기준은 관례가 아니라 *컴파일러가 무엇을 보느냐*다. 그래서 범위를 세 조각으로
//! 잡는다 — 속성 앞의 doc·속성 자신·항목 본문.
//!
//! 통합 테스트끼리는 서로를 import 할 수 없어(각자 독립 바이너리) 이 모듈을 `mod` 로
//! 함께 쓴다. 사본을 늘리지 않으려는 것이다 — 사본이 둘이면 갈리고, 갈린 쪽은 조용하다.

/// 한 줄의 중괄호 수지. **줄 주석·문자열·문자 리터럴 안은 세지 않는다** —
/// 문자열 속 `}` 에 속으면 블록이 일찍 닫히고 그 뒤가 게이트 밖으로 보인다.
pub fn brace_delta(line: &str) -> i32 {
    let b = line.as_bytes();
    let mut depth = 0i32;
    let mut i = 0usize;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            break;
        }
        if c == b'"' {
            in_str = true;
        } else if c == b'\'' {
            // 문자 리터럴 `'{'` — 라이프타임(`'a`)과 구분해 닫는 따옴표까지 건너뛴다.
            let esc = i + 1 < b.len() && b[i + 1] == b'\\';
            let end = if esc { i + 3 } else { i + 2 };
            if end < b.len() && b[end] == b'\'' {
                i = end + 1;
                continue;
            }
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
        }
        i += 1;
    }
    depth
}

/// cfg 술어가 `needle` 을 **함의**하는가.
///
/// 문자열 포함(`contains`)으로 어림잡으면 두 방향으로 틀린다 — `not(test)` 는 **반대**이고
/// `any(test, feature = "test-support")` 는 다른 조건으로도 컴파일된다. 실측(2026-09-05,
/// `src`·`crates`): `not(test)` 1 자리 · `any(test, …)` 4 자리 · `not(debug_assertions)`
/// 2 자리가 그 형태다. 그것들을 게이트 안으로 세면 **출하되는 코드가 스캔에서 사라진다** —
/// 위반이 아니라 침묵이라 조용히 비어 간다.
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

/// 이 줄의 속성이 `needle` 을 함의하는 `#[cfg(…)]` 인가.
fn attr_implies(line: &str, needle: &str) -> bool {
    let t = line.trim();
    let Some(pred) = t.strip_prefix("#[cfg(").and_then(|s| s.strip_suffix(")]")) else {
        return false;
    };
    implies(pred, needle)
}

/// `#[cfg(…)]` 가 `needle` 을 **함의할 때** 그 항목이 덮는 줄 전체를 `true` 로 표시한다.
///
/// 범위는 셋이다:
/// 1. 속성 **앞**의 doc 주석·다른 속성·빈 줄 — Rust 는 doc 주석을 **뒤따르는 항목**에
///    귀속시키므로, 이걸 빼면 `#[cfg(test)] mod` 위의 설명이 게이트 밖으로 보인다.
/// 2. 속성 줄 자신.
/// 3. 항목 본문 — 중괄호 수지가 0 으로 돌아올 때까지. 그 줄에서 블록이 열리지 않으면
///    한 줄짜리 항목이다(`#[cfg(test)] mod x;` 가 이 경우다 — 뒤를 삼키지 않는다).
pub fn cfg_gated_lines<S: AsRef<str>>(lines: &[S], needle: &str) -> Vec<bool> {
    let mut gated = vec![false; lines.len()];
    for (i, line) in lines.iter().enumerate() {
        if !attr_implies(line.as_ref(), needle) {
            continue;
        }
        gated[i] = true;

        // ① 속성 앞의 doc 주석·속성·빈 줄.
        for j in (0..i).rev() {
            let p = lines[j].as_ref().trim();
            if p.is_empty() || p.starts_with("///") || p.starts_with("#[") {
                gated[j] = true;
            } else {
                break;
            }
        }

        // ②③ 속성이 붙는 항목의 첫 줄 — 주석·빈 줄·다른 속성은 건너뛴다.
        let Some(start) = (i + 1..lines.len()).find(|&j| {
            let t = lines[j].as_ref().trim();
            !(t.is_empty() || t.starts_with("//") || t.starts_with("#["))
        }) else {
            continue;
        };
        for g in gated.iter_mut().take(start + 1).skip(i + 1) {
            *g = true;
        }
        let mut depth = brace_delta(lines[start].as_ref());
        let mut j = start;
        while depth > 0 && j + 1 < lines.len() {
            j += 1;
            gated[j] = true;
            depth += brace_delta(lines[j].as_ref());
        }
    }
    gated
}

#[cfg(test)]
mod cfg_span_tests {
    use super::cfg_gated_lines;

    fn gated(src: &str, needle: &str) -> Vec<bool> {
        let lines: Vec<&str> = src.lines().collect();
        cfg_gated_lines(&lines, needle)
    }

    /// 이 모듈이 생긴 이유 — 속성 **앞**의 doc 이 그 항목에 귀속된다.
    #[test]
    fn a_doc_comment_before_the_attribute_is_inside_the_gated_item() {
        let g = gated(
            "/// 설명\n///\n/// 두 번째 줄\n#[cfg(test)]\nmod pin {\n    fn a() {}\n}\nfn after() {}",
            "test",
        );
        assert_eq!(
            &g[0..3],
            [true, true, true],
            "속성 앞의 doc 세 줄이 게이트 밖으로 보인다"
        );
        assert!(g[4] && g[5] && g[6], "모듈 본문이 게이트 안이어야 한다");
        assert!(!g[7], "모듈이 닫힌 뒤는 게이트 밖이다");
    }

    /// 한 줄짜리 항목은 **뒤를 삼키지 않는다** — `#[cfg(test)] mod x;` 를 모듈 시작으로
    /// 읽어 파일 나머지를 통째로 잃은 사례가 있었다.
    #[test]
    fn a_single_line_item_does_not_swallow_what_follows() {
        let g = gated(
            "#[cfg(test)]\nmod x;\nfn shipped() {}\n/// 남아 있어야 한다",
            "test",
        );
        assert!(g[0] && g[1]);
        assert!(!g[2] && !g[3], "선언 한 줄 뒤가 게이트 안으로 먹혔다");
    }

    /// 문자열 안의 `}` 로 블록이 일찍 닫히면 그 뒤가 게이트 밖으로 보인다.
    #[test]
    fn a_brace_inside_a_string_does_not_close_the_gated_block() {
        let g = gated(
            "#[cfg(test)]\nmod m {\n    fn f() { let s = \"}\"; }\n    fn g() {}\n}\nfn after() {}",
            "test",
        );
        assert!(g[3], "문자열 속 `}}` 에 속아 블록이 일찍 닫혔다");
        assert!(!g[5], "블록이 닫힌 뒤까지 게이트로 셌다");
    }

    /// 게이트가 아닌 자리는 게이트가 아니다 — 반대 방향 대조.
    #[test]
    fn an_ungated_item_is_not_marked() {
        let g = gated(
            "/// clap 설명\nfn shipped() {}\n#[cfg(test)]\nfn t() {}",
            "test",
        );
        assert!(!g[0] && !g[1], "게이트 없는 항목을 게이트로 셌다");
        assert!(g[2] && g[3]);
    }

    /// needle 이 다르면 안 걸린다 — `debug_assertions` 축과 `test` 축은 별개다.
    #[test]
    fn the_needle_selects_which_cfg_axis_is_read() {
        let src = "#[cfg(debug_assertions)]\nfn d() {}\n#[cfg(test)]\nfn t() {}";
        let d = gated(src, "debug_assertions");
        let t = gated(src, "test");
        assert!(d[0] && d[1] && !d[2] && !d[3]);
        assert!(!t[0] && !t[1] && t[2] && t[3]);
    }
}
