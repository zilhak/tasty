//! cfg 조건이 지정한 조건을 요구하는지 판단하고 적용 줄 범위를 찾는다.
//! 문자열·주석의 괄호나 cfg 예시를 코드로 읽지 않도록 파일 전체를 먼저 마스킹한다.
//! cfg 항목은 앞의 doc 주석·속성과 본문까지 포함하고, cfg_attr은 조건부 속성만 다룬다.

use crate::source_text::mask_non_code;

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

/// 한 줄의 여는 중괄호 수에서 닫는 수를 뺀다.
/// 여러 줄 문자열·주석은 이 함수만으로 구별할 수 없다. 호출 전에 파일 전체를 마스킹해야 한다.
pub fn brace_delta(line: &str) -> i32 {
    delta(line, b'{', b'}')
}

/// 한 줄의 괄호 수지. `cfg_attr` 이 여러 줄에 걸칠 때 끝을 찾는 데 쓴다 —
/// [`brace_delta`] 와 같은 한계(줄 하나만 본다)와 같은 처방(먼저 덮은 사본)이다.
pub fn paren_delta(line: &str) -> i32 {
    delta(line, b'(', b')')
}

fn delta(line: &str, open: u8, close: u8) -> i32 {
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
            // 문자 리터럴 `'{'` `'('` — 라이프타임(`'a`)과 구분해 닫는 따옴표까지 건너뛴다.
            let esc = i + 1 < b.len() && b[i + 1] == b'\\';
            let end = if esc { i + 3 } else { i + 2 };
            if end < b.len() && b[end] == b'\'' {
                i = end + 1;
                continue;
            }
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
        }
        i += 1;
    }
    depth
}

/// 이 줄의 속성이 `needle` 을 함의하는 `#[cfg(…)]` 인가.
fn attr_implies(line: &str, needle: &str) -> bool {
    let t = line.trim();
    let Some(pred) = t.strip_prefix("#[cfg(").and_then(|s| s.strip_suffix(")]")) else {
        return false;
    };
    implies(pred, needle)
}

/// needle을 요구하는 cfg 항목의 줄을 표시한다.
/// 앞의 doc 주석·속성·빈 줄, cfg 속성, 항목 본문을 포함한다.
/// 중괄호가 열리지 않는 mod 선언은 한 줄로 끝내며 뒤의 항목은 포함하지 않는다.
pub fn cfg_gated_lines<S: AsRef<str>>(lines: &[S], needle: &str) -> Vec<bool> {
    let masked = masked_lines(lines);
    let mut gated = vec![false; lines.len()];
    for i in 0..masked.len() {
        if !attr_implies(&masked[i], needle) {
            continue;
        }
        gated[i] = true;

        // doc 주석과 빈 줄을 구별하려고 이 구간만 원문으로 읽는다.
        // 한계: 여러 줄 문자열의 마지막 줄이 ///로 시작하면 doc 주석으로 오인할 수 있다.
        for j in (0..i).rev() {
            let p = lines[j].as_ref().trim();
            if p.is_empty() || p.starts_with("///") || p.starts_with("#[") {
                gated[j] = true;
            } else {
                break;
            }
        }

        let Some(start) = (i + 1..masked.len()).find(|&j| {
            let t = lines[j].as_ref().trim();
            !(t.is_empty() || t.starts_with("//") || t.starts_with("#["))
        }) else {
            continue;
        };
        for g in gated.iter_mut().take(start + 1).skip(i + 1) {
            *g = true;
        }
        let mut depth = brace_delta(&masked[start]);
        let mut j = start;
        while depth > 0 && j + 1 < masked.len() {
            j += 1;
            gated[j] = true;
            depth += brace_delta(&masked[j]);
        }
    }
    gated
}

/// 여러 줄 문자열·블록 주석을 구별하도록 전체를 합쳐 마스킹한 뒤 다시 줄로 나눈다.
/// 줄 수를 유지하며 이미 마스킹된 입력에도 사용할 수 있다.
fn masked_lines<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
    let joined: String = lines
        .iter()
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join("\n");
    mask_non_code(&joined)
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// `cfg_attr(<술어>, …)` 에서 술어만 떼어낸다. 첫 최상위 쉼표 앞이 술어다.
fn cfg_attr_predicate(attr: &str) -> Option<String> {
    let at = attr.find("cfg_attr(")? + "cfg_attr(".len();
    let rest = &attr[at..];
    let mut depth = 0usize;
    for (off, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    // 인자가 하나뿐인 `cfg_attr` 은 문법 오류다 — 술어로 읽지 않는다.
                    return None;
                }
                depth -= 1;
            }
            ',' if depth == 0 => return Some(rest[..off].to_string()),
            _ => {}
        }
    }
    None
}

/// needle을 요구하는 cfg_attr 속성 줄과 바로 앞의 근거 주석을 표시한다.
/// 항목 본문은 제거하지 않는다. 바깥 속성 #[cfg_attr]와 안쪽 속성 #![cfg_attr]를 지원한다.
/// not(test)나 any(test, …)처럼 test를 반드시 요구하지 않는 조건은 제외한다.
pub fn cfg_attr_lines<S: AsRef<str>>(lines: &[S], needle: &str) -> Vec<bool> {
    let masked = masked_lines(lines);
    let mut marked = vec![false; lines.len()];
    let mut i = 0usize;
    while i < masked.len() {
        let t = masked[i].trim_start();
        if !(t.starts_with("#[cfg_attr(") || t.starts_with("#![cfg_attr(")) {
            i += 1;
            continue;
        }
        // 속성 하나를 끝까지 모은다 — 여러 줄에 걸칠 수 있다.
        let mut end = i;
        let mut depth = 0i32;
        let mut text = String::new();
        loop {
            let line = &masked[end];
            text.push_str(line);
            depth += paren_delta(line);
            if depth <= 0 || end + 1 >= masked.len() {
                break;
            }
            end += 1;
        }
        // 괄호가 안 닫혔으면 읽은 것이 아니다 — 넓게 남긴다(bump 를 한 번 더 요구할 뿐).
        if depth == 0 && cfg_attr_predicate(&text).is_some_and(|pred| implies(&pred, needle)) {
            for m in marked.iter_mut().take(end + 1).skip(i) {
                *m = true;
            }
            // 속성의 근거 주석도 제외한다. 빈 줄과 주석을 구별하려고 원문을 읽는다.
            for j in (0..i).rev() {
                if lines[j].as_ref().trim_start().starts_with("//") {
                    marked[j] = true;
                } else {
                    break;
                }
            }
        }
        i = end + 1;
    }
    marked
}

/// cfg의 항목 범위와 cfg_attr의 속성 범위를 함께 비운 사본을 만든다.
/// 둘은 제거 범위가 다르므로 각각 판정하며 소비자는 이 결합 함수를 공유한다.
/// SLOC·동결 총합·플러그인 버전 검사도 CLI 도구를 통해 이 결과를 사용한다.
/// 줄 번호를 유지하므로 내용 비교에서는 남은 빈 줄을 정규화해야 한다.
pub fn blank_gated_lines(src: &str, needle: &str) -> String {
    let lines: Vec<&str> = src.split('\n').collect();
    let gated = cfg_gated_lines(&lines, needle);
    let attrs = cfg_attr_lines(&lines, needle);
    let mut out = String::with_capacity(src.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !gated[i] && !attrs[i] {
            out.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod cfg_span_tests {
    use super::cfg_gated_lines;

    fn gated(src: &str, needle: &str) -> Vec<bool> {
        let lines: Vec<&str> = src.lines().collect();
        cfg_gated_lines(&lines, needle)
    }

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

    #[test]
    fn an_ungated_item_is_not_marked() {
        let g = gated(
            "/// clap 설명\nfn shipped() {}\n#[cfg(test)]\nfn t() {}",
            "test",
        );
        assert!(!g[0] && !g[1], "게이트 없는 항목을 게이트로 셌다");
        assert!(g[2] && g[3]);
    }

    /// 여러 줄 raw 문자열의 닫는 괄호를 cfg 블록의 끝으로 읽지 않는다.
    #[test]
    fn braces_in_a_multi_line_raw_string_do_not_close_the_gated_block() {
        let g = gated(
            "#[cfg(test)]\nmod m {\n    const J: &str = r#\"{\n        \\\"a\\\": 1\n    }\"#;\n    fn t() {}\n}\nfn shipped() {}",
            "test",
        );
        assert!(
            g[5] && g[6],
            "raw string 속 `}}` 에 속아 블록이 일찍 닫혔다 — 테스트 코드가 제품 코드로 집계된다"
        );
        assert!(!g[7], "블록이 닫힌 뒤까지 게이트로 셌다");
    }

    /// 문자열의 여는 괄호 때문에 cfg 범위가 뒤의 제품 코드까지 늘어나면 안 된다.
    #[test]
    fn an_unclosed_brace_in_a_raw_string_does_not_stretch_the_span_to_eof() {
        let g = gated(
            "#[cfg(test)]\nmod m {\n    const J: &str = r#\"\n{\n\"#;\n    fn t() {}\n}\nfn shipped() {}\nfn also_shipped() {}",
            "test",
        );
        assert!(
            !g[7] && !g[8],
            "raw string 이 여는 `{{` 만 담아 depth 가 안 닫혔고, 스팬이 파일 끝까지 늘어나 \
             제품 코드가 게이트 안으로 사라졌다: {g:?}"
        );
    }

    #[test]
    fn braces_in_a_block_comment_do_not_move_the_span() {
        let g = gated(
            "#[cfg(test)]\nmod m {\n    /* 여는 중괄호 {\n       그리고 그것뿐 */\n    fn t() {}\n}\nfn shipped() {}",
            "test",
        );
        assert!(g[4] && g[5], "블록 주석 속 `{{` 에 스팬이 늘어났다");
        assert!(!g[6], "블록 주석 속 중괄호가 스팬을 파일 끝까지 늘렸다");
    }

    #[test]
    fn a_cfg_attribute_inside_a_string_is_not_an_attribute() {
        let g = gated(
            "fn shipped() {}\nconst FIXTURE: &str = \"\\\n#[cfg(test)]\nmod fixture {\n}\n\";\nfn also_shipped() {}",
            "test",
        );
        assert!(
            g.iter().all(|x| !x),
            "문자열 안의 `#[cfg(test)]` 를 속성으로 읽어 픽스처 본문을 지웠다: {g:?}"
        );
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

/// cfg_gated_lines와 cfg_attr_lines를 함께 사용하는 중복 구현을 찾는다.
/// 주석·리터럴의 언급과 한쪽만 호출하는 코드는 제외한다.
/// 파일 단위 검사라 서로 다른 목적으로 각각 호출해도 중복으로 보고할 수 있다.
/// 같은 범위를 결합하는 코드는 blank_gated_lines를 사용한다.
#[cfg(test)]
mod one_span_judge {
    use crate::source_text::{mask_non_code, rust_sources};

    const HOME: &str = "crates/tasty-doc-guards/src/cfg_predicate.rs";

    #[test]
    fn only_one_place_joins_the_two_span_judges() {
        let root = crate::repo_root();
        let sources = rust_sources(&root, &["src", "crates", "tests"]);
        assert!(
            sources.len() > 500,
            "읽은 .rs 파일이 {}개로 예상보다 적다. 저장소 루트와 순회 범위를 확인한다.",
            sources.len()
        );
        let copies: Vec<String> = sources
            .iter()
            .filter(|(rel, _)| rel.to_string_lossy() != HOME)
            .filter(|(_, text)| {
                let code = mask_non_code(text);
                code.contains("cfg_gated_lines(") && code.contains("cfg_attr_lines(")
            })
            .map(|(rel, _)| rel.to_string_lossy().into_owned())
            .collect();
        assert!(
            copies.is_empty(),
            "한 파일에서 `cfg_gated_lines`와 `cfg_attr_lines`를 모두 호출한다. \
             같은 범위를 결합한다면 `blank_gated_lines`를 사용한다. \
             서로 다른 목적의 호출인지도 확인한다: {copies:?}"
        );
    }
}

#[cfg(test)]
mod cfg_attr_tests {
    use super::cfg_attr_lines;

    fn marked(src: &str) -> Vec<bool> {
        let lines: Vec<&str> = src.lines().collect();
        cfg_attr_lines(&lines, "test")
    }

    fn attr(bang: &str, pred: &str) -> String {
        format!("#{bang}[cfg_attr({pred}, allow(clippy::x))]")
    }

    #[test]
    fn an_inner_crate_attribute_that_requires_test_is_out_of_shipping() {
        let g = marked(&format!("{}\npub fn shipped() {{}}", attr("!", "test")));
        assert!(g[0], "크레이트 루트의 `#![cfg_attr(test, …)]` 를 못 봤다");
        assert!(!g[1], "test 전용이 아닌 항목까지 지웠다");
    }

    #[test]
    fn the_item_under_a_cfg_attr_still_ships() {
        let g = marked("#[cfg_attr(test, derive(Debug))]\npub struct S {\n    pub a: u8,\n}");
        assert!(g[0]);
        assert!(
            !g[1] && !g[2] && !g[3],
            "`cfg_attr` 이 붙은 항목을 통째로 지웠다"
        );
    }

    #[test]
    fn a_predicate_that_holds_outside_test_is_never_stripped() {
        assert!(
            !marked("#[cfg_attr(not(test), deny(warnings))]\nfn f() {}")[0],
            "`not(test)` 는 프로덕션 전용이다"
        );
        assert!(
            !marked(&format!("{}\nfn f() {{}}", attr("", "any(test, unix)")))[0],
            "`any(test, …)` 는 test 밖에서도 참이다"
        );
        assert!(
            !marked(&format!("{}\nfn f() {{}}", attr("", "feature = \"x\"")))[0],
            "test 와 무관한 술어다"
        );
    }

    #[test]
    fn the_mandated_reason_comment_goes_out_with_the_attribute() {
        let g = marked(
            "pub fn before() {}\n// 이유: 테스트 본문의 자리라\n// 명부에 섞이면 안 된다\n#![cfg_attr(test, allow(clippy::x))]\npub fn after() {}",
        );
        assert!(!g[0], "속성과 무관한 코드까지 지웠다");
        assert!(g[1] && g[2], "속성 위의 근거 주석이 남아 차분에 잡힌다");
        assert!(g[3]);
        assert!(!g[4], "속성 아래의 제품 코드를 지웠다");
    }

    #[test]
    fn a_predicate_that_requires_test_among_others_is_stripped() {
        assert!(marked(&format!("{}\nfn f() {{}}", attr("", "all(test, unix)")))[0]);
    }

    #[test]
    fn a_multi_line_attribute_is_marked_to_its_end() {
        let g = marked("#[cfg_attr(\n    test,\n    deny(clippy::x)\n)]\nfn shipped() {}");
        assert!(
            g[0] && g[1] && g[2] && g[3],
            "속성이 여러 줄인데 일부만 지웠다"
        );
        assert!(!g[4], "속성 뒤의 항목까지 지웠다");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_predicate_implies_only_what_it_forces() {
        assert!(implies("test", "test"));
        assert!(implies("all(test, unix)", "test"));
        assert!(implies("all(unix, all(test, windows))", "test"));
        assert!(!implies("not(test)", "test"));
        assert!(!implies("any(test, feature = \"test-support\")", "test"));
        assert!(!implies("feature = \"test-support\"", "test"));
        assert!(implies("all(debug_assertions, unix)", "debug_assertions"));
        assert!(!implies("not(debug_assertions)", "debug_assertions"));
    }
}
