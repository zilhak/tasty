//! 자동으로 도는 잡의 **이름 명부**를 못 박는다 — 수가 아니라 이름이다.
//!
//! ## 왜 하한이 아닌가
//!
//! 이 자리에 한때 잡 **수**의 하한이 있었다(`guard_test_channels_stay_split::JOB_FLOOR`,
//! `min = 8` · 실측 20). 그 하한은 두 방향으로 틀렸다.
//!
//! - **여유가 값을 흡수한다.** 8 과 실측 사이의 여유 안에서 잡이 열하나 사라져도 통과한다.
//!   그 여유가 곧 안 보는 구간인데, 하한을 올리면 그 하한의 목적("순회가 죽었는가")이
//!   바뀐다 — 다른 물음을 같은 도구로 물으려는 것이라 둘 다 못 한다.
//! - **수는 자리바꿈을 못 본다.** 잡 하나가 자동에서 수동 전용으로 넘어가고 다른 하나가
//!   반대로 넘어오면 수가 그대로다. 그 회차에 사라진 커버리지는 **잡 결론에도, 잡 수에도**
//!   안 나온다.
//!
//! 그래서 여기서는 **이름 집합**을 못 박는다. 자리바꿈은 두 집합이 동시에 어긋나 잡히고,
//! 소실은 한쪽만 어긋나 잡힌다. 둘이 다른 실패문을 낸다.
//!
//! ## 이 파일이 순회에 하한을 안 거는 이유
//!
//! 순회 하한은 **0 과 1 만** 가른다(`min: 1`). 명부가 이미 파일 이름까지 담고 있어서,
//! 파일이 하나라도 빠지면 그 파일의 잡 전부가 명부 차집합으로 나온다 — 하한이 낼 수 있는
//! 어떤 수보다 구체적이다. 같은 판단을 `c03389f07` 이 `JOB_FLOOR` 를 없애며 먼저 했다.
//! (`no_todo_file_citation` 의 하한 분류표도 `.github/workflows` 를 "평평한 순회 —
//! 갈래 확인이 곧 파일 명부가 된다" 로 갈라 놓았다. 이 파일이 그 명부다.)

use std::collections::BTreeSet;
use std::path::PathBuf;

use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};
use tasty_doc_guards::workflow_triggers::{automatic_job_names, job_headers};

/// 자동 회차에 도는 잡. 실측 2026-09-08, 트리 `12bc0f4b2`.
///
/// **이 명부가 세는 것**: `automatic_job_names` 가 내는 이름. 즉 `jobs:` 아래 2 칸 헤더
/// 중 본문에 `github.event_name == 'workflow_dispatch'` 가 **없는** 것. 트리거나 경로
/// 필터는 안 본다 — 그것은 `push_trigger` 의 물음이고, 이 명부는 "파일이 켜졌을 때 그
/// 안에서 무엇이 도는가" 만 답한다.
const AUTOMATIC: &[(&str, &str)] = &[
    ("build-check.yml", "build-linux-arm64"),
    ("build-check.yml", "build-linux-x64"),
    ("build-check.yml", "build-macos"),
    ("build-check.yml", "build-windows"),
    ("complexity-check.yml", "check-file-size"),
    ("crossplatform-check.yml", "check-headless"),
    ("crossplatform-check.yml", "check-macos"),
    ("crossplatform-check.yml", "check-release"),
    ("crossplatform-check.yml", "check-windows"),
    ("doc-guards.yml", "doc-guards"),
    ("format-check.yml", "fmt"),
    ("pages.yml", "build"),
    ("pages.yml", "deploy"),
    ("plugin-version-check.yml", "version-bump"),
    ("release.yml", "create-release"),
    ("release.yml", "publish-release"),
    ("script-gates.yml", "script-gates"),
    ("supply-chain-check.yml", "cargo-deny"),
    ("test.yml", "semver-guards"),
];

/// 수동 전용이라 자동 명부에서 빠진 잡. 같은 트리·같은 날.
///
/// **짝으로 못 박는 이유**: 한쪽만 못 박으면 잡이 사라진 것과 갈래를 옮긴 것이 같은
/// 모양이 된다. 둘을 함께 보면 그 둘이 다른 실패문으로 갈린다.
///
/// `release.yml` 의 넷은 `automatic_job_bodies` 의 술어가 **줄이는 방향으로** 틀려서
/// 여기 있다(그 잡의 `if:` 는 분리라 태그 push 에서도 돈다). 그 한계는 그 함수의 doc 이
/// 적고 있고, 오늘 그 누락의 효과는 0 이다 — `release.yml` 에 `cargo test` 가 없다.
const MANUAL_ONLY: &[(&str, &str)] = &[
    ("release.yml", "build-linux-arm64"),
    ("release.yml", "build-linux-x64"),
    ("release.yml", "build-macos"),
    ("release.yml", "build-windows"),
    ("test.yml", "test-linux-x64"),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("레포 루트")
        .to_path_buf()
}

/// 순회는 살아 있는지만 본다 — 명부가 나머지를 지킨다.
const LIVENESS: Floor = Floor {
    min: 1,
    measured: 11,
    measured_on: "2026-09-08",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree(
        "12bc0f4b2 + 회차 95 통합 27 커밋 — 통합이 다시 셌다(11). \
         820 이 잰 값과 같고, 이 회차는 워크플로 파일을 더하거나 지우지 않았다.",
    ),
    why_this_gap: "이 순회의 소실은 하한이 아니라 아래 두 명부가 잡는다. 파일이 빠지면 \
                   그 파일의 잡 전부가 차집합으로 나오고, 실패문이 그 이름을 찍는다. \
                   하한은 '아무것도 못 읽었다' 하나만 가른다.",
};

/// 지금 트리가 내는 두 집합.
fn measured() -> (BTreeSet<(String, String)>, BTreeSet<(String, String)>) {
    let dir = repo_root().join(".github/workflows");
    let walked = walk_with_floor(&dir, &dir, &LIVENESS, Descend::Everything, &|w| {
        w.rel.ends_with(".yml")
    })
    .unwrap_or_else(|why| panic!("{why}"));

    let (mut auto, mut manual) = (BTreeSet::new(), BTreeSet::new());
    for w in walked {
        // 읽기 실패를 넘기지 않는다. 넘기면 그 파일의 잡이 "없다" 로 세어지고, 아래
        // 대조는 미측정을 소실로 판정한다 — 둘은 처방이 다르다.
        let text = std::fs::read_to_string(&w.path)
            .unwrap_or_else(|e| panic!("워크플로 {} 를 못 읽었다: {e}", w.rel));
        let a: BTreeSet<String> = automatic_job_names(&text).into_iter().collect();
        for name in job_headers(&text) {
            let cell = (w.rel.clone(), name.clone());
            if a.contains(&name) {
                auto.insert(cell);
            } else {
                manual.insert(cell);
            }
        }
    }
    (auto, manual)
}

fn pinned(rows: &[(&str, &str)]) -> BTreeSet<(String, String)> {
    rows.iter()
        .map(|(f, j)| ((*f).to_string(), (*j).to_string()))
        .collect()
}

fn show(set: &BTreeSet<(String, String)>) -> String {
    if set.is_empty() {
        return "  (없음)".to_string();
    }
    set.iter()
        .map(|(f, j)| format!("  {f} / {j}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_roster_of_jobs_that_run_on_every_push_is_pinned() {
    let (auto, manual) = measured();
    let want_auto = pinned(AUTOMATIC);
    let want_manual = pinned(MANUAL_ONLY);

    // 자리바꿈을 먼저 본다 — 수만 보면 안 보이는 갈래라 이 판정이 이 파일의 존재 이유다.
    let crossed_out: Vec<_> = want_auto.intersection(&manual).cloned().collect();
    let crossed_in: Vec<_> = want_manual.intersection(&auto).cloned().collect();
    assert!(
        crossed_out.is_empty() && crossed_in.is_empty(),
        "잡이 자동과 수동 전용 사이를 **넘어갔다**. 수는 그대로일 수 있어 하한도 개수 \
         단정도 이것을 못 본다.\n\
         [자동 → 수동 전용] 이 잡이 배선한 조합은 이제 자동 회차에 **미측정**이다. \
         잡 결론에도 안 나온다 — 그 잡은 `skipped` 로도 안 찍히고 그냥 안 돈다:\n{}\n\
         [수동 전용 → 자동] 새 자동 채널이다. 아래 명부에 옮겨 적고, 그 잡이 무엇을 \
         배선했는지 커밋문에 적어라:\n{}",
        show(&crossed_out.iter().cloned().collect()),
        show(&crossed_in.iter().cloned().collect()),
    );

    let gone: BTreeSet<_> = want_auto.difference(&auto).cloned().collect();
    let added: BTreeSet<_> = auto.difference(&want_auto).cloned().collect();
    assert!(
        gone.is_empty() && added.is_empty(),
        "자동 잡 명부가 달라졌다(못박은 {} · 지금 {}).\n\
         [사라졌으면] 그 잡이 배선한 커버리지는 실패가 아니라 **미측정**이다. 그 조합을 \
         다른 잡이 배선하는지 확인하고, 없으면 그 사실을 `docs/dev-guide/ci-gates.md` 에 \
         적은 뒤 이 명부에서 지워라 — 지우는 것만으로는 아무것도 안 닫힌다:\n{}\n\
         [늘었으면] 새 잡이다. 이 명부에 더하고, 그 잡이 무엇을 배선하는지와 그 조합을 \
         이미 배선한 자리가 있는지를 커밋문에 적어라:\n{}",
        want_auto.len(),
        auto.len(),
        show(&gone),
        show(&added),
    );

    let m_gone: BTreeSet<_> = want_manual.difference(&manual).cloned().collect();
    let m_added: BTreeSet<_> = manual.difference(&want_manual).cloned().collect();
    assert!(
        m_gone.is_empty() && m_added.is_empty(),
        "수동 전용 명부가 달라졌다(못박은 {} · 지금 {}).\n\
         이 명부가 짝이라 함께 못 박는다 — 한쪽만 보면 잡의 소실과 갈래 이동이 같은 \
         모양이 된다.\n[사라졌으면]\n{}\n[늘었으면]\n{}",
        want_manual.len(),
        manual.len(),
        show(&m_gone),
        show(&m_added),
    );
}

/// 명부가 **비어 있지 않게** 지킨다 — 빈 집합끼리는 언제나 같다.
///
/// 위 시험은 두 집합의 대조라, 판독이 통째로 죽어 양쪽이 다 비면 `want` 쪽과 어긋나 죽는다.
/// 그러나 명부 상수가 언젠가 비워지면(예 "임시로 껐다") 그 대조는 조용히 참이 된다.
#[test]
fn the_pinned_roster_is_not_empty() {
    assert!(
        !AUTOMATIC.is_empty() && !MANUAL_ONLY.is_empty(),
        "명부가 비었다 — 빈 집합끼리의 대조는 무엇을 못 봐도 초록이다 \
         (자동 {} · 수동 전용 {})",
        AUTOMATIC.len(),
        MANUAL_ONLY.len(),
    );
}
