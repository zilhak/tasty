//! objc2의 define_class!/declare_class! 안에서는 값이 있는 return을 금지한다.
//! 매크로가 만든 extern shim에서 return하면 bool → Bool 등의 반환 변환을 건너뛸 수 있으므로
//! if/else나 match의 결과 표현식을 쓴다. macOS 전용 코드를 다른 플랫폼에서도 소스로 검사한다.
//!
//! 값 없는 return;과 주석·리터럴은 제외한다. 변환이 필요 없는 반환 타입도 텍스트로 구별하지 못해
//! 일괄 검사하며, 중첩 함수나 클로저의 return도 별도로 구별하지 않는다.

use super::*;

/// 빈 수집을 찾는 하한이다. 실제 매크로가 모두 사라졌다면 검사의 필요성도 다시 검토한다.
const MIN_BLOCKS: usize = 1;

const MACROS: &[&str] = &["define_class!", "declare_class!"];

/// 같은 파일의 블록 하나를 놓치거나 다른 블록으로 바꾸면 개수 하한을 통과할 수 있어 이름도 고정한다.
/// 이 명부는 수동 기준이다. 파서 오류에 맞춰 갱신하지 말고 실제 선언의 변경을 먼저 확인한다.
const EXPECTED_BLOCKS: &[(&str, &str)] = &[
    ("src/host_api/webview/macos.rs", "NavDelegate"),
    ("src/host_api/webview/macos.rs", "KeyWebView"),
];

/// 이름을 읽지 못한 블록도 남겨 선언 누락과 구별한다.
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
        "매크로 블록의 (파일, 타입 이름)이 명부와 다르다. 실제 선언이 바뀌었는지, 파서가 누락했는지 확인한다.\n{}",
        drift.join("\n")
    );
}

const RETURN: &str = "return";

/// 줄 번호는 1부터 시작한다.
struct Scan {
    blocks: usize,
    names: Vec<String>,
    violations: Vec<usize>,
    unclosed: Vec<usize>,
}

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
        "define_class!/declare_class!의 값 있는 return은 매크로가 만든 shim의 반환 변환을 건너뛸 수 있다. if/else 또는 match의 결과 표현식으로 반환한다.\n  {}",
        violations.join("\n  ")
    );
}

/// 상위 모듈의 macOS·gui 조건 때문에 Linux 빌드에서는 확인할 수 없는 검사 대상이다.
const GATED_FILE: &str = "src/host_api/webview/macos.rs";

fn gated_source() -> String {
    rust_sources()
        .into_iter()
        .find(|(path, _)| path.to_string_lossy().replace('\\', "/") == GATED_FILE)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("스캔 결과에 {GATED_FILE} 이 없다"))
}

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
        "변경 전 소스에도 위반이 있어 주입 결과와 비교할 수 없다: {:?}",
        before.violations
    );
    assert_eq!(
        after.blocks, before.blocks,
        "return 주입이 매크로 블록 수까지 바꿨다"
    );
    assert_eq!(
        after.violations.len(),
        1,
        "게이트된 파일 안에 심은 값 반환을 못 잡았다(또는 과검출했다): {:?}",
        after.violations
    );
}

mod exemption_mutations {

    use super::*;

    #[test]
    fn catches_a_real_return_next_to_a_commented_one() {
        let src = "define_class!(\n    impl X {\n        fn f(&self) -> bool {\n            /* return false; */ return true;\n        }\n    }\n);\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 1);
        assert_eq!(found.violations, vec![4]);
    }

    /// 합성 입력의 let _도 pre-commit 검사에 걸릴 수 있어 이름 있는 변수를 쓴다.
    #[test]
    fn ignores_a_return_that_only_appears_in_a_string_literal() {
        let src = "define_class!(\n    impl X {\n        fn f(&self) -> usize {\n            let s = \"return true;\";\n            s.len()\n        }\n    }\n);\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 1);
        assert!(found.violations.is_empty());
    }

    #[test]
    fn catches_a_value_return_split_across_lines() {
        let src =
            "define_class!(\n    fn f() -> bool {\n        return\n            true;\n    }\n);\n";
        assert_eq!(scan(&mask_non_code(src)).violations, vec![3]);
    }

    #[test]
    fn allows_a_bare_return_with_whitespace_before_the_semicolon() {
        let src = "define_class!(\n    fn f() {\n        return ;\n    }\n);\n";
        assert!(scan(&mask_non_code(src)).violations.is_empty());
    }

    #[test]
    fn ignores_returns_outside_any_macro_block() {
        let src = "fn f() -> bool {\n    return true;\n}\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 0);
        assert!(found.violations.is_empty());
    }

    #[test]
    fn handles_a_brace_delimited_macro_call() {
        let src = "define_class! {\n    fn f() -> bool {\n        return true;\n    }\n}\n";
        let found = scan(&mask_non_code(src));
        assert_eq!(found.blocks, 1);
        assert_eq!(found.violations, vec![3]);
    }
}

mod population_mutations {
    use super::*;

    #[test]
    fn the_unmutated_block_set_has_no_drift() {
        let actual = scan_block_population();
        assert!(
            block_drift(&actual).is_empty(),
            "변경 전부터 명부와 달라 합성 변경의 결과를 비교할 수 없다"
        );
        assert!(
            actual.len() > 1,
            "블록을 {} 개만 찾았다 — 한 개 이하면 '하나를 놓쳐도 통과한다' 를 잴 수 없다",
            actual.len()
        );
    }

    #[test]
    fn a_lost_block_is_caught_while_the_floor_stays_green() {
        let actual = scan_block_population();
        let victim = actual.iter().next().expect("대조군이 비었다").clone();
        let mut lost = actual.clone();
        lost.remove(&victim);

        assert!(
            lost.len() >= MIN_BLOCKS,
            "블록 하나를 빼면 {}개로 하한 {MIN_BLOCKS} 미만이 된다. 이 입력으로는 하한이 놓치는 누락을 검증할 수 없다.",
            lost.len()
        );

        let drift = block_drift(&lost);
        assert_eq!(drift.len(), 1, "잃은 블록 하나만 말해야 한다: {drift:?}");
        assert!(
            drift[0].contains(&victim.1),
            "잃은 블록을 이름으로 말하지 않는다: {drift:?}"
        );
    }

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

    #[test]
    fn the_guard_file_does_not_count_itself() {
        let actual = scan_block_population();
        let me = "src/source_guards/define_class_return.rs";
        assert!(
            !actual.iter().any(|(path, _)| path == me),
            "가드 자신이 모수에 들어왔다 — 마스킹이 깨졌다: {actual:?}"
        );
        assert!(
            !actual.is_empty(),
            "블록 목록이 비어 있어 검사 파일의 제외 여부를 확인할 수 없다"
        );
    }
}
