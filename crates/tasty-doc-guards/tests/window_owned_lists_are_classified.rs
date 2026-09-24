//! 창별 목록을 합산할지, 특정 창에 라우팅할지, 공유 저장소를 읽을지 분류한다.
//! 분류의 타당성은 사람이 판단하며 자동으로 소유권을 추론하지 않는다.
//! 합산 메서드 목록은 dispatch_list_global과 양방향으로 대조하고, handler 표의 .list 메서드가
//! 분류에서 빠졌는지도 확인한다. tree처럼 .list로 끝나지 않는 이름은 수동 등록해야 한다.
//!
//! 메서드 이름이나 params 유무만으로 창 소유권을 알 수 없다. 예를 들어 hook.list의
//! surface_id는 필터이며, 전 창의 hook을 합쳐야 한다. 조회 합산과 소유 창 라우팅은
//! 같은 자원에 모두 필요할 수 있어 routing_key_method_scope의 목록과 구별한다.
//! 실제 다중 창 동작은 별도 E2E에서 검사하며 이 가드는 소스의 분류만 확인한다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::match_arms::{Source, matching_close};

#[derive(PartialEq, Debug)]
enum Class {
    /// `dispatch_list_global` 이 전 engine 을 합쳐 답한다.
    Aggregated,
    /// 호출자가 대상 id 를 실어 라우터가 주인 창을 푼다. 합산이 필요 없다.
    TargetedByCallerId,
    /// 저장소를 모든 engine이 공유하므로 창마다 합칠 필요가 없다.
    SharedAcrossEngines,
    /// 창별 자원인데 합산되지 않는 결함. 사유에 해결을 막는 조건을 적는다.
    PerEngineNotAggregated,
}
use Class::*;

/// 메서드·분류·사유. 모든 분류에 판단 근거를 기록한다.
const ROSTER: &[(&str, Class, &str)] = &[
    (
        "workspace.list",
        Aggregated,
        "창별 workspace를 합친다. 공유 IdGenerator가 창 사이 ID 충돌을 막는다.",
    ),
    (
        "surface.list",
        Aggregated,
        "창별 surface를 합친다. surface ID는 창 사이에 유일하다.",
    ),
    (
        "pane.list",
        Aggregated,
        "창별 pane을 합친다. pane ID는 창 사이에 유일하다.",
    ),
    (
        "pty.list",
        Aggregated,
        "창별 headless PTY를 합친다. PTY ID는 공유 IdGenerator에서 발급한다.",
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
        "workspace 트리를 전 창에서 모아야 한다. .list 접미사가 없어 별도로 등록한다(ADR-0017).",
    ),
    (
        "tab.list",
        TargetedByCallerId,
        "필수 pane_id로 대상 페인의 소유 창을 찾는다.",
    ),
    (
        "surface.meta.list",
        TargetedByCallerId,
        "필수 surface_id로 대상 surface의 소유 창을 찾는다.",
    ),
    (
        "memory.list",
        SharedAcrossEngines,
        "memory store 는 `new_with_ids` 인자로 전 engine 이 같은 Arc 를 든다",
    ),
    (
        "hook.list",
        Aggregated,
        "hook ID는 창 사이에 유일하다. surface_id는 대상 창 지정이 아닌 필터이므로 모든 창의 hook을 합친다.",
    ),
    (
        "global_hook.list",
        Aggregated,
        "global_hook_manager는 CoreState마다 있어 합산이 필요하다. ID는 공유하며 개별 항목의 소유 창은 Kind::GlobalHook으로 찾는다.",
    ),
    (
        "notification.list",
        Aggregated,
        "각 engine의 알림을 합치며 IdGenerator 공유 카운터가 ID 충돌을 막는다. 생성 ID 역순으로 전체 50개를 반환하고 UI 패널은 창별로 유지한다",
    ),
    (
        "approval.list",
        SharedAcrossEngines,
        "추가 main window를 만드는 ensure_engine_and_plugins가 첫 engine의 approval_store Arc를 공유한다. 생성자만 보면 놓칠 수 있어 창 생성 경로도 확인해야 한다.",
    ),
    (
        "attach.list",
        Aggregated,
        "OccupancyRegistry는 engine별이지만 surface/workspace ID가 공유돼 합칠 수 있다. 응답의 두 배열은 서로 다른 시점의 결과가 되지 않도록 한 순회에서 수집한다.",
    ),
    (
        "image.list",
        Aggregated,
        "engine.workspaces의 image surface를 모으며 surface ID는 창 사이에 유일하다. 플러그인 namespace로 전달된 외부 요청도 플러그인이 호스트로 되돌리면 이 합산 경로를 지난다.",
    ),
    (
        "completion_strategy.list",
        SharedAcrossEngines,
        "`completion_strategy::global()` — 프로세스 전역 레지스트리라 어느 창으로 가도 같다",
    ),
    (
        "hook_handler.list",
        SharedAcrossEngines,
        "`hook_handler::global()` — 상동. hook **핸들러**는 전역이고 hook **인스턴스**만 창 소유다",
    ),
    (
        "webhook.list",
        SharedAcrossEngines,
        "`webhook::list()` — 프로세스 전역",
    ),
    (
        "session.list",
        SharedAcrossEngines,
        "`core.session_list()` — `core` 는 전 engine 이 같은 것을 본다",
    ),
    (
        "preset.list",
        SharedAcrossEngines,
        "`core.preset_store` (공유 Mutex). 핸들러가 `state` 를 받지만 `_state` 로 안 쓴다",
    ),
    (
        "memory.secret.list",
        SharedAcrossEngines,
        "`core.with_memory` — `memory.list` 와 같은 저장소",
    ),
    (
        "telemetry.cap.list",
        SharedAcrossEngines,
        "`core` 만 읽는다(`_state` · `_engine` 미사용)",
    ),
    ("telemetry.anomaly.list", SharedAcrossEngines, "상동"),
    (
        "remote.profile.list",
        SharedAcrossEngines,
        "`RemoteProfiles::load` — 파일에서 읽으므로 engine 과 무관",
    ),
    (
        "remote.passkey.list",
        SharedAcrossEngines,
        "`Passkeys::load` — 상동",
    ),
];

/// debug API는 사용자 조작 재현용이므로 에이전트의 포커스 독립성 요구에서 제외한다.
const OUT_OF_SCOPE: &[(&str, &str)] = &[
    (
        "debug.tool.list",
        "debug 전용. `state.tool_registry` 라 창별인 것은 맞다",
    ),
    (
        "debug.banner.list",
        "debug 전용. 정의 목록 자체는 `all_defs()` 로 전역이다",
    ),
    ("debug.host_popup.list", "debug 전용. 상동"),
];

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

/// 공용 match 파서로 합산 메서드를 읽는다. cfg별 함수 정의가 여러 개면 모두 검사한다.
/// 문자열 메서드명이나 기본 분기로 해석하지 못한 항목은 누락시키지 않고 실패시킨다.
fn aggregated_arms(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("src/app/dispatch/list_global.rs"))
        .expect("list_global.rs 를 읽지 못했다 — 경로가 바뀌었으면 이 가드도 함께 옮긴다");
    let src = Source::new(&text);
    let bodies = src.fn_bodies("dispatch_list_global");
    assert!(
        !bodies.is_empty(),
        "`fn dispatch_list_global` 을 못 찾았다 — 이름이 바뀌었으면 이 가드도 함께 옮긴다"
    );
    let mut out = Vec::new();
    for body in bodies {
        const HEAD: &str = "match request.method.as_str()";
        let block = src
            .code_slice(&body)
            .find(HEAD)
            .and_then(|k| {
                let from = body.start + k + HEAD.len();
                src.code[from..].find('{').map(|o| from + o)
            })
            .and_then(|open| matching_close(&src.code, open).map(|close| open..close + 1))
            .unwrap_or_else(|| panic!("`{HEAD} {{ … }}` 를 못 찾았다 — 합산기 모양이 바뀌었다"));
        let arms = src
            .match_arms(block)
            .unwrap_or_else(|e| panic!("합산기 팔을 못 읽었다 — {e}"));
        for arm in arms {
            for alt in src.alternatives(&arm.pattern) {
                match src.plain_string(&alt) {
                    Some(name) => out.push(name.to_string()),
                    None if src.slice(&alt) == "_" => {}
                    None => panic!(
                        "합산기 {}행의 팔 조각 `{}` 은 따옴표 이름이 아니라 누가 합산되는지 \
                         판정할 수 없다",
                        src.line_of(alt.start),
                        src.slice(&alt)
                    ),
                }
            }
        }
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
        "합산 대상으로 등록됐지만 dispatch_list_global에 없는 메서드다: {missing:?}. 전 창의 목록이 빠짐없이 반환되는지 확인한다."
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
        "합산 함수에 미등록 메서드가 있다: {unknown:?}. 창 사이 ID 유일성과 소유권을 확인해 분류·사유를 기록한다."
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

/// 같은 메서드가 분류 목록과 제외 목록에 중복되지 않아야 한다.
/// 전체 목록의 완전성은 합산 함수·handler 표와의 별도 대조에서 확인한다.
#[test]
fn no_method_is_listed_twice() {
    let mut seen: std::collections::BTreeMap<&str, Vec<String>> = std::collections::BTreeMap::new();
    for (m, class, _) in ROSTER {
        seen.entry(m)
            .or_default()
            .push(format!("ROSTER({class:?})"));
    }
    for (m, _) in OUT_OF_SCOPE {
        seen.entry(m).or_default().push("OUT_OF_SCOPE".to_string());
    }
    let dupes: Vec<String> = seen
        .iter()
        .filter(|(_, wheres)| wheres.len() > 1)
        .map(|(m, wheres)| format!("  {m} — {}", wheres.join(" + ")))
        .collect();
    assert_eq!(
        seen.len(),
        ROSTER.len() + OUT_OF_SCOPE.len(),
        "같은 메서드가 여러 분류에 등록됐다. 올바른 분류를 검토해 하나만 남긴다:\n{}",
        dupes.join("\n")
    );
}

/// handler 표에서 .list 메서드를 읽는다. 창 소유권은 추론하지 않고 분류 누락만 찾는다.
fn dispatch_list_methods(root: &Path) -> Vec<String> {
    let src = std::fs::read_to_string(root.join("src/adapters/ipc/handler.rs"))
        .expect("handler.rs 를 읽지 못했다 — 경로가 바뀌었으면 이 가드도 함께 옮긴다");
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        let mut rest = t;
        while let Some(at) = rest.find('"') {
            let after = &rest[at + 1..];
            let Some(end) = after.find('"') else { break };
            let lit = &after[..end];
            if lit.ends_with(".list")
                && lit
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_')
            {
                out.push(lit.to_string());
            }
            rest = &after[end + 1..];
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn the_roster_covers_every_list_method_in_the_dispatch_table() {
    let root = repo_root();
    let found = dispatch_list_methods(&root);
    assert!(
        found.len() >= 20,
        "handler 표에서 .list 메서드를 {}개만 읽었다. 표의 형식과 추출 범위를 확인한다.",
        found.len()
    );
    let known: std::collections::BTreeSet<&str> = ROSTER
        .iter()
        .map(|(m, _, _)| *m)
        .chain(OUT_OF_SCOPE.iter().map(|(m, _)| *m))
        .collect();
    let missing: Vec<&String> = found
        .iter()
        .filter(|m| !known.contains(m.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "handler 표에는 있지만 분류하지 않은 .list 메서드다. 소유권과 호출 범위를 확인해 ROSTER 또는 OUT_OF_SCOPE에 이유와 함께 등록한다:\n  {}",
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn the_open_ones_are_not_silently_emptied() {
    // 해결된 결함은 분류를 옮기고 개수도 함께 갱신해야 한다.
    let open = ROSTER
        .iter()
        .filter(|(_, c, _)| *c == PerEngineNotAggregated)
        .count();
    assert_eq!(
        open, 0,
        "창별인데 합산 안 되는 항목의 수가 바뀌었다. 고쳤으면 분류를 바꾸고 이 수를 \
         함께 내려라 — 남겨 두면 다음 사람이 이미 닫힌 것을 다시 센다."
    );
}
