//! 게이트 출력에 대한 **부정 단언**(`!out.contains("…")`)이 같은 리터럴의 **긍정
//! 단언**과 짝지어 있는가를 묻는다.
//!
//! 왜 필요한가: 게이트의 출력 문구를 술어로 쓰는 자리는 그 문구가 바뀌는 순간 죽는데,
//! **극성에 따라 죽는 소리가 다르다.**
//!   · 긍정 단언 `assert!(out.contains(X))` — X 가 사라지면 그 자리에서 빨개진다. 시끄럽다.
//!   · 부정 단언 `assert!(!out.contains(X))` — X 가 사라지면 **영원히 초록**이다. 조용하다.
//!     그 단언이 지키려던 성질("이 상태에서는 그 절을 내지 않는다")은 그때부터 아무도
//!     안 본다. 안 걸리는 술어는 없는 술어보다 나쁘다 — 채널은 도는데 초록이 뜬다.
//!
//! 실측 2026-09-08 이 이 물음을 만들었다. 형제 도구 `scripts/gate-delta.sh` 가 게이트
//! 출력을 문자열로 판정하고 있었는데, 여섯 게이트를 전수로 돌려 세니 표지 하나는
//! 생산자가 **0/6**(완전히 죽음)이고 다른 하나는 **3/5** 였다. 그쪽은 rc 로 옮겨 술어
//! 자체를 없앴다. 남은 것이 테스트 안의 부정 단언이고, 그것은 rc 로 못 옮긴다 —
//! 출력의 **부재**를 묻는 단언이라 종료 코드에 안 나타난다.
//!
//! 짝지음이 답인 이유: 같은 리터럴의 긍정 단언이 같은 파일에 있으면, 그 문구가 바뀌는
//! 순간 **긍정 쪽이 먼저 빨개진다.** 부정 쪽의 조용함이 긍정 쪽의 시끄러움에 업힌다.
//! 새 채널을 만드는 것이 아니라, 이미 시끄러운 자리에 조용한 자리를 묶는 것이다.
//!
//! 좌변이 **`scripts/` 를 실행하는 파일**인 이유: 게이트 출력을 술어로 쓸 수 있는 것은
//! 게이트를 돌리는 파일뿐이다. 레포 전체의 부정 단언을 세면 게이트와 무관한 것들이
//! 대부분이고, 그것들에 "긍정 짝을 만들어라" 는 실재하지 않는 결함에 대한 처방이 된다.
//!
//! **리터럴이 `scripts/` 소스에 있는가로 거르지 않는다.** 그 필터는 좁히는 방향이라
//! 놓치는 쪽이 조용하다 — 게이트가 보간으로 만드는 문구(`경고(900↑) $N`)와 픽스처가
//! 심어 되돌아 나오는 경로는 소스에 리터럴로 없다. 실측: 그 필터로 후보 30 중 14 가
//! "죽었다" 로 나왔고 14 전부 오탐이었다(그 단언들은 지금 초록이다).
//!
//! `#![cfg(unix)]` 인 이유는 형제들과 같다 — 좌변 판정이 `bash` 를 부르는 자리를 세므로
//! 그 셸이 없는 플랫폼에서는 게이트가 아니라 셸의 부재가 결과를 정한다.

//!
//! **Windows 에서 이 타깃은 빈다 — 커버리지 0 이고, 그 0 은 이제 미측정이 아니라 잰
//! 값이다.** `#![cfg(unix)]` 라 그렇고, 그 cfg 가 그 타깃에서 실제로 거짓인 것을 재서
//! 안다: `rustc --print cfg --target x86_64-pc-windows-gnu` 에 `unix` 선언이 **없다**
//! (호스트에는 있다). 컴파일은 거기서도 통과한다 — `cargo clippy -p tasty --all-targets
//! --locked --target x86_64-pc-windows-gnu` rc=0, 이 파일 진단 0 (실측 2026-09-08).
//! 즉 **Windows 잡의 초록은 이 게이트가 거기서 돈다는 뜻이 아니다.** 한때 이 자리에
//! "Windows 잡은 `--lib --bins` 라 안 본다" 고 적혀 있었는데, 그 잡은 `--all-targets`
//! 로 이 타깃을 **컴파일한다** — 안 보는 것은 잡이 아니라 `cfg` 다.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 루트 `tests/` 한 겹의 순회 하한.
///
/// 이 가드의 물음은 "부정 단언이 짝지어 있는가" 라서, 순회가 죽어 좌변을 하나도 못
/// 모으면 **위반 0 으로 초록**이 된다 — 지키려던 것이 깨진 그 순간에도.
const ROOT_TESTS_FLOOR: Floor = Floor {
    min: 21,
    // 좌변의 사실은 `tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS` 하나가 갖는다 — 같은 모수를 재는
    // 자리가 셋인데 값이 36 과 39 로 갈려 있었다.
    measured: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::ROOT_TEST_TARGETS.counted_on,
    why_this_gap: "루트 통합 테스트 타깃의 감소를 실측했다(`8bdbf1bdb` 직전 \
                   1215 커밋): 10 회, 최대 -6. 여유 18 은 그 세 배다. 앞선 판에서 여유의 크기를 \
                   숫자로 안 적기로 했던 것을 여기서 뒤집는다 — 그때 걱정한 낡음(옛 문장이 '여유 \
                   15' 였고 그때 이미 16 이었다)은 그 숫자를 실측에서 뺀 차로 적었기 때문에 \
                   생겼다. 진폭에서 뽑은 수는 실측이 움직여도 안 낡고 대신 관측 창과 함께 낡는다 \
                   — 창을 적어 두면 언제 다시 재야 하는지가 남는다. 이 가드는 부정 단언의 짝을 \
                   보므로 순회가 죽으면 위반 0 으로 초록이 된다",
};

/// `crates/` 아래 통합 테스트 타깃의 순회 하한.
const CRATE_TESTS_FLOOR: Floor = Floor {
    min: 80,
    // 좌변의 사실은 `tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS` 하나가 갖는다 — 93 과 101 로
    // 갈려 있었고 둘 다 이 트리의 수가 아니었다.
    measured: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::CRATE_TEST_TARGETS.counted_on,
    why_this_gap: "크레이트 52 개 중 13 이 통합 테스트를 갖는다. 같은 창(1215 \
                   커밋)에서 이 모수의 감소는 0 회다 — 진폭이 없어 여유를 거기서 못 뽑는다. \
                   사건의 크기에 건다: 이 가드가 사는 크레이트를 뺀 최대 크레이트가 7 타깃이고 \
                   여유 21 은 그 세 배다. 이 자리가 루트보다 여유가 큰 것은 '크레이트가 통째로 \
                   접히면 한꺼번에 빠져서' 가 아니라 그 사건이 아직 관측되지 않아 곱수를 진폭이 \
                   아니라 사건 크기에 걸었기 때문이다",
};

/// 짝이 없어도 되는 자리. **이름과 사유를 함께** 둔다 — 사유 없는 항목은 아래가 막는다.
///
/// 예외를 스크립트 주석이나 테스트 주석에 적지 않는 이유: 거기 적으면 늘리는 비용이 한
/// 줄이고 리뷰에 안 뜬다. 여기 두면 늘릴 때 이 파일이 바뀌고, 그 수가 세어진다.
const EXCEPTIONS: &[(&str, &str, &str)] = &[
    // 회차 92 통합에서 처음 보였다. 둘 다 **게이트 출력이 아니라 소스 텍스트**를
    // 훑는 프로브다 — 판정하려는 술어가 그 자리에 살아 있는지를 원문에서 읽는다.
    // 짝이 될 "그 문구가 실제로 나오는 상태" 라는 것이 없다: 문구가 사라지는 것이
    // 곧 이 프로브가 잡으려는 사건이라, 긍정 짝을 두면 그 사건을 못 잡는다.
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
    // 회차 95 통합에서 보였고, **앞의 둘과도 모양이 다르다.** 앞의 둘은 소스 원문을
    // 훑는 프로브였다. 이것은 단언이 아니라 순회의 **경로 필터**다 —
    // `if !rel_str.ends_with(".rs") || !rel_str.contains("tests/") { continue; }` 의
    // 그 자리이고, 리터럴은 게이트 출력이 아니라 **레포 상대경로의 한 조각**이다.
    // 짝이 될 "그 문구가 실제로 나오는 상태" 라는 것이 원리적으로 없다: 긍정 짝을
    // 두면 경로에 `tests/` 가 든 파일이 있다는 사실을 단언하게 되는데, 그것은 이
    // 필터가 지키려는 성질이 아니다.
    (
        "tasty-doc-guards/tests/ci_channel_claims_match_workflows.rs",
        "tests/",
        "게이트 출력이 아니라 순회의 경로 필터 — 리터럴이 레포 상대경로의 조각이고 짝이 될 상태가 없다",
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 이 파일이 다른 스크립트를 **실행**하는가.
fn spawns_a_script(body: &str) -> bool {
    body.contains("Command::new(\"bash\")")
        || body.contains("Command::new(\"sh\")")
        || body.contains("bash scripts/")
        || body.contains("bash \"scripts/")
}

/// 좌변: 통합 테스트 타깃 **한 겹** 중 스크립트를 실행하는 것.
fn executors() -> Vec<Walked> {
    let r = root();
    let root_tests = r.join("tests");
    let crate_tests = r.join("crates");

    let one_layer = |w: &Walked| w.rel.ends_with(".rs") && w.rel.matches('/').count() == 0;
    let crate_layer = |w: &Walked| {
        // `<크레이트>/tests/<파일>.rs` 한 겹만 — 더 깊은 것은 타깃이 아니라 그 타깃의 모듈이다.
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

/// 한 줄에서 `.contains("…")` 의 리터럴을 뽑고, 그 앞에 `!` 가 붙었는지 함께 낸다.
///
/// 이스케이프가 든 리터럴은 **건너뛴다** — 여기서 러스트 문자열 문법을 다시 구현하면
/// 같은 물음에 답이 둘이 된다. 지금 좌변에 그런 리터럴은 없다(있으면 못 보는 것이고,
/// 그 사실을 아래 테스트가 수로 남긴다).
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
    // 좌변이 깨지면 이 테스트는 아무것도 안 물은 채 초록이 된다.
    assert!(
        !files.is_empty(),
        "스크립트를 실행하는 통합 테스트 타깃을 하나도 못 셌다 — 좌변이 깨졌다"
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
        "게이트 출력에 대한 **부정 단언**에 같은 리터럴의 긍정 짝이 없다. 그 문구가 바뀌면 \
         이 단언은 빨개지지 않고 **영원히 초록**이 된다 — 지키려던 성질을 그때부터 아무도 \
         안 본다.\n\
         고치는 법: 그 문구가 **실제로 나오는 상태**를 재현하는 시험을 같은 파일에 두고, \
         거기서 같은 리터럴을 긍정으로 단언해라. 그러면 문구가 바뀌는 순간 긍정 쪽이 \
         먼저 빨개진다.\n\
         그 리터럴이 게이트 출력이 아니라면 짝을 만들지 말고 이 파일의 EXCEPTIONS 에 \
         사유와 함께 적어라 — 테스트 주석에 적지 마라:\n  {}",
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
