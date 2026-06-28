#![forbid(unsafe_code)]

//! Tasty Codex plugin — 외부 plugin.
//!
//! `tasty codex spawn|children|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. 자식 terminal surface에서 `codex` CLI를 띄우고 Claude Code의
//! `tasty claude` 명령과 동일한 멀티에이전트 워크플로를 제공한다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod handlers;
mod state;

use std::collections::HashSet;

use serde_json::{Value, json};
use state::CodexState;
use tasty_plugin_sdk::{
    BusHandle, EventDispatchCtx, HostHandle, IpcMethodCtx, IpcMethodError, Plugin,
    SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.codex";
const PLUGIN_VERSION: &str = "0.1.0";

struct CodexPlugin {
    state: CodexState,
}

impl CodexPlugin {
    fn new() -> Self {
        Self {
            state: CodexState::load(),
        }
    }
}

impl Plugin for CodexPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // codex plugin 은 자체 surface_kind 를 등록하지 않는다. 자식 codex 프로세스는
        // 호스트의 일반 terminal surface 에서 실행되며, surface 자체는 plugin 이 만들지
        // 않는다. 매니페스트에 surface_kinds 가 없으므로 이 콜백은 호출되지 않는다.
        SurfaceResult::default()
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    /// Event Bus 1.0: `surface.closed` 구독. 닫힌 surface가 codex child registry에
    /// 있으면 stale 엔트리 제거. claude plugin의 `on_event`와 동일 패턴.
    fn on_event(&mut self, ctx: EventDispatchCtx) {
        if ctx.envelope.key != "surface.closed" {
            return;
        }
        let Some(sid) = ctx
            .envelope
            .payload
            .get("surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
        else {
            return;
        };
        if self.state.unregister_child_by_surface(sid) {
            self.state.save();
        }
    }

    /// worker dispatch 시작 직전 1회 호출.
    ///
    /// 1. `surface.closed` 이벤트 구독 — 매니페스트의 `event_subscribe = ["surface.closed"]` 와 짝.
    /// 2. host 의 살아있는 surface 집합과 `CodexState` child registry 를 cross-check
    ///    하여 stale entry 를 정리한다. Cmd-Q 종료 등으로 `surface.closed` 가 누락된
    ///    경로에서 누적된 ghost 자식들을 매 boot 마다 self-heal.
    fn on_start(&mut self, host: HostHandle, bus: BusHandle) {
        if let Err(e) = bus.subscribe("surface.closed") {
            tracing::warn!("subscribe surface.closed failed: {e}");
        }
        reconcile_on_start(&mut self.state, &host);
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        let IpcMethodCtx {
            method,
            params,
            host,
            ..
        } = ctx;
        match method.as_str() {
            "codex.launch" => handlers::handle_launch(&mut self.state, &host, params),
            "codex.spawn" => handlers::handle_spawn(&mut self.state, &host, params),
            "codex.children" => handlers::handle_children(&self.state, params),
            "codex.parent" => handlers::handle_parent(&self.state, params),
            "codex.tell" => handlers::handle_tell(&host, params),
            "codex.wait" => handlers::handle_wait(&self.state, &host, params),
            "codex.wait_by_surface" => handlers::handle_wait_by_surface(&self.state, &host, params),
            "codex.broadcast" => handlers::handle_broadcast(&self.state, &host, params),
            "codex.kill" => handlers::handle_kill(&mut self.state, &host, params),
            "codex.respawn" => handlers::handle_respawn(&mut self.state, &host, params),
            "codex.install" => handlers::handle_install(&mut self.state),
            "codex.uninstall" => handlers::handle_uninstall(&mut self.state),
            "codex.hook" => handlers::handle_hook(&mut self.state, params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

/// 부팅 시 host 의 살아있는 surface 목록을 `surface.list` IPC 로 조회하여
/// `CodexState` 의 child registry 와 cross-check 한다. IPC 가 실패하거나, 응답이
/// array 가 아니거나, **array 가 비어있는** 경우 — 보수적으로 reconcile 을 건너뛴다.
/// 빈 array 는 boot 시 layout restore 가 아직 일어나지 않은 시점에 reconcile 이
/// 돌아 정상 child 까지 모두 stale 로 오판될 위험이 있어 race 회피 목적으로 skip.
/// 살아있는 entry 를 실수로 제거하는 비용이 stale 이 한 boot 더 남는 비용보다 크다.
///
/// 변경이 1건이라도 발생하면 `state.save()` 를 한 번 호출하여 디스크 sync.
fn reconcile_on_start(state: &mut CodexState, host: &HostHandle) {
    let resp = match host.call("surface.list", json!({})) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("codex reconcile skip: surface.list IPC failed: {e}");
            return;
        }
    };
    let Some(arr) = resp.as_array() else {
        tracing::warn!("codex reconcile skip: surface.list returned non-array");
        return;
    };
    let Some(live) = live_set_or_skip(arr) else {
        return;
    };
    let summary = state.reconcile_with_live_surfaces(&live);
    if summary.removed_children > 0 || summary.removed_parents > 0 {
        state.save();
    }
    tracing::info!(
        removed_children = summary.removed_children,
        removed_parents = summary.removed_parents,
        live_count = live.len(),
        "codex reconcile on_start"
    );
}

/// `surface.list` 응답 array 를 live surface id 집합으로 변환한다. **빈 array** 인
/// 경우 `None` 을 반환하여 호출자가 reconcile 자체를 skip 하도록 신호한다.
/// boot 시 layout restore 가 아직 안 일어난 시점에 빈 응답이 올 수 있고, 그때
/// reconcile 이 돌면 모든 child 가 stale 로 간주되어 정상 entry 까지 사라진다.
/// 호스트는 워크스페이스/페인 구조가 비어 있어도 최소 1개 surface 는 항상 노출하므로
/// 빈 array 는 거의 항상 "아직 준비 안 됨" 신호로 해석해도 안전하다.
fn live_set_or_skip(arr: &[Value]) -> Option<HashSet<u32>> {
    if arr.is_empty() {
        tracing::warn!("codex reconcile skip: empty surface list — likely pre-layout-restore");
        return None;
    }
    Some(
        arr.iter()
            .filter_map(|s| s.get("id").and_then(|v| v.as_u64()).map(|v| v as u32))
            .collect(),
    )
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(CodexPlugin::new())
}

#[cfg(test)]
mod reconcile_tests {
    use super::*;

    #[test]
    fn live_set_or_skip_returns_none_for_empty_array() {
        // 빈 array → None → 호출자가 reconcile skip. CodexState 가 변경되지 않음을
        // 보장하기 위한 첫 관문 (단위로 분리하여 테스트 가능).
        assert_eq!(live_set_or_skip(&[]), None);
    }

    #[test]
    fn live_set_or_skip_returns_ids_for_non_empty() {
        let arr = vec![
            json!({ "id": 1 }),
            json!({ "id": 7 }),
            json!({ "no_id": true }),
        ];
        let set = live_set_or_skip(&arr).expect("non-empty array should produce Some");
        assert_eq!(set.len(), 2);
        assert!(set.contains(&1));
        assert!(set.contains(&7));
    }

    #[test]
    fn empty_surface_list_does_not_mutate_state() {
        // 빈 array 시나리오: state 에 child 1명 등록 → live_set_or_skip 이 None 반환
        // → reconcile_with_live_surfaces 호출 자체가 일어나지 않음 → state 불변.
        // pre-layout-restore race 시 정상 child 가 사라지지 않는지 검증.
        let mut state = CodexState::default();
        let idx = state.next_index_for(10);
        state.register_child(
            10,
            state::ChildEntry {
                child_surface_id: 100,
                index: idx,
                cwd: None,
                role: None,
                nickname: None,
            },
        );
        let before_children = state.list_children(10).len();
        let before_parent = state.parent_of_child(100);

        // 빈 array 가 오면 live_set_or_skip 이 None → 호출자가 early return.
        // 그 동작을 그대로 모사한다. live_set_or_skip(&[]) 가 None 이므로 이 분기는
        // 실행되지 않아야 한다 — 만약 실행되면 아래 assert 가 실패.
        if let Some(live) = live_set_or_skip(&[]) {
            let _ = state.reconcile_with_live_surfaces(&live); // 검증 대상은 state 불변성, summary 는 미사용
        }

        assert_eq!(state.list_children(10).len(), before_children);
        assert_eq!(state.parent_of_child(100), before_parent);
    }
}
