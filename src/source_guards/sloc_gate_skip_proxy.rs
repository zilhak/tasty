//! 파일 크기 게이트의 이름 기반 제외를 선언·Cargo 타깃·생성 표지와 대조한다.
//! 출하 파일을 *_tests.rs 등으로 이름만 바꿔 검사에서 빼는 것을 막는다.
//! 반대로 test 전용 파일이 이름 때문에 검사되고 복잡도 예외 목록에도 있다면 제외 기준을 재검토한다.
//! 실제 SLOC을 다시 계산하지 않고 예외 목록과의 교집합을 확인한다.
//! 제외 패턴과 임계값은 scripts/check-file-size.sh에서 읽는다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{SCAN_ROOTS, repo_root, rust_sources};

const GATE: &str = "scripts/check-file-size.sh";

fn gate_threshold() -> usize {
    let text = std::fs::read_to_string(repo_root().join(GATE))
        .unwrap_or_else(|e| panic!("{GATE} 를 읽을 수 없다 — {e}"));
    text.lines()
        .find_map(|line| line.trim().strip_prefix("THRESHOLD="))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_else(|| panic!("{GATE} 에서 THRESHOLD 를 못 읽었다 — 형식이 바뀌었다"))
}

fn gate_skip_patterns() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join(GATE))
        .unwrap_or_else(|e| panic!("{GATE} 를 읽을 수 없다 — {e}"));
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("skip()") {
            inside = true;
            continue;
        }
        if inside {
            if t == "}" {
                break;
            }
            if let Some(head) = t.split(')').next() {
                for pat in head.split('|') {
                    let pat = pat.trim();
                    // case의 기본 분기 *를 제외 패턴으로 읽으면 모든 파일을 제외하게 된다.
                    if pat == "*" {
                        continue;
                    }
                    if pat.starts_with('*') || pat.contains('/') {
                        out.push(pat.to_string());
                    }
                }
            }
        }
    }
    assert!(
        !out.iter().any(|p| p == "*"),
        "skip의 기본 분기 *를 제외 패턴으로 읽었다. 모든 파일을 제외하지 않도록 파서를 확인한다: {out:?}"
    );
    assert!(
        out.len() >= 4,
        "{GATE}에서 제외 패턴을 {}개만 읽었다. 실제 패턴과 파싱 범위를 확인한다.",
        out.len()
    );
    out
}

/// 스크립트가 쓰는 * glob만 지원한다.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut rest = text;
    if !parts[0].is_empty() {
        if !rest.starts_with(parts[0]) {
            return false;
        }
        rest = &rest[parts[0].len()..];
    }
    let last = parts.len() - 1;
    for (i, part) in parts.iter().enumerate().skip(1) {
        if part.is_empty() {
            continue;
        }
        if i == last && !pattern.ends_with('*') {
            return rest.ends_with(part) && rest.len() >= part.len();
        }
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

fn name_skipped(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| glob_matches(p, path))
}

/// test 전용 파일 분류는 공용 shipping_scope를 사용한다. 반환 경로는 저장소 상대 경로다.
pub(super) fn test_only_files() -> BTreeSet<PathBuf> {
    tasty_doc_guards::shipping_scope::test_only_files(&repo_root(), &rust_sources())
}

fn implies_test(pred: &str) -> bool {
    tasty_doc_guards::cfg_predicate::implies(pred, "test")
}

fn as_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// 파일 이름과 별도로 확인할 제외 근거.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backing {
    /// test 전용 선언 아래의 파일.
    Declaration,
    /// Cargo 통합 시험 타깃으로 분류된 파일.
    CargoTestTarget,
    /// 생성 표지가 있는 파일. 생성기를 실제로 실행했는지는 확인하지 않는다.
    Generated,
    /// 파일 이름 외에는 제외 근거를 찾지 못한 경우.
    NameOnly,
}

/// 처음 20줄의 생성 표지를 확인한다. 표지만으로 실제 생성물임을 증명할 수는 없다.
const GENERATED_MARKER: &str = "DO NOT EDIT";

fn is_cargo_test_target(rel: &str) -> bool {
    tasty_doc_guards::shipping_scope::is_cargo_test_target(&repo_root(), Path::new(rel))
}

/// test_only에도 Cargo 타깃이 들어 있으므로 타깃 분류를 먼저 해 두 근거를 구별한다.
fn backing(c: &Candidate) -> Backing {
    if is_cargo_test_target(&c.rel) {
        Backing::CargoTestTarget
    } else if c.test_only {
        Backing::Declaration
    } else if c.generated {
        Backing::Generated
    } else {
        Backing::NameOnly
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    rel: String,
    lines: usize,
    test_only: bool,
    generated: bool,
}

fn population() -> Vec<Candidate> {
    let test_only = test_only_files();
    rust_sources()
        .into_iter()
        .map(|(path, text)| Candidate {
            test_only: test_only.contains(&path),
            generated: text
                .lines()
                .take(20)
                .any(|line| line.contains(GENERATED_MARKER)),
            rel: as_slash(&path),
            lines: text.lines().count(),
        })
        .collect()
}

fn bypassing(pop: &[Candidate], patterns: &[String]) -> (Vec<String>, usize) {
    let skipped: Vec<&Candidate> = pop
        .iter()
        .filter(|c| name_skipped(&c.rel, patterns))
        .collect();
    let bad = skipped
        .iter()
        .filter(|c| backing(c) == Backing::NameOnly)
        .map(|c| c.rel.clone())
        .collect();
    (bad, skipped.len())
}

fn measured_though_not_shipped(pop: &[Candidate], patterns: &[String]) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = pop
        .iter()
        .filter(|c| c.test_only && !name_skipped(&c.rel, patterns))
        .map(|c| (c.lines, c.rel.clone()))
        .collect();
    out.sort_by_key(|(lines, _)| std::cmp::Reverse(*lines));
    out
}

/// 과거 측정: 2026-09-05 e5128a8c의 design_token_guard는 code SLOC 727, 원시 줄 수 1052였다.
/// 원시 줄 수는 주석 때문에 실제 SLOC보다 크고, 리터럴까지 가린 줄 수는 실제 SLOC보다 작을 수 있다.
/// 따라서 어느 값도 SLOC 임계와 직접 비교하지 않고 게이트의 기존 예외 목록을 확인한다.
/// 아래 값은 현재 파일 크기가 아니라 당시 비교 근거다.
const MEASURED_NOTE: (&str, usize, usize) = ("src/design_token_guard.rs", 727, 1052);

fn allowlist_entries() -> BTreeSet<String> {
    let text = std::fs::read_to_string(repo_root().join(".complexity-file-allowlist"))
        .expect("`.complexity-file-allowlist` 를 못 읽었다 — 경로가 바뀌었는지 확인해라");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn costing(measured: &[(usize, String)], allow: &BTreeSet<String>) -> Vec<String> {
    measured
        .iter()
        .map(|(_, rel)| rel)
        .filter(|rel| allow.contains(*rel))
        .cloned()
        .collect()
}

#[test]
fn every_name_skipped_file_is_really_not_shipped() {
    let patterns = gate_skip_patterns();
    let (bad, skipped) = bypassing(&population(), &patterns);

    assert!(
        skipped > 0,
        "이름으로 제외된 파일을 찾지 못했다. 제외 패턴 {}개의 파싱과 적용을 확인한다.",
        patterns.len()
    );
    assert!(
        bad.is_empty(),
        "파일 이름 외에 SLOC 제외 근거가 없다. test 전용·Cargo 타깃·생성 여부를 확인하고, 출하 코드라면 제외 이름으로 우회하지 말고 복잡도 예외 절차를 따른다:\n  {}",
        bad.join("\n  ")
    );
}

/// test 전용이지만 이름으로 제외되지 않는 파일이 복잡도 예외 목록에 있는지 확인한다. 실제 현재 SLOC 초과를 다시 측정하지는 않는다.
#[test]
fn the_filename_proxy_has_not_started_costing_anything() {
    let patterns = gate_skip_patterns();
    let threshold = gate_threshold();
    let measured = measured_though_not_shipped(&population(), &patterns);
    let allow = allowlist_entries();

    assert!(
        !measured.is_empty(),
        "test 전용이지만 이름으로 제외되지 않는 파일을 찾지 못했다. 선언 분류와 수집을 확인한다."
    );
    assert!(
        !allow.is_empty(),
        "복잡도 예외 목록을 읽지 못해 test 전용 파일과 대조할 수 없다"
    );

    let bad = costing(&measured, &allow);
    let (note, note_code, note_raw) = MEASURED_NOTE;
    assert!(
        bad.is_empty(),
        "이름으로 제외되지 않는 test 전용 파일이 복잡도 예외 목록에 있다(게이트 임계 {threshold}):\n  {}\n파일 이름 관례를 맞추거나 게이트를 선언 기반으로 바꿀지 검토한다. 대상 {}개 · 예외 목록 {}개. 원시 줄 수를 SLOC 대신 쓰지 않는 과거 근거: {note} 원시 {note_raw} / code {note_code}.",
        bad.join("\n  "),
        measured.len(),
        allow.len()
    );
}

#[test]
fn this_guard_is_inside_the_population_it_judges() {
    let me: PathBuf = ["src", "source_guards", "sloc_gate_skip_proxy.rs"]
        .iter()
        .collect();
    let test_only = test_only_files();
    assert!(
        test_only.contains(&me),
        "이 파일이 선언상 test-only 로 안 잡힌다 — 선언 파서가 `#[cfg(test)] mod \
         source_guards;` 를 못 따라온 것이다. 스캔 루트: {SCAN_ROOTS:?}"
    );
    assert!(
        !name_skipped(&as_slash(&me), &gate_skip_patterns()),
        "이 파일이 이름으로 면제된다 — 그러면 (나)의 모수 밖이라 자기 판정을 안 받는다"
    );
}

/// 복합 test 조건·path 속성으로 연결한 시험 파일과 출하 진입점을 함께 대조한다.
#[test]
fn the_declaration_parser_still_sees_the_two_shapes_that_once_fooled_it() {
    let test_only = test_only_files();
    let has = |parts: &[&str]| {
        let p: PathBuf = parts.iter().collect();
        assert!(
            repo_root().join(&p).is_file(),
            "표본이 사라졌다: {} — 옮겨졌으면 이 테스트의 좌표를 고쳐라",
            p.display()
        );
        test_only.contains(&p)
    };

    assert!(
        has(&["src", "state", "popup_close_tests.rs"]),
        "`#[cfg(all(test, feature = \"gui\"))]` 형태를 못 읽는다 — 복합 cfg 안의 test 를 \
         함의로 판정해야 한다"
    );
    assert!(
        has(&["src", "completion_strategy", "registry_tests.rs"]),
        "`#[path = \"...\"]` 로 선언된 모듈을 못 따라간다 — 마스킹된 줄에는 경로가 없으니 \
         같은 줄 번호의 원문에서 꺼내야 한다"
    );
    assert!(
        !has(&["src", "main.rs"]),
        "출하 진입점을 test 전용 파일로 잘못 분류했다"
    );

    assert!(
        test_only.len() > 10,
        "test 전용 파일을 {}개만 찾았다. 모듈 선언의 추적 범위를 확인한다.",
        test_only.len()
    );
}

/// not(test)와 any(unix, test)는 test 전용을 뜻하지 않는다.
#[test]
fn cfg_predicates_that_do_not_imply_test_are_not_treated_as_test_only() {
    assert!(implies_test("test"));
    assert!(implies_test("all(test, feature = \"gui\")"));
    assert!(implies_test("all(unix, test)"));
    assert!(implies_test("all(test, all(unix, debug_assertions))"));

    assert!(!implies_test("not(test)"), "부정을 함의로 읽었다");
    assert!(!implies_test("any(unix, test)"), "선언을 함의로 읽었다");
    assert!(
        !implies_test("all(unix, windows)"),
        "test 가 없는데 참이라 한다"
    );
    assert!(!implies_test("feature = \"gui\""));
}

/// 누락 영향이 큰 실제 파일을 고르도록 줄 수가 가장 큰 후보를 사용한다.
fn largest(pop: &[Candidate], pick: impl Fn(&Candidate) -> bool) -> Candidate {
    let mut hit: Vec<&Candidate> = pop.iter().filter(|c| pick(c)).collect();
    hit.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.rel.cmp(&b.rel)));
    hit.first()
        .map(|c| (*c).clone())
        .expect("합성 변경을 적용할 실제 후보가 없다")
}

#[test]
fn a_shipping_file_renamed_to_a_test_name_is_caught() {
    let patterns = gate_skip_patterns();
    let pop = population();

    let (before, skipped_before) = bypassing(&pop, &patterns);
    assert!(
        before.is_empty(),
        "변이 전에 이미 위반이 있다 — 아래 판정이 변이 때문인지 알 수 없다: {before:?}"
    );

    let victim = largest(&pop, |c| {
        !c.test_only && !name_skipped(&c.rel, &patterns) && !c.generated
    });
    // 너무 작은 파일로만 우회를 검증하지 않도록 합성 대상의 크기도 확인한다.
    assert!(
        victim.lines >= 200,
        "가장 큰 출하 파일이 {}줄({})로 합성 대상의 최소 크기에 못 미친다",
        victim.lines,
        victim.rel
    );
    let renamed = victim.rel.replace(".rs", "_tests.rs");

    assert!(
        name_skipped(&renamed, &patterns),
        "개명이 게이트의 면제 패턴에 걸리지 않는다 — 이 변이는 게이트를 우회하지 못하므로 \
         아래 판정이 무엇을 증명하는지 알 수 없다: {renamed}"
    );
    assert!(
        !is_cargo_test_target(&renamed) && !victim.generated,
        "다른 근거로도 면제되는 파일을 골랐다 — 개명이 원인이라고 말할 수 없다: {renamed}"
    );

    let mut mutated = pop.clone();
    mutated
        .iter_mut()
        .find(|c| c.rel == victim.rel)
        .expect("고른 대상이 모수에 없다")
        .rel = renamed.clone();

    let (after, skipped_after) = bypassing(&mutated, &patterns);
    assert!(
        after.contains(&renamed),
        "출하 파일 {}({}줄)을 {renamed}로 바꾼 우회를 검출하지 못했다: {after:?}",
        victim.rel,
        victim.lines
    );
    assert_eq!(
        skipped_after,
        skipped_before + 1,
        "면제 대상 수가 1 만큼 늘어야 한다 — 그 외의 변화가 있으면 변이가 모수를 흔든 것이다"
    );
}

/// 파일 수·이름을 그대로 두고 test 전용 여부만 바꿔 분류가 달라지는지 확인한다.
#[test]
fn a_name_skipped_file_that_starts_shipping_is_caught_without_changing_any_count() {
    let patterns = gate_skip_patterns();
    let pop = population();
    let (before, skipped_before) = bypassing(&pop, &patterns);
    assert!(before.is_empty(), "변이 전에 이미 위반이 있다: {before:?}");

    let victim = largest(&pop, |c| {
        c.test_only && name_skipped(&c.rel, &patterns) && !is_cargo_test_target(&c.rel)
    });

    assert!(
        victim.lines >= 100,
        "이름으로 제외된 가장 큰 파일이 {}줄({})로 합성 대상의 최소 크기에 못 미친다",
        victim.lines,
        victim.rel
    );

    let mut mutated = pop.clone();
    mutated
        .iter_mut()
        .find(|c| c.rel == victim.rel)
        .expect("고른 대상이 모수에 없다")
        .test_only = false;

    let (after, skipped_after) = bypassing(&mutated, &patterns);
    assert_eq!(
        (mutated.len(), skipped_after),
        (pop.len(), skipped_before),
        "test 전용 여부만 바꾸는 입력이 파일 수나 제외 수까지 바꿨다"
    );
    assert_eq!(
        after,
        vec![victim.rel.clone()],
        "이름을 그대로 둔 {}가 출하 코드로 바뀌었는데 제외 우회를 검출하지 못했다",
        victim.rel
    );
}

#[test]
fn a_measured_file_entering_the_review_list_is_caught_even_when_the_list_size_is_unchanged() {
    let patterns = gate_skip_patterns();
    let measured = measured_though_not_shipped(&population(), &patterns);
    let allow = allowlist_entries();

    assert!(
        costing(&measured, &allow).is_empty(),
        "변이 전에 이미 위반이 있다 — 아래 판정이 변이 때문인지 알 수 없다"
    );

    let (victim_lines, victim) = measured
        .first()
        .cloned()
        .expect("합성 변경을 적용할 test 전용 후보가 없다");
    assert!(
        victim_lines >= 200,
        "가장 큰 후보 {victim}이 {victim_lines}줄로 합성 대상의 최소 크기에 못 미친다"
    );

    let mut grown = allow.clone();
    grown.insert(victim.clone());
    assert_eq!(
        costing(&measured, &grown),
        vec![victim.clone()],
        "test 전용 파일 {victim}을 복잡도 예외에 추가했는데 검출하지 못했다"
    );

    let evicted = allow
        .iter()
        .next()
        .cloned()
        .expect("심사 목록이 비었다 — 크기 보존 변이를 만들 수 없다");
    let mut swapped = allow.clone();
    swapped.remove(&evicted);
    swapped.insert(victim.clone());
    assert_eq!(
        swapped.len(),
        allow.len(),
        "크기 보존 변이가 크기를 바꿨다 — 아래 판정이 크기 변화에 반응한 것일 수 있다"
    );
    assert_eq!(
        costing(&measured, &swapped),
        vec![victim.clone()],
        "예외 목록 크기를 유지하며 {evicted}를 {victim}으로 바꿨는데 test 전용 예외를 검출하지 못했다"
    );

    let outsider = allow
        .iter()
        .find(|a| !measured.iter().any(|(_, rel)| rel == *a))
        .cloned()
        .expect("심사 목록 전부가 모수 안이다 — 음성 대조를 만들 수 없다");
    let mut only_outsider = BTreeSet::new();
    only_outsider.insert(outsider.clone());
    assert!(
        costing(&measured, &only_outsider).is_empty(),
        "모수 밖 파일 `{outsider}` 이 목록에 있는 것을 위반으로 읽었다 — 출하 코드가 임계를 \
         넘어 심사받는 것은 게이트의 정상 동작이다"
    );
}

/// 이름으로 제외된 파일의 근거를 종류별로 확인한다. 파일 수 증감 자체를 결함으로 판단하지 않는다.
#[test]
fn every_exemption_rests_on_something_other_than_the_name() {
    let patterns = gate_skip_patterns();
    let pop = population();

    let mut by_kind: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in pop.iter().filter(|c| name_skipped(&c.rel, &patterns)) {
        by_kind
            .entry(format!("{:?}", backing(c)))
            .or_default()
            .push(c.rel.clone());
    }

    for kind in ["Declaration", "CargoTestTarget", "Generated"] {
        assert!(
            by_kind.get(kind).is_some_and(|v| !v.is_empty()),
            "제외 근거 {kind}에 해당하는 파일을 찾지 못했다. 수집·분류를 확인한다: {:?}",
            by_kind.keys().collect::<Vec<_>>()
        );
    }

    let nameless = by_kind.get("NameOnly").cloned().unwrap_or_default();
    assert!(
        nameless.is_empty(),
        "이름 외의 제외 근거가 없는 파일이다. test 전용 선언·Cargo 시험 타깃·실제 생성물 여부를 확인한다. 생성물은 {GENERATED_MARKER} 표지를 유지하고 셋 다 아니면 복잡도 검사를 받는다:\n  {}",
        nameless.join("\n  ")
    );
}

#[test]
fn a_name_that_merely_looks_exempt_no_longer_buys_an_exemption() {
    let patterns = gate_skip_patterns();
    let pop = population();
    let (before, _) = bypassing(&pop, &patterns);
    assert!(before.is_empty(), "변이 전에 이미 위반이 있다: {before:?}");

    let victim = largest(&pop, |c| {
        !c.test_only && !name_skipped(&c.rel, &patterns) && !c.generated
    });
    assert!(
        victim.lines >= 200,
        "고른 출하 파일이 {}줄({})로 합성 대상의 최소 크기에 못 미친다",
        victim.lines,
        victim.rel
    );

    // src 내부의 tests 디렉터리는 Cargo 통합 타깃이 아니므로 경로 이름만으로 제외해서는 안 된다.
    let in_tests_dir = "src/tests/smuggled.rs".to_string();
    assert!(
        name_skipped(&in_tests_dir, &patterns),
        "게이트가 이 경로를 면제하지 않는다 — 변이가 우회를 만들지 못했다: {in_tests_dir}"
    );
    assert!(
        !is_cargo_test_target(&in_tests_dir),
        "`{in_tests_dir}` 를 cargo 통합 타깃으로 읽었다 — 크레이트 루트 바로 아래 `tests/` 만 \
         타깃이고 `src/` 안의 같은 이름 디렉토리는 모듈이다"
    );

    let named_generated = "src/looks_generated.rs".to_string();
    assert!(
        name_skipped(&named_generated, &patterns),
        "게이트가 이 경로를 면제하지 않는다: {named_generated}"
    );

    for smuggled in [in_tests_dir, named_generated] {
        let mut mutated = pop.clone();
        let slot = mutated
            .iter_mut()
            .find(|c| c.rel == victim.rel)
            .expect("고른 대상이 모수에 없다");
        slot.rel = smuggled.clone();
        assert_eq!(
            mutated.len(),
            pop.len(),
            "경로만 바꾸는 변이가 모수 크기를 바꿨다"
        );

        let (after, _) = bypassing(&mutated, &patterns);
        assert!(
            after.contains(&smuggled),
            "출하 파일을 {smuggled}로 옮긴 이름 기반 제외를 검출하지 못했다: {after:?}"
        );
    }
}
