//! `objc2` 의 `define_class!` / `declare_class!` 본문에서 **값을 돌려주는
//! `return`** 을 금지한다.
//!
//! 그 매크로는 본문을 `let __objc2_result = { ...본문... };` 로 감싸 자기가 만든
//! `extern "C-unwind"` shim 에 심는다. shim 의 반환 타입은 소스에 적힌 타입이
//! 아니라 **변환된** `<T as ConvertReturn<_>>::Inner` 다(`bool` → `Bool`,
//! `Retained<_>` → 별도 표현). 그래서 `return <값>` 은 사용자가 쓴 함수가 아니라
//! shim 을 빠져나가며 변환 후 타입으로 검사돼 컴파일이 깨진다 — 반면 꼬리
//! 표현식은 변환 전 타입으로 추론되므로 멀쩡하다. 한 함수 안에서 두 경로의 기대
//! 타입이 다르다.
//!
//! **이 함정은 macOS 에서만 컴파일된다** — Linux·Windows 개발자는 로컬에서 볼 수
//! 없고 CI 의 macOS 잡만 본다. 그래서 소스 스캔으로 전 플랫폼에서 막는다.
//!
//! ## 면제와 그 근거
//!
//! - **값 없는 `return;` 은 허용한다**(A-1). 반환 타입이 없는 메서드에서는 매크로가
//!   변환 없이 전개하므로 합법이다. 이 면제의 경계는
//!   `catches_a_value_return_split_across_lines` 가 지킨다 — 줄바꿈이 끼어도 값
//!   반환은 값 반환이다.
//! - **주석·문자열 안은 보지 않는다**(A-2, `mask_non_code` 공통). 그 창 안쪽에
//!   진짜 위반을 심어도 잡히는지는
//!   `catches_a_real_return_next_to_a_commented_one` 이 확인한다.
//!
//! ## 의도적으로 넓게 잡는 곳
//!
//! 반환 타입이 `EncodeReturn` 을 그대로 만족하는 타입(예: `NSRect`)이면
//! `return <값>` 도 사실은 합법이지만, 텍스트로는 그 구분을 못 하므로 **일괄
//! 금지**한다. 과검출 방향이라 면제가 아니고, 표현식 형태로 쓰면 어느 경우든 옳다.

use super::*;

/// 스캔 하한 — 이 레포에는 `define_class!` 블록이 실제로 존재한다. 0 개가 되면
/// 가드가 아무것도 안 보고 통과하는 것이므로, 그때는 이 하한을 의도적으로 고쳐야 한다.
const MIN_BLOCKS: usize = 1;

const MACROS: &[&str] = &["define_class!", "declare_class!"];
const RETURN: &str = "return";

/// 마스킹된 소스 하나에 대한 판정 결과. 줄 번호는 1-based.
struct Scan {
    /// 찾은 매크로 블록 수(스캔 하한용).
    blocks: usize,
    /// 값 반환 `return` 이 있는 줄.
    violations: Vec<usize>,
    /// 구분자가 닫히지 않은 매크로 시작 줄 — 마스킹이 깨졌다는 신호다.
    unclosed: Vec<usize>,
}

/// 레포 전수 테스트와 합성 입력 테스트가 함께 부르는 판정기.
fn scan(masked: &str) -> Scan {
    let mut out = Scan {
        blocks: 0,
        violations: Vec::new(),
        unclosed: Vec::new(),
    };
    for mac in MACROS {
        for start in word_positions(masked, mac) {
            let Some(open) = next_opening_delim(masked, start) else {
                out.unclosed.push(line_of(masked, start));
                continue;
            };
            let Some(end) = matching_delim(masked, open) else {
                out.unclosed.push(line_of(masked, start));
                continue;
            };
            out.blocks += 1;
            let body = &masked[open..end];
            for rel in word_positions(body, RETURN) {
                let rest = body[rel + RETURN.len()..].trim_start();
                if !rest.starts_with(';') {
                    out.violations.push(line_of(masked, open + rel));
                }
            }
        }
    }
    out
}

#[test]
fn no_value_returning_return_inside_define_class() {
    let mut blocks = 0usize;
    let mut violations = Vec::new();
    let mut unclosed = Vec::new();
    for (path, text) in rust_sources() {
        let found = scan(&mask_non_code(&text));
        blocks += found.blocks;
        for line in found.violations {
            violations.push(format!("{}:{line}", path.display()));
        }
        for line in found.unclosed {
            unclosed.push(format!("{}:{line}", path.display()));
        }
    }
    assert!(
        unclosed.is_empty(),
        "매크로 호출의 구분자가 닫히지 않는다 — 마스킹이 깨졌을 수 있다.\n  {}",
        unclosed.join("\n  ")
    );
    assert!(
        blocks >= MIN_BLOCKS,
        "스캔 하한 미달: {mac_list} 블록을 {blocks} 개 찾았다(하한 {MIN_BLOCKS}). \
         블록이 정말 사라졌다면 이 하한을 함께 고쳐라",
        mac_list = MACROS.join(" / "),
    );
    assert!(
        violations.is_empty(),
        "define_class!/declare_class! 본문에서 값을 돌려주는 `return` 은 매크로가 만든 \
         shim 을 빠져나가 변환 후 타입(예: bool → objc2::runtime::Bool)으로 검사된다 — \
         macOS 에서만 컴파일이 깨진다. 값은 표현식으로 흘려라(if/else 또는 match).\n  {}",
        violations.join("\n  ")
    );
}

/// 이 가드가 겨냥하는 유일한 실물. 이 파일은 부모(`src/host_api/webview.rs`)의
/// `#[cfg(target_os = "macos")]` 와 조부모(`src/host_api.rs`)의
/// `#[cfg(feature = "gui")]` 두 게이트 아래 있어 **Linux 의 어느 조합에서도
/// 컴파일되지 않는다.** 게이트가 파일 자신에도 부모에도 없고 조부모에 있으므로,
/// 이 파일만 열어 `cfg` 를 찾으면 "게이트 없음" 으로 잘못 읽힌다.
const GATED_FILE: &str = "src/host_api/webview/macos.rs";

/// 스캔이 실제로 읽어온 그 파일의 내용. 없으면 스캔이 거기 못 닿은 것이다.
fn gated_source() -> String {
    rust_sources()
        .into_iter()
        .find(|(path, _)| path.to_string_lossy().replace('\\', "/") == GATED_FILE)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("스캔 결과에 {GATED_FILE} 이 없다"))
}

/// `MIN_BLOCKS` 하한은 **어딘가에서** 1개를 봤다는 것까지만 말한다. 그 1개가
/// **컴파일된 적 없는 이 파일에서 왔다**는 것은 하한이 못 보여준다 — 여기서 직접
/// 못박는다. 이 단정이 통과하는 것은 소스 스캔이 `cfg` 와 무관하게 대상을 본다는
/// 뜻이고, 그것이 이 모듈을 `tests/` 가 아니라 여기 둔 이유의 절반이다.
#[test]
fn the_scan_reaches_a_file_no_local_build_compiles() {
    let found = scan(&mask_non_code(&gated_source()));
    assert!(
        found.blocks > 0,
        "{GATED_FILE} 에서 매크로 블록을 하나도 못 찾았다 — 스캔이 이 파일에 닿지 \
         못했거나 마스킹이 본문까지 지웠다"
    );
    assert!(
        found.violations.is_empty(),
        "실물 파일이 이미 위반을 담고 있다: {:?}",
        found.violations
    );
}

/// 위 단정은 "읽었다" 까지다. **읽은 것 안에 진짜 위반이 있으면 잡는가** 는 따로
/// 물어야 한다 — 판정기가 대상을 보면서도 아무것도 못 보는 상태를 배제한다.
/// 파일을 고치지 않고 읽어온 내용에 주입해서 확인하므로 트리는 그대로다.
#[test]
fn a_violation_planted_inside_that_gated_file_is_caught() {
    let raw = gated_source();
    let at = raw.find(MACROS[0]).expect("매크로 호출이 있어야 한다");
    let open = raw[at..]
        .find('(')
        .map(|rel| at + rel + 1)
        .expect("매크로 호출의 여는 구분자가 있어야 한다");
    let mut mutated = String::with_capacity(raw.len() + 32);
    mutated.push_str(&raw[..open]);
    mutated.push_str("\n    return true;\n");
    mutated.push_str(&raw[open..]);

    let before = scan(&mask_non_code(&raw));
    let after = scan(&mask_non_code(&mutated));
    assert!(
        before.violations.is_empty(),
        "원본이 이미 빨갛다 — 이 변이가 무엇을 보였는지 갈리지 않는다: {:?}",
        before.violations
    );
    assert_eq!(
        after.blocks, before.blocks,
        "주입이 블록 수를 바꾸면 안 된다(잡힌 이유가 흐려진다)"
    );
    assert_eq!(
        after.violations.len(),
        1,
        "게이트된 파일 안에 심은 값 반환을 못 잡았다(또는 과검출했다): {:?}",
        after.violations
    );
}

mod exemption_mutations {
    //! 이 가드의 **면제마다** 그것을 겨냥한 변이. 면제 창 안쪽에 진짜 위반을 심었을
    //! 때 잡히는지를 묻는다 — 면제를 넣기만 하고 검증하지 않으면 그 면제만큼 구멍이다.

    use super::*;

    /// A-2(주석·문자열 마스킹)를 겨냥한다. 주석 속 가짜 `return` 바로 옆에 진짜
    /// 위반을 두어, 마스킹이 진짜까지 삼키지 않는지 본다.
    #[test]
    fn catches_a_real_return_next_to_a_commented_one() {
        let src = "define_class!(\n    impl X {\n        fn f(&self) -> bool {\n            /* return false; */ return true;\n        }\n    }\n);\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 1);
        assert_eq!(found.violations, vec![4]);
    }

    /// 같은 면제의 정당한 쪽 — 문자열 안의 `return` 은 코드가 아니다.
    ///
    /// 스니펫에 `let _ =` 를 쓰지 않는다: pre-commit 의 `let _` 검사는 문자열
    /// 리터럴을 덮지 않아 합성 스니펫 **안쪽**을 진짜 코드로 오인한다(이 가드가
    /// 마스킹으로 피하는 바로 그 함정이다).
    #[test]
    fn ignores_a_return_that_only_appears_in_a_string_literal() {
        let src = "define_class!(\n    impl X {\n        fn f(&self) -> usize {\n            let s = \"return true;\";\n            s.len()\n        }\n    }\n);\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 1);
        assert!(found.violations.is_empty());
    }

    /// A-1(값 없는 `return;` 허용)의 경계 — 줄바꿈이 끼어도 값 반환은 값 반환이다.
    #[test]
    fn catches_a_value_return_split_across_lines() {
        let src =
            "define_class!(\n    fn f() -> bool {\n        return\n            true;\n    }\n);\n";
        assert_eq!(scan(&mask_non_code(src)).violations, vec![3]);
    }

    /// A-1 의 정당한 쪽 — 세미콜론 앞에 공백이 끼어도 값이 없으면 통과다.
    #[test]
    fn allows_a_bare_return_with_whitespace_before_the_semicolon() {
        let src = "define_class!(\n    fn f() {\n        return ;\n    }\n);\n";
        assert!(scan(&mask_non_code(src)).violations.is_empty());
    }

    /// 스캔 범위의 경계 — 블록 밖의 값 반환은 이 가드의 대상이 아니다.
    #[test]
    fn ignores_returns_outside_any_macro_block() {
        let src = "fn f() -> bool {\n    return true;\n}\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 0);
        assert!(found.violations.is_empty());
    }

    /// 구분자 면제 — 매크로를 중괄호로 불러도 같은 블록으로 본다.
    #[test]
    fn handles_a_brace_delimited_macro_call() {
        let src = "define_class! {\n    fn f() -> bool {\n        return true;\n    }\n}\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 1);
        assert_eq!(found.violations, vec![3]);
    }
}
