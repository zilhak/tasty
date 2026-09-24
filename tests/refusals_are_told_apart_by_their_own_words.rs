//! 스크립트의 die 호출마다 다른 오류와 구별되는 문구를 검사하는 시험 리터럴이 있는지 확인한다.
//! 같은 종료 코드만 확인하면 다른 실패가 대신 그 코드를 내도 통과할 수 있다.
//! 문구를 참조하는 시험의 존재만 찾으며 실제로 해당 분기에 도달하거나 그 출력을 단정하는지까지 검증하지 않는다.
//! 미대응 수는 CAP와 같아야 한다. 수가 줄었으면 수집 오류인지 실제 시험 추가인지 확인한 뒤 CAP도 갱신한다.
//! 셸을 실행하지 않는 소스 검사여서 운영체제로 제외하지 않는다.

use std::fs;
use std::path::{Path, PathBuf};

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 빈 수집을 CAP 감소로 오인하지 않도록 별도 하한에서 먼저 실패시킨다.
const SCRIPT_FLOOR: Floor = Floor {
    min: 16,
    measured: 27,
    measured_on: "2026-09-20",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree(
        "e3a747ea4 — scripts 아래 .sh를 재귀로 세면 27개다. gates_pin_their_judge_absence와 같은 측정 선언을 추가한 살아 있는 커밋이다.",
    ),
    why_this_gap: "측정 27개와 하한 16 사이 여유는 작은 스크립트 추가·삭제를 허용하고 순회 생략이나 하위 디렉터리만 읽는 오류를 찾는다. 일부 누락까지 모두 보장하지는 않는다.",
};

const ROOT_TESTS_FLOOR: Floor = Floor {
    min: 21,
    measured: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.counted_on,
    why_this_gap: "과거 fdca139c0..91ca7d37d 구간에서 루트 통합 타깃 감소 3회와 최대 폭 16개를 관측했다. 현재 수는 공용 측정을 따른다. 하한은 수집 누락을 먼저 진단하기 위한 것이며 실제 대규모 통합·삭제가 생기면 근거를 다시 확인한다.",
};

const CRATE_TESTS_FLOOR: Floor = Floor {
    min: 80,
    measured: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.counted_on,
    why_this_gap: "과거 관측 구간에서는 크레이트 통합 타깃의 감소가 없어 한 크레이트의 타깃 수를 기준으로 여유를 정했다. 하한 80은 당시 7타깃의 세 배인 21의 여유로 정한 값이다. 이후 공용 측정값이 증가하면서 여유도 커졌으며 현재 배수를 새로 선택한 것은 아니다.",
};

/// 게이트가 아닌 사용법 안내 스크립트는 사유와 함께 제외한다.
const NOT_A_GATE: &[(&str, &str)] = &[(
    "survey-guard-move.sh",
    "CI나 훅에서 호출하지 않는 수동 조사 도구다. 이 스크립트의 거절은 게이트 판정이 아니라 인자 사용법 안내이므로 오류별 시험 리터럴 검사의 대상에서 제외한다.",
)];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 2026-09-14 미대응19곳을 기록했다. 증가와 감소를 모두 확인하는 고정값이다.
const CAP: usize = 19;

/// 짧은 일반 토큰을 오류별 증거로 오인하지 않도록 최소 문자 수를 요구한다.
const MIN_EVIDENCE: usize = 8;

/// 자기 진단 문자열이 증거로 수집되지 않도록 이 파일은 제외한다.
const SELF: &str = "refusals_are_told_apart_by_their_own_words.rs";

struct Refusal {
    script: String,
    line: usize,
    message: String,
}

fn walk(dir: &Path, floor: &Floor, keep: &dyn Fn(&Walked) -> bool) -> Vec<Walked> {
    walk_with_floor(dir, dir, floor, Descend::Everything, keep)
        .unwrap_or_else(|why| panic!("{why}"))
}

/// 줄 주석과 die() 정의를 빼고 한 줄의 die 큰따옴표 문자열을 읽는다. 완전한 셸 구문 분석은 아니다.
fn refusals() -> Vec<Refusal> {
    let dir = root().join("scripts");
    let mut out = Vec::new();
    for w in walk(&dir, &SCRIPT_FLOOR, &|w: &Walked| w.rel.ends_with(".sh")) {
        if NOT_A_GATE.iter().any(|(name, _)| w.rel.ends_with(name)) {
            continue;
        }
        let body = fs::read_to_string(&w.path).unwrap_or_default();
        for (i, line) in body.lines().enumerate() {
            if line.trim_start().starts_with('#') || line.contains("die()") {
                continue;
            }
            let Some(rest) = line.split_once("die \"") else {
                continue;
            };
            if rest
                .0
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue;
            }
            let Some((msg, _)) = rest.1.split_once('"') else {
                continue;
            };
            out.push(Refusal {
                script: w.rel.clone(),
                line: i + 1,
                message: msg.to_string(),
            });
        }
    }
    out
}

/// 한 줄의 큰따옴표 문자열을 읽고 이스케이프를 건너뛴다. 모든 Rust 리터럴 형식을 처리하지는 않는다.
fn literals_in(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut buf = String::new();
        let mut closed = false;
        while let Some(d) = chars.next() {
            if d == '\\' {
                if let Some(e) = chars.next() {
                    buf.push(e);
                }
                continue;
            }
            if d == '"' {
                closed = true;
                break;
            }
            buf.push(d);
        }
        if closed {
            out.push(buf);
        }
    }
    out
}

/// 다른 게이트의 같은 문구를 증거로 쓰지 않도록 해당 스크립트 이름을 포함하는 시험 파일에서 찾는다.
fn test_sources() -> Vec<(String, Vec<String>)> {
    let r = root();
    let root_tests = r.join("tests");
    let crate_tests = r.join("crates");

    let mut files = walk(&root_tests, &ROOT_TESTS_FLOOR, &|w: &Walked| {
        w.rel.ends_with(".rs") && !w.rel.contains('/')
    });
    files.extend(walk(&crate_tests, &CRATE_TESTS_FLOOR, &|w: &Walked| {
        let parts: Vec<&str> = w.rel.split('/').collect();
        parts.len() == 3 && parts[1] == "tests" && w.rel.ends_with(".rs")
    }));

    let mut out = Vec::new();
    for w in files {
        if w.rel.ends_with(SELF) {
            continue;
        }
        let body = fs::read_to_string(&w.path).unwrap_or_default();
        let mut lits: Vec<String> = body
            .lines()
            .flat_map(literals_in)
            .filter(|l| l.chars().count() >= MIN_EVIDENCE)
            .collect();
        lits.sort();
        lits.dedup();
        out.push((body, lits));
    }
    out
}

fn pool_for(script: &str, sources: &[(String, Vec<String>)]) -> Vec<String> {
    let base = script.rsplit('/').next().unwrap_or(script);
    let mut out: Vec<String> = sources
        .iter()
        .filter(|(body, _)| body.contains(base))
        .flat_map(|(_, lits)| lits.iter().cloned())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// 증거는 해당 오류 메시지에 있고, 다른 오류 메시지와 메시지를 제거한 스크립트 본문에는 없어야 한다.
fn evidence<'a>(r: &Refusal, all: &[Refusal], lits: &'a [String], rest: &str) -> Option<&'a str> {
    lits.iter()
        .filter(|l| r.message.contains(l.as_str()))
        .filter(|l| {
            all.iter()
                .filter(|o| o.message.contains(l.as_str()))
                .count()
                == 1
        })
        .filter(|l| !rest.contains(l.as_str()))
        .max_by_key(|l| l.chars().count())
        .map(|l| l.as_str())
}

fn body_without_messages(script: &str, all: &[Refusal]) -> String {
    let mut body = fs::read_to_string(root().join("scripts").join(script)).unwrap_or_default();
    for r in all.iter().filter(|r| r.script == script) {
        body = body.replace(&r.message, "");
    }
    body
}

#[test]
fn the_left_side_is_counted_not_listed() {
    let all = refusals();
    assert!(
        !all.is_empty(),
        "scripts에서 die 호출을 찾지 못했다. 수집 경로와 호출 형식을 확인한다."
    );
    let lits: Vec<String> = test_sources().into_iter().flat_map(|(_, l)| l).collect();
    assert!(
        lits.len() > 500,
        "시험 리터럴을 {} 개만 수집했다. 파일 범위와 리터럴 판독 형식을 확인한다.",
        lits.len()
    );
}

#[test]
fn refusals_without_words_of_their_own_stay_capped() {
    let all = refusals();
    let sources = test_sources();

    let mut scripts: Vec<String> = all.iter().map(|r| r.script.clone()).collect();
    scripts.sort();
    scripts.dedup();
    let rests: Vec<(String, String)> = scripts
        .iter()
        .map(|s| (s.clone(), body_without_messages(s, &all)))
        .collect();

    let mut naked = Vec::new();
    let mut spoken = 0usize;
    for r in &all {
        let rest = rests
            .iter()
            .find(|(s, _)| *s == r.script)
            .map(|(_, b)| b.as_str())
            .unwrap_or("");
        let pool = pool_for(&r.script, &sources);
        if evidence(r, &all, &pool, rest).is_some() {
            spoken += 1;
        } else {
            naked.push(format!("{}:{}  {}", r.script, r.line, r.message));
        }
    }

    assert!(
        spoken > 0,
        "구별되는 시험 리터럴이 있는 오류를 찾지 못했다. 수집한 오류 {} 곳과 증거 판독을 확인한다.",
        all.len()
    );
    assert!(
        naked.len() <= CAP,
        "구별되는 시험 리터럴이 없는 오류가 CAP {CAP}을 넘었다({} 곳). 해당 오류의 실제 출력을 구별해 검사하는 시험을 추가한다. 수집·판독 오류도 함께 확인하고 상한만 올려 통과시키지 않는다:\n{}",
        naked.len(),
        naked.join("\n")
    );
    assert!(
        naked.len() >= CAP,
        "미대응 오류 수가 CAP {CAP}보다 적다({}). 수집 누락이 아니라 실제 검사 보강인지 확인한 뒤 CAP를 함께 낮춘다.\n    const CAP: usize = {};",
        naked.len(),
        naked.len()
    );
}

#[test]
fn the_non_gate_list_names_real_scripts_with_reasons() {
    let dir = root().join("scripts");
    let scripts = walk(&dir, &SCRIPT_FLOOR, &|w: &Walked| w.rel.ends_with(".sh"));
    for (name, why) in NOT_A_GATE {
        assert!(
            scripts.iter().any(|w| w.rel.ends_with(name)),
            "NOT_A_GATE의 {name}이 scripts에 없다. 이동 여부를 확인해 항목을 갱신하거나 제거한다."
        );
        assert!(
            why.chars().count() >= 40,
            "'{name}' 의 사유가 너무 짧다 — 왜 그 거절이 판정이 아닌지를 적어야 한다"
        );
    }
}
