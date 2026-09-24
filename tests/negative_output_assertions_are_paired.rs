//! 스크립트를 실행하는 테스트 파일에서 contains의 부정 확인에 같은 문자열의 긍정 확인이 있는지 찾는다.
//! 부정 확인만 있으면 출력 문구가 사라져도 통과할 수 있다. 같은 파일에 실제 출력도 요구하는 시험을 둔다.
//! 스크립트 원문의 문자열로 범위를 좁히면 보간된 메시지와 합성 경로를 놓칠 수 있어 그렇게 제한하지 않는다.
//! 호출 형식과 문자열을 읽는 검사이며 단정의 실제 의미까지 분석하지는 않는다.

//! Unix 전용 타깃이므로 Windows에서는 이 시험이 실행되지 않는다.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

const ROOT_TESTS_FLOOR: Floor = Floor {
    min: 21,
    measured: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.counted_on,
    why_this_gap: "과거 fdca139c0..91ca7d37d의 558커밋에서 루트 통합 타깃 감소를 3회 관측했고 최대 폭은 16개였다. 하한은 이 규모의 감소 하나를 허용하도록 두었다. 수집 수는 공용 측정을 사용하며 더 큰 감소가 생기면 실제 삭제와 수집 누락을 구별해 다시 측정한다.",
};

const CRATE_TESTS_FLOOR: Floor = Floor {
    min: 80,
    measured: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.counted_on,
    why_this_gap: "크레이트 통합 타깃은 과거 관측 구간에서 감소 사례가 없어 당시 한 크레이트의 타깃 수를 기준으로 하한 80을 정했다. 이후 공용 측정값이 늘면서 여유도 커졌으며 현재 여유를 새로 선정한 것은 아니다. 크레이트 통합·삭제가 생기면 목록과 하한 근거를 다시 검토한다.",
};

const EXCEPTIONS: &[(&str, &str, &str)] = &[
    // 아래 둘은 출력 검사가 아니라 소스의 구현 표지를 확인한다.
    (
        "tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs",
        "swap(true",
        "게이트 출력이 아니라 소스 프로브 — 이 문구의 부재가 곧 잡으려는 사건이다",
    ),
    (
        "tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs",
        "into_inner()",
        "게이트 출력이 아니라 소스 프로브 — 이 문구의 부재가 곧 잡으려는 사건이다",
    ),
    // 아래 항목은 출력 검사가 아니라 수집 경로 필터다.
    (
        "tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs",
        "tests/",
        "게이트 출력이 아니라 순회의 경로 필터 — 리터럴이 레포 상대경로의 조각이고 짝이 될 상태가 없다",
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn spawns_a_script(body: &str) -> bool {
    body.contains("Command::new(\"bash\")")
        || body.contains("Command::new(\"sh\")")
        || body.contains("bash scripts/")
        || body.contains("bash \"scripts/")
}

fn executors() -> Vec<Walked> {
    let r = root();
    let root_tests = r.join("tests");
    let crate_tests = r.join("crates");

    let one_layer = |w: &Walked| w.rel.ends_with(".rs") && w.rel.matches('/').count() == 0;
    let crate_layer = |w: &Walked| {
        let parts: Vec<&str> = w.rel.split('/').collect();
        parts.len() == 3 && parts[1] == "tests" && w.rel.ends_with(".rs")
    };

    let mut all = walk_with_floor(
        &root_tests,
        &root_tests,
        &ROOT_TESTS_FLOOR,
        Descend::Everything,
        &one_layer,
    )
    .unwrap_or_else(|why| panic!("{why}"));
    all.extend(
        walk_with_floor(
            &crate_tests,
            &crate_tests,
            &CRATE_TESTS_FLOOR,
            Descend::SkipBuildCaches,
            &crate_layer,
        )
        .unwrap_or_else(|why| panic!("{why}")),
    );

    all.retain(|w| {
        fs::read_to_string(&w.path)
            .map(|b| spawns_a_script(&b))
            .unwrap_or(false)
    });
    all.sort_by(|a, b| a.rel.cmp(&b.rel));
    all
}

/// 한 줄의 contains 문자열과 앞의 !를 읽는다. 이스케이프가 있는 리터럴은 제외한다.
fn contains_literals(line: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find(".contains(\"") {
        let before = &rest[..at];
        let negated = before.trim_end().ends_with('!')
            || before
                .rfind('!')
                .map(|i| {
                    before[i + 1..]
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_')
                })
                .unwrap_or(false);
        let body = &rest[at + ".contains(\"".len()..];
        match body.find('"') {
            Some(end) if !body[..end].contains('\\') => {
                out.push((negated, body[..end].to_string()));
                rest = &body[end..];
            }
            _ => break,
        }
    }
    out
}

#[test]
fn every_negative_output_assertion_has_a_positive_twin() {
    let files = executors();
    assert!(
        !files.is_empty(),
        "스크립트 실행 형식이 있는 통합 타깃을 찾지 못했다. 수집 범위와 판독 형식을 확인한다."
    );

    let mut bad = Vec::new();
    for w in &files {
        let body = fs::read_to_string(&w.path).unwrap_or_default();
        let mut positives: Vec<String> = Vec::new();
        let mut negatives: Vec<(usize, String)> = Vec::new();
        for (i, line) in body.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            for (neg, lit) in contains_literals(line) {
                if neg {
                    negatives.push((i + 1, lit));
                } else {
                    positives.push(lit);
                }
            }
        }
        for (line_no, lit) in negatives {
            if positives.contains(&lit) {
                continue;
            }
            if EXCEPTIONS.iter().any(|(f, l, _)| *f == w.rel && *l == lit) {
                continue;
            }
            bad.push(format!("{}:{line_no}  {lit:?}", w.rel));
        }
    }

    assert!(
        bad.is_empty(),
        "contains의 부정 확인에 같은 문자열을 요구하는 긍정 확인이 없다. 출력이 사라져도 통과하지 않도록 같은 파일에 실제 출력 시험을 둔다. 출력이 아닌 소스·경로 검사라면 EXCEPTIONS에 사유를 등록한다:\n  {}",
        bad.join("\n  ")
    );
}

#[test]
fn every_exception_carries_a_reason() {
    for (f, lit, reason) in EXCEPTIONS {
        assert!(
            reason.chars().count() >= 40,
            "{f} 의 {lit:?} 예외 사유가 너무 짧다 — 왜 짝이 없는 것이 옳은지를 적어야 한다"
        );
    }
}
