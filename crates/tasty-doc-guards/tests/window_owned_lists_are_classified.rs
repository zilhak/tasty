//! 창 소유 목록 자원의 **합산 소속 명부**. `src/app/dispatch/list_global.rs` 의
//! 합산 집합과 이 명부가 양방향으로 맞물린다.
//!
//! ## 왜 이 판정기인가 — 자동 발견은 네 형태로 다 새었다
//!
//! "창 소유 컬렉션을 순회 · 대상 인자 없음 · 합산 집합에 없음"([ADR-0175](../../../docs/adr/0175-window-owned-list-membership-is-judged-by-shape-not-by-name.md))
//! 을 **정적으로 자동 발견**하려는 시도는 이 저장소에서 네 번 다 실패했고, 넷 다
//! 실측이다.
//!
//! 1. **이름 모양**(`*.list` / `*_list`)으로 뽑으면 `tree` 가 빠진다 — 그리고 실제로
//!    새고 있던 것이 그 `tree` 였다.
//! 2. **"params 를 안 받는다"** 로 대상 인자 유무를 근사하면 `hook.list` 가 빠진다 —
//!    그 params 는 대상이 아니라 **필터**(`surface_id`)다. 거짓 음성이라 "고칠 것이
//!    없다" 로 보인다.
//! 3. **`engine.<필드>` 줄 단위 grep** 은 rustfmt 가 줄을 접은 `engine\n  .hook_manager`
//!    를 못 본다. 그 형태로 네 필드가 통째로, `workspaces` 는 일곱 자리가 안 보였다.
//! 4. **`CoreState` 의 컬렉션 필드 전수**로 올리면 명부가 ~50 이 되는데 대부분이
//!    `pending_*` 버퍼·per-surface 캐시라 **자원이 아니다.** 크기가 뜻을 죽인다.
//!
//! 그래서 자동 발견을 포기하고 **손으로 유지하는 명부**로 간다. 이 명부가 못 하는
//! 것을 먼저 적는다: **새 list 메서드가 들어와도 이 가드는 모른다.** 할 수 있는 것은
//! 두 가지다 — 합산 집합에서 무엇이 **빠지는** 것을 잡고, 지금 안 합산되는 자원이
//! **왜** 그런지를 검사받는 텍스트로 만든다.
//!
//! ## 이 가드가 도는 자리가 값이다
//!
//! 같은 축의 실행 단언은 `tests/e2e_tests.rs` 의 `multi_window_owner_routing` 에
//! 있는데, 그것은 창을 요구해 헤드리스 조합 CI 가 이름으로 `--skip` 하는 유일한
//! 테스트다. 즉 **자동으로 도는 채널이 없다.** 이 가드는 `tasty-doc-guards` 라
//! 경로 필터 없는 잡에서 push 마다 돈다. 둘은 겹치는 것이 아니라 **채널이 다르다.**

use std::path::{Path, PathBuf};

#[derive(PartialEq, Debug)]
enum Class {
    /// `dispatch_list_global` 이 전 engine 을 합쳐 답한다.
    Aggregated,
    /// 호출자가 대상 id 를 실어 라우터가 주인 창을 푼다. 합산이 필요 없다.
    TargetedByCallerId,
    /// 저장소가 engine 을 건너 공유된다. 어느 창으로 가도 같은 답이라 합산이 항등이다.
    SharedAcrossEngines,
    /// **창별인데 합산되지 않는다 — 열린 결함.** 사유 칸에 무엇이 막고 있는지 적는다.
    PerEngineNotAggregated,
}
use Class::*;

/// (메서드, 갈래, 사유). 사유는 모든 갈래에서 필수다 — 갈래 이름만으로는 다음 사람이
/// 판정을 재현하지 못한다.
const ROSTER: &[(&str, Class, &str)] = &[
    (
        "workspace.list",
        Aggregated,
        "워크스페이스는 창 소유. id 가 IdGenerator 공유라 이어 붙이면 키가 된다",
    ),
    ("surface.list", Aggregated, "상동 — surface id 공유"),
    ("pane.list", Aggregated, "상동 — pane id 공유"),
    (
        "pty.list",
        Aggregated,
        "headless pty. id 공유가 빠져 있던 동안 두 창의 pty 가 같은 id 를 받아 먼저 만든 쪽이 닿지 않았다",
    ),
    ("output.observe_list", Aggregated, "observer id 공유"),
    (
        "workspace_category.list",
        Aggregated,
        "category id 공유. 예약 `normal`(id 0)만 한 줄로 접는다",
    ),
    (
        "tree",
        Aggregated,
        "이름이 `*.list` 가 아니라 이름 기반 census 에서 빠져 있었다 — ADR-0175 가 그 자리다",
    ),
    (
        "tab.list",
        TargetedByCallerId,
        "`pane_id` 필수. 실측으로 비포커스 창의 pane 을 지목해 답이 온다",
    ),
    (
        "surface.meta.list",
        TargetedByCallerId,
        "`surface_id` 필수. 실측 동일",
    ),
    (
        "memory.list",
        SharedAcrossEngines,
        "memory store 는 `new_with_ids` 인자로 전 engine 이 같은 Arc 를 든다",
    ),
    (
        "hook.list",
        PerEngineNotAggregated,
        "`hook_manager` 가 engine 마다 새로 만들어지고 hook id 가 IdGenerator 에 없어 창 간 충돌한다 — 합산 전에 id 공간을 먼저 고쳐야 한다",
    ),
    (
        "global_hook.list",
        PerEngineNotAggregated,
        "`global_hook_manager` 도 engine 마다 새로 만들어진다. 이름과 달리 창에 매여 있고 id 도 충돌한다",
    ),
    (
        "notification.list",
        PerEngineNotAggregated,
        "`notifications` 가 engine 마다 새로 만들어진다. 지목 수단이 없어 다른 창의 알림은 보이지도 닿지도 않는다",
    ),
    (
        "approval.list",
        PerEngineNotAggregated,
        "`approval_store` 가 engine 마다 `Arc::new` 된다 — 공유 Arc 가 아니다. 휴먼 핸드오프가 창별로 갈린다",
    ),
    (
        "attach.list",
        PerEngineNotAggregated,
        "engine 별 `OccupancyRegistry`. CLI 표면이 없어(`tasty tool attach` 는 다른 메서드) 실행 재현은 안 했다",
    ),
];

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/tasty-doc-guards → 레포 루트.
    p.pop();
    p.pop();
    p
}

/// `dispatch_list_global` 의 match arm 에서 메서드 이름을 뽑는다.
fn aggregated_arms(root: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(root.join("src/app/dispatch/list_global.rs"))
        .expect("list_global.rs 를 읽지 못했다 — 경로가 바뀌었으면 이 가드도 함께 옮긴다");
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim_start();
        // 주석 줄의 예시 이름을 arm 으로 세지 않는다.
        if t.starts_with("//") {
            continue;
        }
        let Some(rest) = t.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        if !rest[end..]
            .trim_start_matches('"')
            .trim_start()
            .starts_with("=>")
        {
            continue;
        }
        out.push(rest[..end].to_string());
    }
    out
}

#[test]
fn every_aggregated_entry_is_actually_in_the_aggregator() {
    let arms = aggregated_arms(&repo_root());
    let missing: Vec<_> = ROSTER
        .iter()
        .filter(|(m, c, _)| *c == Aggregated && !arms.iter().any(|a| a == m))
        .map(|(m, _, _)| *m)
        .collect();
    assert!(
        missing.is_empty(),
        "명부가 합산이라고 적은 메서드가 `dispatch_list_global` 에 없다: {missing:?}\n\
         합산에서 빠지면 그 목록은 **포커스된 창의 것만 답하고 에러가 없다.**"
    );
}

#[test]
fn the_aggregator_has_nothing_the_roster_does_not_know() {
    let arms = aggregated_arms(&repo_root());
    let unknown: Vec<_> = arms
        .iter()
        .filter(|a| !ROSTER.iter().any(|(m, _, _)| m == *a))
        .collect();
    assert!(
        unknown.is_empty(),
        "합산 집합에 명부가 모르는 메서드가 있다: {unknown:?}\n\
         명부에 갈래와 사유를 적어라 — 합산은 id 가 창을 건너 유일할 때만 옳다."
    );
}

#[test]
fn every_entry_carries_a_reason() {
    let empty: Vec<_> = ROSTER
        .iter()
        .filter(|(_, _, why)| why.trim().is_empty())
        .map(|(m, _, _)| *m)
        .collect();
    assert!(empty.is_empty(), "사유가 빈 명부 항목: {empty:?}");
}

#[test]
fn the_open_ones_are_not_silently_emptied() {
    // 열린 결함이 사라졌다면 그것은 고쳐졌다는 뜻이고, 그때 갈래를 옮기는 것이
    // 이 명부를 갱신하는 방법이다. 수를 박아 두는 것은 그 갱신을 **강제**하기 위한
    // 것이지 그 수가 옳다는 뜻이 아니다.
    let open = ROSTER
        .iter()
        .filter(|(_, c, _)| *c == PerEngineNotAggregated)
        .count();
    assert_eq!(
        open, 5,
        "창별인데 합산 안 되는 항목의 수가 바뀌었다. 고쳤으면 갈래를 옮기고 이 수를 \
         함께 내려라 — 남겨 두면 다음 사람이 이미 닫힌 것을 다시 센다."
    );
}
