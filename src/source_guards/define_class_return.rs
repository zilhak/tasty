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

/// 매크로 블록의 **정체**를 집합으로 고정한다 — `(파일, 그 블록이 선언하는 타입 이름)`.
///
/// ## 왜 하한으로 부족한가
///
/// `MIN_BLOCKS = 1` 은 "어딘가에서 1 개는 봤다" 까지만 말한다. 실물 블록 둘이 **같은
/// 파일에 있어서**, 파서가 둘 중 하나를 놓쳐도 남은 1 개가 하한을 통과한다. 하한이
/// 겨누는 실패 모드(파서가 블록을 놓침)에 대해 하한값 자체가 무력하다.
///
/// ## 왜 개수가 아니라 이름인가
///
/// 파일별 **개수**를 고정하면 블록이 사라지는 것은 잡지만 **하나를 다른 것으로 바꾸는**
/// 것은 못 잡는다(개수가 그대로다). `define_class!` 는 본문에 `struct <이름>;` 을 반드시
/// 갖고(objc2 가 요구한다) 그 이름은 식별자라 마스킹을 견디며 줄이 움직여도 안 바뀐다.
/// 그래서 개수 대신 이름을 고정한다.
///
/// ## 대조군이 무엇이고 왜 독립인가 — **git 이 아니다**
///
/// 스캔 **파일 집합**은 `git ls-files` 라는 런타임 독립 오라클이 있었다(`scan_population`).
/// "매크로 블록" 에는 그런 오라클이 없다 — 블록은 이 파서의 산물이고, 같은 것을 다른
/// 방법으로 세려면 마스킹과 구분자 매칭을 다시 구현해야 하는데 그러면 같은 결함 종류를
/// 물려받는다. 그건 대조가 아니라 복제다.
///
/// 그래서 대조군을 **다른 계측기**가 아니라 **다른 시점**에서 가져온다: 사람이 쓴 이
/// 스냅샷이다. 파서가 나중에 블록을 잃으면 파서의 산출은 바뀌고 이 표는 안 바뀌므로
/// 드리프트가 뜬다.
///
/// **남는 약점을 적어 둔다**: 누가 깨진 파서에 맞춰 이 표를 갱신하면 무력화된다. 하한도
/// 같은 약점이 있지만 하한은 갱신조차 필요 없이 조용하다 — 표는 최소한 diff 에 이름
/// 변경으로 드러나고 사유를 요구한다. 그 차이가 이 승급이 사는 것이다. 빨개지면 **먼저
/// 왜 바뀌었는지 확인하고** 그 다음에 표를 고친다.
const EXPECTED_BLOCKS: &[(&str, &str)] = &[
    ("src/host_api/webview/macos.rs", "NavDelegate"),
    ("src/host_api/webview/macos.rs", "KeyWebView"),
];

/// 블록이 선언하는 타입 이름. 못 찾으면 조용히 빠지지 않고 자리를 남긴다 — 빠지면
/// 개수가 줄어 "블록이 사라졌다" 로 잘못 읽힌다.
fn declared_type(body: &str) -> String {
    word_positions(body, "struct")
        .into_iter()
        .find_map(|at| {
            let rest = body[at + "struct".len()..].trim_start();
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .unwrap_or_else(|| "<이름을 못 읽었다>".to_string())
}

/// 스냅샷과의 차이를 사람이 읽을 줄로 낸다. 순수 함수라 변이가 트리를 안 고치고 찌른다.
fn block_drift(actual: &BTreeSet<(String, String)>) -> Vec<String> {
    let expected: BTreeSet<(String, String)> = EXPECTED_BLOCKS
        .iter()
        .map(|(path, name)| ((*path).to_string(), (*name).to_string()))
        .collect();
    let mut drift: Vec<String> = expected
        .difference(actual)
        .map(|(path, name)| format!("  사라짐: {path} 의 `{name}`"))
        .collect();
    drift.extend(
        actual
            .difference(&expected)
            .map(|(path, name)| format!("  새로 생김: {path} 의 `{name}`")),
    );
    drift
}

/// 레포를 훑어 `(파일, 블록 이름)` 집합을 만든다. 본 판정과 변이가 같은 것을 쓴다.
fn scan_block_population() -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    for (path, text) in rust_sources() {
        let rel = path.to_string_lossy().replace('\\', "/");
        for name in scan(&mask_non_code(&text)).names {
            out.insert((rel.clone(), name));
        }
    }
    out
}

fn assert_population(actual: &BTreeSet<(String, String)>) {
    let drift = block_drift(actual);
    assert!(
        drift.is_empty(),
        "매크로 블록 집합이 스냅샷과 다르다. 개수 하한은 한 파일 안에서 블록이 하나 \
         빠지는 것도, 하나가 다른 것으로 바뀌는 것도 못 잡으므로 이름을 고정한다.\n{}",
        drift.join("\n")
    );
}

const RETURN: &str = "return";

/// 마스킹된 소스 하나에 대한 판정 결과. 줄 번호는 1-based.
struct Scan {
    /// 찾은 매크로 블록 수(스캔 하한용).
    blocks: usize,
    /// 각 블록이 선언하는 타입 이름 — 모수 고정용. 개수와 달리 교체를 가른다.
    names: Vec<String>,
    /// 값 반환 `return` 이 있는 줄.
    violations: Vec<usize>,
    /// 구분자가 닫히지 않은 매크로 시작 줄 — 마스킹이 깨졌다는 신호다.
    unclosed: Vec<usize>,
}

/// 레포 전수 테스트와 합성 입력 테스트가 함께 부르는 판정기.
fn scan(masked: &str) -> Scan {
    let mut out = Scan {
        blocks: 0,
        names: Vec::new(),
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
            out.names.push(declared_type(body));
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
    let mut found_blocks: BTreeSet<(String, String)> = BTreeSet::new();
    for (path, text) in rust_sources() {
        let found = scan(&mask_non_code(&text));
        blocks += found.blocks;
        let rel = path.to_string_lossy().replace('\\', "/");
        for name in found.names {
            found_blocks.insert((rel.clone(), name));
        }
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
    assert_population(&found_blocks);
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

/// 이 승급을 겨냥한 변이. 판정기가 순수 함수라 트리를 안 고치고 찌른다.
mod population_mutations {
    use super::*;

    /// 무변이 대조 + 같은 자리의 비영 대조. 집합이 비면 아래 차분들이 전부 무의미하다.
    #[test]
    fn the_unmutated_block_set_has_no_drift() {
        let actual = scan_block_population();
        assert!(
            block_drift(&actual).is_empty(),
            "무변이인데 차분이 있다 — 아래 변이들의 빨강은 변이 때문이 아니게 된다"
        );
        assert!(
            actual.len() > 1,
            "블록을 {} 개만 찾았다 — 한 개 이하면 '하나를 놓쳐도 통과한다' 를 잴 수 없다",
            actual.len()
        );
    }

    /// **하한이 못 잡는 폭을 같은 테스트에서 단정한다.** 블록 하나를 잃어도 남은 개수가
    /// `MIN_BLOCKS` 이상이라 하한만 있는 가드는 이 변이에 초록이다. 집합 동등은 잃은
    /// 것을 이름으로 말한다 — 그 차이가 이 승급의 전부다.
    #[test]
    fn a_lost_block_is_caught_while_the_floor_stays_green() {
        let actual = scan_block_population();
        let victim = actual.iter().next().expect("대조군이 비었다").clone();
        let mut lost = actual.clone();
        lost.remove(&victim);

        assert!(
            lost.len() >= MIN_BLOCKS,
            "전제가 깨졌다: 블록 하나를 잃으면 {} 개가 남아 하한 {MIN_BLOCKS} 아래로 \
             내려간다. 그렇다면 하한이 이 변이를 잡는다는 뜻이고, 이 테스트가 재려던 \
             '하한의 사각' 이 사라진 것이다 — doc 의 서술을 다시 재고 고쳐라",
            lost.len()
        );

        let drift = block_drift(&lost);
        assert_eq!(drift.len(), 1, "잃은 블록 하나만 말해야 한다: {drift:?}");
        assert!(
            drift[0].contains(&victim.1),
            "잃은 블록을 이름으로 말하지 않는다: {drift:?}"
        );
    }

    /// **개수를 그대로 두는 변이.** 블록 하나를 다른 것으로 바꾸면 파일별 개수는 안
    /// 변한다 — 건수 고정으로는 못 잡고 이름 집합만 잡는다. 이 테스트가 "개수가 아니라
    /// 이름" 이라는 선택의 근거 그 자체다.
    #[test]
    fn a_swapped_block_keeps_the_count_and_is_still_caught() {
        let actual = scan_block_population();
        let victim = actual.iter().next().expect("대조군이 비었다").clone();
        let mut swapped = actual.clone();
        swapped.remove(&victim);
        swapped.insert((victim.0.clone(), format!("{}Replaced", victim.1)));

        assert_eq!(
            swapped.len(),
            actual.len(),
            "변이가 개수를 바꿨다 — 이 테스트의 전제가 깨졌다"
        );
        // 파일별 개수도 그대로다: 같은 파일 안에서 하나를 빼고 하나를 넣었다.
        let count_of = |set: &BTreeSet<(String, String)>, file: &str| {
            set.iter().filter(|(path, _)| path == file).count()
        };
        assert_eq!(
            count_of(&swapped, &victim.0),
            count_of(&actual, &victim.0),
            "파일별 개수까지 같아야 이 변이가 '건수 고정으로는 못 잡는다' 를 증명한다"
        );

        let drift = block_drift(&swapped);
        assert_eq!(
            drift.len(),
            2,
            "사라짐과 새로 생김을 둘 다 말해야 한다: {drift:?}"
        );
    }

    /// 이 가드 파일 자신이 모수에 들어오지 않는가. 여기 `define_class!` 는 전부 문자열
    /// 리터럴이라 마스킹으로 지워진다 — 안 지워지면 판정기가 자기를 세게 되고, 스냅샷이
    /// "실물 블록 + 이 파일이 자기를 언급한 횟수" 라는 자기 참조가 된다.
    #[test]
    fn the_guard_file_does_not_count_itself() {
        let actual = scan_block_population();
        let me = "src/source_guards/define_class_return.rs";
        assert!(
            !actual.iter().any(|(path, _)| path == me),
            "가드 자신이 모수에 들어왔다 — 마스킹이 깨졌다: {actual:?}"
        );
        // 0 을 보고하는 자리라 같은 산출물의 비영 대조를 같은 자리에 둔다.
        assert!(
            !actual.is_empty(),
            "모수가 비었다 — 위 단정은 언제나 통과한다"
        );
    }
}
