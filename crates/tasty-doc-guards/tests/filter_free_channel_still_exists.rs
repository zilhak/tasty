//! 이 디렉토리의 가드들이 **매 push 도는 채널**을 실제로 갖고 있는지 본다.
//!
//! ADR-0138 의 결정은 "읽는 것이 전부 `docs/**` 인 가드를 의존 0 크레이트로 옮긴다" 였고,
//! 그 결정이 값을 갖는 근거는 **옮긴 자리에 경로 필터가 없다**는 것 하나다. 그런데 그
//! 근거를 확인하는 것이 아무 데도 없었다 — 실측으로 확인했다(2026-09-05):
//!
//! - `filtered_guards_are_not_totally_blind` 는 이 디렉토리를 이름으로 **건너뛴다**
//!   (`FILTER_FREE_DIR`). 채널이 있다고 **가정**하는 것이지 재는 것이 아니다.
//! - `ci_channel_claims_match_workflows` 의 `automatic_job_bodies` 는 경로 필터를
//!   모델하지 않는다. `push:` 만 있으면 자동으로 세므로, 필터가 생겨도 그 잡은 여전히
//!   "자동" 이다.
//! - `src/source_guards` 의 `EXPECTED_TEST_INVOCATIONS` 는 파일별 **호출 건수**를
//!   고정한다. 필터가 붙어도 건수는 그대로고, 호출을 한 타깃으로 좁혀도 그대로다.
//!
//! 변이 둘로 그 셋을 동시에 확인했다 — ① `push:` 에 `paths:` 를 달기 ② 호출을
//! `--test <이름>` 하나로 좁히기. **두 변이 모두 세 판정기 전부에서 살아남았다.**
//! 그 상태가 되면 이 디렉토리의 가드들은 자기가 깨질 수 있는 유일한 종류의 push
//! (문서만 담은 push)에서 안 도는 자리로 조용히 돌아간다 — ADR-0138 이 벗어나려던
//! 바로 그 상태이고, 되돌아간 것을 아무도 못 본다.
//!
//! **이름이 아니라 성질로 판정한다**(ADR-0175 와 같은 이유). 물음은 "`doc-guards.yml`
//! 이 있는가" 가 아니라 "경로 필터 없는 잡 중 이 패키지를 **좁히지 않고** 돌리는 것이
//! 있는가" 다. 워크플로 이름이 바뀌거나 잡이 다른 파일로 옮겨가도 채널이 남아 있으면
//! 통과해야 한다 — 이름으로 박으면 옮기는 것 자체가 거짓 실패가 된다.
//!
//! **이 가드 자신의 채널**: 여기서 잡는 변경은 `.github/workflows/**` 를 건드린다.
//! 그 경로는 `crossplatform-check.yml` 의 `paths-ignore`(`docs/**` · `site/**` ·
//! `**/*.md`) 밖이라, 필터가 붙는 그 push 에서 `check-headless` 가 전체 스위트를 돌며
//! 이 타깃을 실행한다. 즉 자기 채널이 사라지는 변경은 다른 채널이 본다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tasty_doc_guards::workflow_triggers::filter_free_coverage;

/// 채널이 지켜 주는 대상이 사는 곳. `filtered_guards_are_not_totally_blind` 의
/// `FILTER_FREE_DIR` 과 **같은 값이어야 한다** — 아래에서 그 정합을 함께 본다.
const GUARD_DIR: &str = "crates/tasty-doc-guards/tests";

/// 그 디렉토리의 패키지 이름. 채널 판정은 이 이름을 좁히지 않고 부르는가로 한다.
const PACKAGE: &str = "tasty-doc-guards";

/// 채널이 지켜 주는 순수 스캔 가드 수의 하한. 실측 17 (2026-09-05).
/// 모수가 비면 "채널이 있다" 는 아무것도 안 지키는 참이 된다.
///
/// **판별식** — 단정 앞에서 실측값을 찍게 해 뒀으므로(R445) 그 줄이 계기다:
///
/// ```text
/// cargo test -p tasty-doc-guards --test filter_free_channel_still_exists -- --nocapture
///   → [필터 없는 채널] 순수 스캔 가드 <실측> · 하한 12
/// ```
///
/// ★ 이 수는 **형제 가드의 부분집합 크기**다. `filtered_guards_are_not_totally_blind` 의
/// `MIN_SCANNED` 가 세는 "필터 뒤 스캔 가드" 안에 이 디렉토리의 가드가 들어 있고, 그
/// 부분이 이 수다. 그래서 **한쪽만 보면 판정이 안 선다** — 이 수가 줄었을 때 원인은
/// 둘이다: 가드가 사라졌거나(형제도 같이 준다), 다른 디렉토리로 옮겨졌거나(형제는 그대로).
/// **두 수를 함께 재야 갈린다.**
///
/// 실측 2026-09-07(`de0572359`): 이 수 **27** · 형제 **62** ⇒ 이 디렉토리 밖 35.
/// 09-05 의 17/51 에서 안이 +10, 밖이 +1 움직였다.
///
/// ★★ 여유가 **15** 다(27 대 12). 형제 쪽 주석이 "여유를 6 만 둔다" 는 규율을 적어 두었고
/// 이 자리는 그 두 배 넘게 벌어져 있다 — 벌어진 만큼이 곧 술어가 죽어도 안 보이는 구간이다.
/// 값을 올릴지는 하한 조이기라는 별개 축이라 **여기서는 실측만 남긴다.**
///
/// **이 수를 내려서 초록을 만들지 마라.** 내리면 아래 `uncovered` 판정이 모수가 준 만큼
/// 약해진다 — "채널이 있다" 는 명제가 더 적은 가드에 대해서만 참이 되는데 색은 그대로다.
///
/// 정당한 수선: 이 디렉토리에서 가드를 실제로 지웠으면 이 수도 함께 내려라. 옮긴 것이라면
/// **형제 수가 안 움직였는지 먼저 확인해라** — 안 움직였으면 지운 것이 아니라 옮긴 것이고,
/// 그때는 형제 쪽 명부도 함께 봐야 한다.
const MIN_GUARDED: usize = 12;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 이 디렉토리에 사는 **순수 소스 스캔** 타깃. 술어는
/// `filtered_guards_are_not_totally_blind::is_pure_source_scan` 과 같은 성질이다 —
/// 읽는 **행위**로 세고, 프로세스를 띄우는 것은 뺀다.
fn guarded_targets(root: &Path) -> BTreeSet<String> {
    let dir = root.join(GUARD_DIR);
    let mut out = BTreeSet::new();
    for e in std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .flatten()
    {
        let p = e.path();
        if !p.extension().is_some_and(|x| x == "rs") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&p) else {
            continue;
        };
        let reads = src.contains("CARGO_MANIFEST_DIR")
            || src.contains("repo_root()")
            || src.contains("read_to_string")
            || src.contains("read_dir");
        let spawns = [
            "Command::new",
            "spawn_diag",
            "TASTY_E2E_BIN",
            "CARGO_BIN_EXE",
        ]
        .iter()
        .any(|m| src.contains(m));
        if reads && !spawns {
            out.insert(p.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    out
}

#[test]
fn a_filter_free_job_runs_this_package_whole() {
    let root = repo_root();
    let coverage = filter_free_coverage(&root.join(".github/workflows")).unwrap_or_else(|bad| {
        panic!(
            "`on:` 을 못 읽은 워크플로가 있다 — 판정 불가는 통과가 아니다. 인라인 \
                 표기(`on: [push]`)면 블록 표기로 바꾸거나 판독기를 넓혀라: {bad:?}"
        )
    });

    // 양성 대조: 판독이 통째로 비면 아래 단언은 아무것도 안 본다. **한쪽만 비는 것은
    // 고장이 아니다** — 워크플로가 `--test` 를 안 쓰는 것은 정상 설정이라, 그 자리를
    // 고장으로 말하면 설정 변화에 틀린 진단이 붙는다.
    assert!(
        !coverage.named.is_empty() || !coverage.packages.is_empty() || coverage.whole_workspace,
        "필터 없는 채널을 하나도 못 읽었다 — 판독이 깨졌거나 채널이 전부 사라졌다"
    );

    let guarded = guarded_targets(&root);
    // R445 — 측정값은 단정보다 **앞에** 찍는다. 단정이 죽으면 뒤의 출력은 안 돌고,
    // 그러면 다음 사람이 하한을 검사하려고 술어를 손으로 흉내 내게 된다(R460 위반을 강요).
    println!(
        "[필터 없는 채널] 순수 스캔 가드 {} · 하한 {MIN_GUARDED}",
        guarded.len()
    );
    assert!(
        guarded.len() >= MIN_GUARDED,
        "`{GUARD_DIR}` 의 순수 스캔 가드를 {}개밖에 못 셌다(하한 {MIN_GUARDED}) — \
         모수가 줄면 '채널이 있다' 는 아무것도 안 지킨다",
        guarded.len()
    );

    let uncovered: Vec<&String> = guarded
        .iter()
        .filter(|stem| !coverage.covers(stem, PACKAGE))
        .collect();
    assert!(
        uncovered.is_empty(),
        "`{GUARD_DIR}` 의 아래 가드가 경로 필터 없는 채널에 안 덮인다. 그러면 자기가 \
         깨질 수 있는 유일한 종류의 push(문서만 담은 push)에서 안 도는 자리로 돌아간다 \
         — ADR-0138 이 벗어나려던 그 상태다. 필터를 떼거나, `{PACKAGE}` 를 좁히지 않고 \
         돌리는 잡을 경로 필터 없는 워크플로에 두어라. 본 것: 이름으로 지목된 타깃 \
         {:?} · 좁힘 없이 불린 패키지 {:?}:\n  {}",
        coverage.named,
        coverage.packages,
        uncovered
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

/// 채널이 지키는 자리와, **"여기로 옮겨라" 가 가리키는 자리**가 같은 값인가.
///
/// `filtered_guards_are_not_totally_blind` 는 사각인 가드에게 `FILTER_FREE_DIR` 로
/// 옮기라고 요구한다. 그 요구가 값을 가지려면 **그 자리에 채널이 실제로 있어야** 하고,
/// 그것을 지키는 것이 이 파일이다. 두 값이 갈라지면 옮기라는 자리와 채널이 지켜지는
/// 자리가 달라져, 요구를 따른 가드가 아무 채널도 없는 곳에 착지한다.
#[test]
fn the_move_target_and_the_guarded_channel_are_the_same_directory() {
    let root = repo_root();
    let path = root
        .join(GUARD_DIR)
        .join("filtered_guards_are_not_totally_blind.rs");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e} — 옮기라고 요구하는 판정기가 사라졌으면 이 가드의 전제도 \
             사라진 것이다. 함께 다시 판단해라",
            path.display()
        )
    });
    let decl = format!("const FILTER_FREE_DIR: &str = \"{GUARD_DIR}\";");
    assert!(
        src.contains(&decl),
        "`filtered_guards_are_not_totally_blind` 의 `FILTER_FREE_DIR` 이 `{GUARD_DIR}` 가 \
         아니다. 그 상수는 사각인 가드에게 '여기로 옮겨라' 라고 가리키는 자리이고, 이 \
         가드가 지키는 것이 바로 그 자리의 채널이다 — 두 값이 갈라지면 요구를 따른 가드가 \
         아무 채널도 없는 곳에 착지한다"
    );
}
