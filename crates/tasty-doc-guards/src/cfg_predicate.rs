//! `#[cfg(...)]` 술어를 **읽는다** — 문자열로 어림잡지 않는다.
//!
//! 어림잡으면 두 방향으로 틀리고, 틀림의 결과는 위반이 아니라 **침묵**이다: 술어가
//! 게이트로 읽히면 그 줄들이 스캔 모수에서 빠진다.
//!
//! 줄 범위(`cfg_gated_lines`)도 여기 있다 — 소스를 훑는 가드가 같은 함정에 세 번 빠졌다.
//! 셋 다 **속성이 어디에 붙었는가**를 줄 번호로 어림잡은 결과다:
//!
//! - 블록에 붙은 `#[cfg(debug_assertions)]` 를 못 보고 그 안의 팔을 release 로 셌다.
//! - `#[cfg(test)] mod x;`(선언 한 줄)을 모듈 **시작**으로 읽어 그 뒤를 통째로 잃었다.
//! - 속성 **앞**의 doc 주석을 그 항목 **밖**으로 봤다.
//!
//! 판정 기준은 관례가 아니라 *컴파일러가 무엇을 보느냐*다. 그래서 범위를 세 조각으로
//! 잡는다 — 속성 앞의 doc·속성 자신·항목 본문.

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

/// 한 줄의 중괄호 수지. **줄 주석·문자열·문자 리터럴 안은 세지 않는다** —
/// 문자열 속 `}` 에 속으면 블록이 일찍 닫히고 그 뒤가 게이트 밖으로 보인다.
pub fn brace_delta(line: &str) -> i32 {
    delta(line, b'{', b'}')
}

/// 한 줄의 괄호 수지. `cfg_attr` 이 여러 줄에 걸칠 때 끝을 찾는 데 쓴다 —
/// [`brace_delta`] 와 같은 이유로 리터럴·주석 안은 세지 않는다.
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

/// 술어가 `needle` 을 **함의하는** `cfg_attr` 속성의 **줄만** 표시한다.
///
/// [`cfg_gated_lines`] 와 범위가 다르다 — `cfg_attr` 는 **항목을 지우지 않는다.**
/// `#[cfg_attr(test, derive(Debug))] struct S;` 에서 `struct S` 는 출하되고,
/// 조건부인 것은 붙는 속성뿐이다. 그래서 지울 것도 그 속성 줄뿐이다. 항목까지
/// 지우면 출하 코드가 조용히 스캔 밖으로 나간다 — 이 모듈이 막으려는 바로 그
/// 형태이고, 이 축에서 대가가 가장 큰 오류다.
///
/// 극성은 [`implies`] 가 가른다. `not(test)` 는 **프로덕션 전용**이라 지우면 안 되고,
/// `any(test, …)` 는 test 밖에서도 참이라 지우면 안 된다 — 둘 다 함의가 아니다.
///
/// 속성 바로 위의 주석 덩이는 함께 표시한다 — 근거 주석이 그 자리에 있어야 한다는
/// 요구가 별도 게이트에 있어서, 그 주석은 속성 선언의 일부다.
///
/// 여는 형태는 `#[cfg_attr(…)]`(바깥) 과 `#![cfg_attr(…)]`(안쪽, 크레이트/모듈 루트)
/// 둘 다다. 뒤쪽을 빠뜨린 것이 이 함수가 생긴 이유다 — 크레이트 루트의
/// `#![cfg_attr(test, …)]` 한 줄이 출하 산출물을 바꾸지 않는데도 내용이
/// 달라진 것으로 읽혔다.
pub fn cfg_attr_lines<S: AsRef<str>>(lines: &[S], needle: &str) -> Vec<bool> {
    let mut marked = vec![false; lines.len()];
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].as_ref().trim_start();
        if !(t.starts_with("#[cfg_attr(") || t.starts_with("#![cfg_attr(")) {
            i += 1;
            continue;
        }
        // 속성 하나를 끝까지 모은다 — 여러 줄에 걸칠 수 있다.
        let mut end = i;
        let mut depth = 0i32;
        let mut text = String::new();
        loop {
            let line = lines[end].as_ref();
            text.push_str(line);
            depth += paren_delta(line);
            if depth <= 0 || end + 1 >= lines.len() {
                break;
            }
            end += 1;
        }
        // 괄호가 안 닫혔으면 읽은 것이 아니다 — 넓게 남긴다(bump 를 한 번 더 요구할 뿐).
        if depth == 0 && cfg_attr_predicate(&text).is_some_and(|pred| implies(&pred, needle)) {
            for m in marked.iter_mut().take(end + 1).skip(i) {
                *m = true;
            }
            // 속성 **바로 위**의 주석 덩이도 같이 표시한다. 억제 속성의 근거 주석은
            // 자유 산문이 아니라 `scripts/check-allow-reason.sh` 가 **그 자리에**
            // 있으라고 요구하는 선언의 일부다 — 속성이 출하 밖이면 그 근거도 함께
            // 나간다. 주석은 어떤 빌드에도 안 들어가므로 이 확장으로 실변경이 숨을
            // 여지는 없다.
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

#[cfg(test)]
mod cfg_attr_tests {
    use super::cfg_attr_lines;

    fn marked(src: &str) -> Vec<bool> {
        let lines: Vec<&str> = src.lines().collect();
        cfg_attr_lines(&lines, "test")
    }

    /// 픽스처의 `cfg_attr` 속성 줄을 조립한다.
    ///
    /// **한때 억제 이름을 따로 끼워 넣었다** — `scripts/check-allow-reason.sh` 가 문자열
    /// 리터럴을 코드와 구분 못 해 이 파일의 픽스처를 사유 없는 억제 6 자리로 집계했기
    /// 때문이다. 그 우회의 대가는 **픽스처가 실물과 한 글자 달라지는 것**이었다: 다음
    /// 사람이 실물 형태로 되돌리면 게이트가 다시 깨지고 이유를 모른다. 그 게이트가
    /// 이제 마스킹 사본에서 세므로 우회를 걷었고, 여기 형태가 실물과 같다.
    fn attr(bang: &str, pred: &str) -> String {
        format!("#{bang}[cfg_attr({pred}, allow(clippy::x))]")
    }

    /// 이 함수가 생긴 형태 — 크레이트 루트의 안쪽 속성. 출하 빌드에서 `test` 는
    /// 안 켜지므로 이 줄은 산출물에 없다.
    #[test]
    fn an_inner_crate_attribute_that_requires_test_is_out_of_shipping() {
        let g = marked(&format!("{}\npub fn shipped() {{}}", attr("!", "test")));
        assert!(g[0], "크레이트 루트의 `#![cfg_attr(test, …)]` 를 못 봤다");
        assert!(!g[1], "출하되는 항목까지 지웠다");
    }

    /// **범위가 다르다** — `cfg_attr` 는 항목을 지우지 않는다. 여기서 항목까지
    /// 지우면 출하 코드가 조용히 스캔 밖으로 나간다.
    #[test]
    fn the_item_under_a_cfg_attr_still_ships() {
        let g = marked("#[cfg_attr(test, derive(Debug))]\npub struct S {\n    pub a: u8,\n}");
        assert!(g[0]);
        assert!(
            !g[1] && !g[2] && !g[3],
            "`cfg_attr` 이 붙은 항목을 통째로 지웠다"
        );
    }

    /// 반대 극성 둘. 이 둘이 지워지면 **프로덕션이 검사 밖으로 나간다** — 이 축에서
    /// 대가가 가장 큰 오류다.
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

    /// 억제 속성의 근거 주석은 그 자리에 있으라고 요구되는 선언의 일부라 함께
    /// 나간다. 위 항목 보존과 헷갈리면 안 된다 — 위는 속성 **아래**의 코드고
    /// 여기는 속성 **위**의 주석이다.
    #[test]
    fn the_mandated_reason_comment_goes_out_with_the_attribute() {
        let g = marked(
            "pub fn before() {}\n// 이유: 테스트 본문의 자리라\n// 명부에 섞이면 안 된다\n#![cfg_attr(test, allow(clippy::x))]\npub fn after() {}",
        );
        assert!(!g[0], "속성과 무관한 코드까지 지웠다");
        assert!(g[1] && g[2], "속성 위의 근거 주석이 남아 차분에 잡힌다");
        assert!(g[3]);
        assert!(!g[4], "속성 아래의 출하 코드를 지웠다");
    }

    /// `all(…)` 은 한 갈래만 요구해도 요구다 — [`super::implies`] 와 같은 판정.
    #[test]
    fn a_predicate_that_requires_test_among_others_is_stripped() {
        assert!(marked(&format!("{}\nfn f() {{}}", attr("", "all(test, unix)")))[0]);
    }

    /// 여러 줄에 걸친 속성도 끝까지 읽는다 — 첫 줄만 지우면 남은 줄이 차분에 남는다.
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
        // 반대 극성 — 이 셋이 참이 되면 출하 코드가 스캔에서 조용히 사라진다.
        assert!(!implies("not(test)", "test"));
        assert!(!implies("any(test, feature = \"test-support\")", "test"));
        assert!(!implies("feature = \"test-support\"", "test"));
        // needle 은 인자다 — `test` 전용이 아니다.
        assert!(implies("all(debug_assertions, unix)", "debug_assertions"));
        assert!(!implies("not(debug_assertions)", "debug_assertions"));
    }
}
