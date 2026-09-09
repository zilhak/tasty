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
//! 네 번째는 성질이 다르다 — **줄 단위 렉싱**이었다. 여러 줄에 걸치는 리터럴은 줄
//! 하나만 봐서는 못 가르므로, 그 안이 코드로 보인다. 실측(2026-09-10)으로 이 트리에
//! 형태가 둘, 그것을 문 파일이 7 개 있었다:
//!
//! - **여러 줄 raw string**(`r#"{…}"#`) 3 파일 — 전부 JSON 픽스처.
//! - **`\` 로 이어붙인 여러 줄 문자열** 4 파일 — 그중 셋은 가드 자신의 합성 픽스처라
//!   그 문자열 **안의 `#[cfg(test)]` 가 속성으로도** 읽혔다.
//!
//! 결과는 양방향이고 **둘 다 실재했다** — 스팬이 일찍 닫혀 테스트 코드가 출하로
//! 세어지고(과다계상), 안 닫혀 **그 뒤의 출하 코드가 통째로 게이트 안으로 사라진다.**
//! 뒤쪽이 조용한 통과다. 그래서 스팬을 재는 함수들은
//! [`mask_non_code`](crate::source_text::mask_non_code) 로 덮은 사본에서 센다 —
//! 렉싱은 한 벌만 둔다.
//!
//! 판정 기준은 관례가 아니라 *컴파일러가 무엇을 보느냐*다. 그래서 범위를 세 조각으로
//! 잡는다 — 속성 앞의 doc·속성 자신·항목 본문.

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

/// 한 줄의 중괄호 수지. 한 줄 안에서 닫히는 줄 주석·문자열·문자 리터럴은 안 센다.
///
/// **줄 하나만 본다 — 여러 줄에 걸치는 것은 여기서 원리적으로 못 가른다.** raw string
/// (`r#"…"#`)과 블록 주석(`/* … */`)이 그것이다. 그래서 스팬을 재는 쪽은
/// [`mask_non_code`](crate::source_text::mask_non_code) 로 그것들을 **먼저 덮은
/// 사본**에서 센다([`cfg_gated_lines`]·[`cfg_attr_lines`] 가 안에서 그렇게 한다).
///
/// 안 덮으면 **두 방향으로** 틀린다. 리터럴 속 `}` 에 속으면 블록이 일찍 닫혀 그 뒤의
/// 테스트 코드가 출하로 세어지고(과다계상 — 값이 커져 시끄럽다), 리터럴 속 `{` 로
/// depth 가 올라간 채 **안 닫히면** 스팬이 파일 끝까지 늘어나 그 뒤의 출하 코드가
/// 통째로 게이트 안으로 사라진다. 뒤쪽이 이 doc 을 고치게 한 방향이다 — 그 오독의
/// 결과는 위반이 아니라 **침묵**이라 아무 게이트도 안 운다.
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

/// `#[cfg(…)]` 가 `needle` 을 **함의할 때** 그 항목이 덮는 줄 전체를 `true` 로 표시한다.
///
/// 범위는 셋이다:
/// 1. 속성 **앞**의 doc 주석·다른 속성·빈 줄 — Rust 는 doc 주석을 **뒤따르는 항목**에
///    귀속시키므로, 이걸 빼면 `#[cfg(test)] mod` 위의 설명이 게이트 밖으로 보인다.
/// 2. 속성 줄 자신.
/// 3. 항목 본문 — 중괄호 수지가 0 으로 돌아올 때까지. 그 줄에서 블록이 열리지 않으면
///    한 줄짜리 항목이다(`#[cfg(test)] mod x;` 가 이 경우다 — 뒤를 삼키지 않는다).
pub fn cfg_gated_lines<S: AsRef<str>>(lines: &[S], needle: &str) -> Vec<bool> {
    let masked = masked_lines(lines);
    let mut gated = vec![false; lines.len()];
    for i in 0..masked.len() {
        if !attr_implies(&masked[i], needle) {
            continue;
        }
        gated[i] = true;

        // ① 속성 앞의 doc 주석·속성·빈 줄. **여기는 원문을 본다** — 사본에서는 주석이
        // 공백이라 진짜 빈 줄과 안 갈리고, 그러면 이 스캔이 `///` 아닌 주석 덩이까지
        // 넘어 올라가 범위가 조용히 넓어진다.
        //
        // 그 선택의 대가는 이 스캔만의 반대 오독이다 — **여러 줄 리터럴의 마지막 줄이
        // 원문에서 `/// …"#;` 처럼 보이면** 이 스캔이 그것을 doc 으로 읽고 넘어 올라가,
        // 출하되는 문자열 내용이 게이트 안으로 빨려 든다. 방향이 조용한 쪽이라 적어 둔다.
        // 실측(2026-09-10) 전수: 원문에서 doc/속성/빈 줄로 보이는데 사본에서는 아닌 줄이
        // 트리에 6 개 있고, 그중 실제로 이 스캔에 먹힌 것은 **0** 이다(6 중 하나는 진짜
        // `#[cfg(test)]` 스팬 안이고 나머지 다섯은 스캔이 닿지 않는 자리다). 실례가
        // 생기기 전에 고치면 위 두 방향 중 어느 쪽을 택할지가 값 없이 결정된다.
        for j in (0..i).rev() {
            let p = lines[j].as_ref().trim();
            if p.is_empty() || p.starts_with("///") || p.starts_with("#[") {
                gated[j] = true;
            } else {
                break;
            }
        }

        // ②③ 속성이 붙는 항목의 첫 줄 — 주석·빈 줄·다른 속성은 건너뛴다.
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

/// 줄 목록을 **한 번 이어 붙여** [`mask_non_code`](crate::source_text::mask_non_code)
/// 를 먹인 뒤 다시 줄로 가른다.
///
/// 이어 붙이는 이유가 이 함수의 전부다 — raw string 과 블록 주석은 **여러 줄에
/// 걸치므로** 줄 단위로는 못 가른다([`brace_delta`] 의 한계). 사본은 줄 수와 줄
/// 번호를 보존하므로 결과 인덱스는 원본과 그대로 맞는다.
///
/// 이미 마스킹된 입력을 다시 먹여도 결과는 같다 — 덮인 자리에 따옴표도 `//` 도
/// 안 남는다. 그래서 마스킹한 사본을 넘겨 오던 소비자를 안 건드린다.
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
            // 속성 **바로 위**의 주석 덩이도 같이 표시한다. 억제 속성의 근거 주석은
            // 자유 산문이 아니라 `scripts/check-allow-reason.sh` 가 **그 자리에**
            // 있으라고 요구하는 선언의 일부다 — 속성이 출하 밖이면 그 근거도 함께
            // 나간다. 주석은 어떤 빌드에도 안 들어가므로 이 확장으로 실변경이 숨을
            // 여지는 없다.
            // **여기만 원문을 본다.** 사본에서는 주석이 공백이라 진짜 빈 줄과 안
            // 갈리고, 그러면 스캔이 위쪽 주석 덩이를 넘어 계속 올라간다.
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

/// `needle` 을 함의하는 `#[cfg(…)]` 가 덮는 줄과, 술어가 `needle` 을 요구하는
/// `cfg_attr` **속성 줄**을 빈 줄로 바꾼 사본. 줄 수는 그대로다.
///
/// 두 축을 **따로** 세는 이유는 범위가 다르기 때문이다. `#[cfg(test)]` 는 항목을
/// 통째로 들어내고, `cfg_attr` 은 붙는 **속성만** 조건부라 항목은 출하된다.
/// 한 판정으로 합치면 둘 중 하나는 틀린 범위를 쓴다.
///
/// 지운 자리를 **빈 줄로 남기는** 것은 줄 번호를 보존하려는 것이다 — 사본이 원본보다
/// 짧아지면 "지운 결과" 와 "안 읽힌 결과" 가 구분되지 않는다. 내용 동등을 묻는
/// 소비자는 비교 전에 빈 줄을 접어야 한다.
///
/// **소비자가 셋이라 여기 산다** (2026-09-10 실측 — `git grep` 전수):
///
/// - `crates/tasty-doc-guards/src/bin/strip-cfg-test.rs` — 이 사본을 파일로 써서
///   `tokei` 에게 세게 한다.
/// - `src/source_guards/headless_app_layer_coverage.rs` — "명부 밖에 이름이 사는가" 의
///   좌변을 만들 때 이 함수를 직접 부른다(사본 파일이 필요 없다).
/// - `crates/tasty-doc-guards/tests/agent_facing_reads_of_active_state_are_classified.rs`
///   의 `shipped_code` — 마스킹한 사본을 넣어 부른다.
///
/// **그 셋은 이 함수의 소비자다 — 층이 하나 더 있다.** 첫째 소비자는 CLI 판정기라, 그
/// 바이너리를 부르는 **셸 게이트가 또 셋**이다(2026-09-10 실측 — `resolve_judge
/// strip-cfg-test` 전수): `scripts/check-file-size.sh`(파일 SLOC) ·
/// `scripts/check-frozen-sum-ratchet.sh`(동결 총합) ·
/// `scripts/check-plugin-version-bump.sh`(plugin 버전). `scripts/gate-delta.sh` 는
/// 경로만 넘기고 판정하지 않으므로 소비자가 아니다.
///
/// **이 층을 안 세면 뒤쪽 하나가 빠진다.** 실제로 한 번 빠뜨렸다 — 이 렉싱을 고친
/// 회차가 앞의 두 게이트만 다시 재고 plugin 버전 게이트를 안 셌는데, 그쪽 좌변은
/// 움직였다(`crates/tasty-plugin-agent-stream/src/record.rs` 의 증거 사본 351 → 240,
/// 테스트 블록만 고친 변경의 판정이 "bump 요구" → "판정 대상 0" 으로 뒤집힌다 — 방향은
/// 안전). 실측은 `docs/adr/0166-the-plugin-version-gate-judges-the-artifact-not-the-directory.md`.
///
/// 소비자가 자기 사본을 만들면 같은 물음에 답이 둘이 되고, 갈린 답은 조용하다. 두
/// 형태를 실측으로 밟았다 — 앞의 갈림은 **출하되지도 않는 코드에 대한 영구 면제**라는
/// 처방으로 나타났고, 셋째 소비자는 **글자 그대로 같은 루프를 든 사본**이라 정본이
/// 렉싱을 고쳐도 안 따라올 자리였다.
///
/// **그래서 채널을 붙였다** — 이 모듈의 `one_span_judge` 가 [`cfg_gated_lines`] 와
/// [`cfg_attr_lines`] 를 둘 다 부르는 자리를 잡는다. 그 전까지 이 중복을 잡는 것은
/// `git grep` 밖에 없었다.
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

    /// **여러 줄 raw string 안의 중괄호는 코드가 아니다.** 줄 단위로 세면 그 `}` 가
    /// 블록을 일찍 닫아 **테스트 코드가 출하로 세어진다** — 실측(2026-09-10)으로
    /// `crates/tasty-ipc/src/stream.rs` · `crates/tasty-plugin-agent-stream/src/record.rs`
    /// · `src/core/child_terminal.rs` 셋이 이 형태였고, 셋 다 JSON 픽스처였다.
    #[test]
    fn braces_in_a_multi_line_raw_string_do_not_close_the_gated_block() {
        let g = gated(
            "#[cfg(test)]\nmod m {\n    const J: &str = r#\"{\n        \\\"a\\\": 1\n    }\"#;\n    fn t() {}\n}\nfn shipped() {}",
            "test",
        );
        assert!(
            g[5] && g[6],
            "raw string 속 `}}` 에 속아 블록이 일찍 닫혔다 — 테스트 코드가 출하로 세어진다"
        );
        assert!(!g[7], "블록이 닫힌 뒤까지 게이트로 셌다");
    }

    /// **반대 방향 — 스팬이 안 닫히는 형태.** 여러 줄 raw string 이 여는 `{` 만 담으면
    /// 줄 단위 계수에서 depth 가 올라간 채 **0 으로 안 돌아오고**, 스팬이 파일 끝까지
    /// 늘어나 그 뒤의 **출하 코드가 통째로 게이트 안으로 사라진다.** 위 시험(일찍
    /// 닫힘)은 테스트를 출하로 세는 과다계상이라 시끄럽지만, 이쪽은 **조용한 통과**다 —
    /// 명부 밖에서 답하는 함수가 잔여 검사에 안 보이는 자리가 그것이었다.
    #[test]
    fn an_unclosed_brace_in_a_raw_string_does_not_stretch_the_span_to_eof() {
        let g = gated(
            "#[cfg(test)]\nmod m {\n    const J: &str = r#\"\n{\n\"#;\n    fn t() {}\n}\nfn shipped() {}\nfn also_shipped() {}",
            "test",
        );
        assert!(
            !g[7] && !g[8],
            "raw string 이 여는 `{{` 만 담아 depth 가 안 닫혔고, 스팬이 파일 끝까지 늘어나 \
             출하 코드가 게이트 안으로 사라졌다: {g:?}"
        );
    }

    /// **블록 주석 안의 중괄호도 코드가 아니다.** 이 레포에 실례는 없지만 렉싱 축이
    /// 같아서 함께 못박는다 — 같은 마스킹 한 벌이 셋을 다 덮는다.
    #[test]
    fn braces_in_a_block_comment_do_not_move_the_span() {
        let g = gated(
            "#[cfg(test)]\nmod m {\n    /* 여는 중괄호 {\n       그리고 그것뿐 */\n    fn t() {}\n}\nfn shipped() {}",
            "test",
        );
        assert!(g[4] && g[5], "블록 주석 속 `{{` 에 스팬이 늘어났다");
        assert!(!g[6], "블록 주석 속 중괄호가 스팬을 파일 끝까지 늘렸다");
    }

    /// **문자열 안의 `#[cfg(test)]` 는 속성이 아니다.** 가드 자신의 합성 픽스처가 그
    /// 형태를 든다 — 실측(2026-09-10)으로 `src/source_guards/` 의 두 가드와
    /// `crates/tasty-doc-guards/tests/host_writes_nothing_to_stdout.rs` 가 그 자리였고,
    /// 픽스처 문자열의 본문이 **출하 코드 취급으로 지워지고 있었다.**
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

/// **스팬 판정을 이어 붙이는 자리는 여기 하나다** — 사본이 생기면 잡는다.
///
/// [`cfg_gated_lines`] 와 [`cfg_attr_lines`] 를 **둘 다** 부르는 코드는 사실상
/// [`blank_gated_lines`] 를 다시 쓰고 있는 것이다. 그 사본은 조용하다: 정본이 렉싱을
/// 고쳐도 안 따라오고, 갈린 답은 실패가 아니라 다른 수로 나온다. 실측(2026-09-10)으로
/// 그런 사본이 하나 있었고(`crates/tasty-doc-guards/tests/agent_facing_reads_of_active_state_are_classified.rs`
/// 의 `shipped_code`), **그것을 잡는 채널은 `git grep` 밖에 없었다.**
///
/// 좌변은 마스킹한 사본이다 — 주석·문자열이 이름을 인용하는 것은 호출이 아니다.
/// 한쪽만 부르는 자리는 대상이 아니다(범위가 다른 물음을 물을 수 있다).
///
/// **★ 과잉 포획 방향이 있다 — 좌변이 파일 단위다.** 한 파일 안에서 **서로 다른 물음을
/// 묻는 두 호출**이 각각 [`cfg_gated_lines`] 와 [`cfg_attr_lines`] 를 부르면, 그 파일은
/// 사본이 아닌데도 지목된다. 그리고 그때 실패문의 처방([`blank_gated_lines`] 를 불러라)은
/// **그 자리에서 이행 불가**다 — 두 물음의 범위가 다르니 합친 판정으로 대체할 수 없다.
/// 위 "한쪽만 부르는 자리는 대상이 아니다" 는 **단독 호출 면제**만 말하고 이 방향은
/// 말하지 않는다. 실측(2026-09-10)으로 **현재 실례는 0** 이다. 재현은 지금
/// `cfg_gated_lines` 만 부르는 `crates/tasty-doc-guards/tests/no_panic_in_window_creation.rs`
/// 에 `cfg_attr_lines` 호출 한 줄을 더하면 지목되는 것으로 확인했다.
///
/// **좁히지 않고 적어 둔다.** 좁히려면 두 호출이 같은 항목 안에 있는지를 봐야 하고,
/// 그러려면 훑는 파일의 항목 스팬을 이 가드가 따로 파싱해야 한다 — 이 모듈이 막으려는
/// 바로 그 **판정 사본**을 하나 더 만드는 것이다. 그 대가에 비해 이 방향은 값이 싸다:
/// 과잉 포획은 실패로 시끄럽고, **사본을 놓치는 반대 방향만이 조용하다.** 실례가 생기면
/// 그때 이 doc 과 실패문에 그 자리를 무엇으로 가를지를 함께 적는다 — 그 전에 좁히면
/// 실재하지 않는 것에 대한 처방이 된다.
#[cfg(test)]
mod one_span_judge {
    use crate::source_text::{mask_non_code, rust_sources};

    /// 이 판정을 이어 붙이는 정본. 여기서 둘을 다 부르는 것이 이 모듈의 존재 이유다.
    const HOME: &str = "crates/tasty-doc-guards/src/cfg_predicate.rs";

    #[test]
    fn only_one_place_joins_the_two_span_judges() {
        let root = crate::repo_root();
        let sources = rust_sources(&root, &["src", "crates", "tests"]);
        assert!(
            sources.len() > 500,
            "훑은 .rs 가 {} 개뿐이다 — 빈 모수의 잔여 0 은 통과가 아니라 미측정이다",
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
            "`cfg_gated_lines` 와 `cfg_attr_lines` 를 둘 다 부르는 자리는 \
             `blank_gated_lines` 를 다시 쓰고 있는 것이다 — 그 사본은 정본이 렉싱을 \
             고쳐도 안 따라오고, 갈린 답은 실패가 아니라 다른 수로 나온다. \
             `blank_gated_lines` 를 불러라: {copies:?}"
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
