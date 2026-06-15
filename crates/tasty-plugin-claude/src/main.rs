//! Tasty Claude Code plugin — 외부 plugin.
//!
//! `tasty claude launch|spawn|children|parent|tell|wait|broadcast|kill|respawn|install|uninstall|hook`
//! CLI 세트를 제공한다. Phase 2가 끝나면 호스트 내부에 박혀 있던 Claude Code 통합이
//! 이 plugin으로 일원화되며, codex/aider 등 다른 코딩 에이전트 plugin들과 동등한 1급
//! 확장점 위에서 동작한다.
//!
//! 호스트 코드에는 의존하지 않으며 `tasty-plugin-sdk`만 사용한다.

mod error_scan;
mod handlers;
mod hook;
mod install;
mod state;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use error_scan::ErrorScanner;
use handlers::*;
use serde_json::{Value, json};
use state::ClaudeState;
use tasty_plugin_sdk::{
    EventDispatchCtx, HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx,
    SurfaceEventCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.claude";
const PLUGIN_VERSION: &str = "0.1.0";

/// PTY 에러 폴링 간격. 호스트 메모리 스캔(O(1))과의 정확도 차이를 좁히기 위해
/// 짧게. 자식 N명에 대해 N IPC/주기지만 N이 10 이하인 일상 시나리오에서는 무시
/// 가능한 부하 (8 calls/sec @ 10 children).
const ERROR_SCAN_INTERVAL: Duration = Duration::from_millis(800);

struct ClaudePlugin {
    state: ClaudeState,
    scanner: Arc<Mutex<ErrorScanner>>,
}

impl ClaudePlugin {
    fn new() -> Self {
        Self {
            state: ClaudeState::load(),
            scanner: Arc::new(Mutex::new(ErrorScanner::new())),
        }
    }
}

impl Plugin for ClaudePlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        // claude plugin은 자체 surface_kind를 등록하지 않는다 — 자식 Claude 프로세스는
        // 호스트의 일반 terminal surface에서 실행되며, surface 자체는 plugin이 직접
        // 만들지 않는다. 매니페스트에 surface_kinds가 없으므로 이 콜백은 호출되지
        // 않을 것이다.
        SurfaceResult::default()
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        // 분기를 작게 cutover-안전 단계로 채워나간다. BUILTINS 미등록 동안엔 모든
        // claude.* 트래픽이 호스트로 가므로 본 분기는 실제로는 단위 테스트로만 검증.
        // 호스트 핸들러 제거 + BUILTINS 등록은 step 04e cutover에서 atomic으로.
        match ctx.method.as_str() {
            "claude.hook" => hook::handle_claude_hook(&mut self.state, &ctx.host, &ctx.params),
            "claude.install" => match install::run_install() {
                Ok(added) => Ok(json!({ "installed": added })),
                Err(e) => Err(IpcMethodError::new(format!("install failed: {e}"))),
            },
            "claude.uninstall" => match install::run_uninstall() {
                Ok(removed) => Ok(json!({ "uninstalled": removed })),
                Err(e) => Err(IpcMethodError::new(format!("uninstall failed: {e}"))),
            },
            // step 04a: plugin 자기 ClaudeState만 보면 응답 가능한 핸들러들.
            "claude.set_idle_state" => handle_set_idle_state(&mut self.state, &ctx.params),
            "claude.set_needs_input" => handle_set_needs_input(&mut self.state, &ctx.params),
            "claude.parent" => handle_parent(&self.state, &ctx.params),
            // step 04b: 호스트 IPC(surface.foreground_process / surface.locate /
            // surface.close)와 ClaudeState를 함께 조합하는 핸들러들.
            "claude.children" => handle_children(&self.state, &ctx.host, &ctx.params),
            "claude.wait" => handle_wait(&self.state, &ctx.host, &ctx.params),
            "claude.wait_by_surface" => handle_wait_by_surface(&self.state, &ctx.host, &ctx.params),
            "claude.wait_any" => handle_wait_any(&self.state, &ctx.host, &ctx.params),
            "claude.kill" => handle_kill(&mut self.state, &ctx.host, &ctx.params),
            // step 04c: PTY 송신 핸들러. surface.send IPC를 통해 자식 terminal에
            // text를 보낸다.
            "claude.broadcast" => handle_broadcast(&self.state, &ctx.host, &ctx.params),
            "claude.tell" => handle_tell(&ctx.host, &ctx.params),
            // step 04d.1: 새 workspace에 claude 띄우기.
            "claude.launch" => handle_launch(&self.scanner, &ctx.host, &ctx.params),
            // step 04d.2: 자식 surface의 PTY를 갈아끼우고 claude 재시작.
            "claude.respawn" => handle_respawn(&mut self.state, &ctx.host, &ctx.params),
            // step 04d.3: parent surface가 사는 workspace 내 spawn pane을 자동
            // 관리(필요 시 생성)하고, 2x2 grid에 따라 새 자식 surface를 배치 +
            // claude 실행.
            "claude.spawn" => handle_spawn(&mut self.state, &ctx.host, &ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }

    fn on_event(&mut self, ctx: EventDispatchCtx) {
        // Event Bus 1.0: `surface.closed` 구독 시 호출. 닫힌 surface가 claude
        // 자식이었다면 child registry에서 제거하고, parent였다면 closed_parents로
        // 마킹한다. error scan에서도 함께 제외한다.
        if ctx.envelope.key != "surface.closed" {
            return;
        }
        let sid = match ctx
            .envelope
            .payload
            .get("surface_id")
            .and_then(|v| v.as_u64())
        {
            Some(v) => v as u32,
            None => return,
        };
        let parent_was_child = self.state.parent_of_child(sid).is_some();
        if parent_was_child {
            self.state.unregister_child(sid);
            self.state.save();
            if let Ok(mut s) = self.scanner.lock() {
                s.disable(sid);
            }
            return;
        }
        if self.state.is_known_parent(sid) {
            self.state.mark_parent_closed(sid);
            self.state.save();
        }
    }

    fn on_start(&mut self, host: HostHandle, bus: tasty_plugin_sdk::BusHandle) {
        // worker dispatch가 시작되기 직전에 1회 호출.
        // - `surface.closed` 이벤트 구독 (Event Bus 1.0). 옛 surface_observer
        //   매니페스트 필드의 대체 경로.
        // - host 의 살아있는 surface 집합과 `ClaudeState` child registry 를
        //   cross-check 하여 stale entry 정리. Cmd-Q / CLI `pane.close` 경유 등
        //   `surface.closed` 가 누락된 경로에서 누적된 ghost 자식들을 매 boot
        //   마다 self-heal. codex plugin 의 동명 helper 와 패턴 일치.
        // - PTY error scan을 위한 background polling thread spawn. 호스트가
        //   메모리 스캔하던 패턴을 1:1로 옮겼고 (`error_scan.rs::CLAUDE_ERROR_PATTERN`),
        //   polling 간격은 800ms로 호스트 tick에 근접하게 맞춘다.
        if let Err(e) = bus.subscribe("surface.closed") {
            tracing::warn!("subscribe surface.closed failed: {e}");
        }
        reconcile_on_start(&mut self.state, &self.scanner, &host);
        let scanner = self.scanner.clone();
        std::thread::Builder::new()
            .name("claude-error-scan".into())
            .spawn(move || error_scan_loop(scanner, host))
            .expect("spawn claude-error-scan thread");
    }
}

// ─── step 04a 핸들러들 ───────────────────────────────────────────────────────
//
// 호스트 src/ipc/handler/claude.rs의 응답 JSON과 byte-for-byte 동일해야 cutover
// 후 CLI 출력 회귀가 없다. param 키 이름 / 응답 필드 / 누락된 surface_id의 에러
// 분기까지 1:1 보존한다.

fn error_scan_loop(scanner: Arc<Mutex<ErrorScanner>>, host: HostHandle) {
    loop {
        std::thread::sleep(ERROR_SCAN_INTERVAL);
        // lock을 짧게 잡고 snapshot만 떠서 IPC 호출 동안 다른 메서드(enable/disable)가
        // 끼어들 수 있게 한다. snapshot 후 surface가 disable되면 다음 tick에 자연
        // 반영.
        let surfaces = match scanner.lock() {
            Ok(s) => s.enabled_snapshot(),
            Err(e) => {
                tracing::error!("claude scanner mutex poisoned: {e}");
                return;
            }
        };
        for sid in surfaces {
            // 각 IPC call은 최대 60초까지 block 가능하지만 정상 응답은 ms 단위.
            // 한 surface에서 timeout이 나도 나머지에 영향 없도록 그냥 진행.
            if let Ok(mut s) = scanner.lock() {
                // 반환값(매치된 snippet)은 단위 테스트용. polling 루프에서는 무시.
                s.scan_one(&host, sid);
            }
        }
    }
}

/// 부팅 시 host 의 살아있는 surface 목록을 `surface.list` IPC 로 조회하여
/// `ClaudeState` 의 child registry 와 cross-check 한다. IPC 가 실패하거나, 응답이
/// array 가 아니거나, **array 가 비어있는** 경우 — 보수적으로 reconcile 을 건너뛴다.
/// 빈 array 는 boot 시 layout restore 가 아직 일어나지 않은 시점에 reconcile 이
/// 돌아 정상 child 까지 모두 stale 로 오판될 위험이 있어 race 회피 목적으로 skip.
/// 살아있는 entry 를 실수로 제거하는 비용이 stale 이 한 boot 더 남는 비용보다 크다.
///
/// codex plugin 의 동명 helper 와 패턴 일치. claude 특유: 제거된 각 child
/// surface_id 에 대해 `ErrorScanner::disable` 호출 — reconcile 후 죽은 surface 를
/// 가리키는 background polling 을 즉시 중단해 IPC 오류 발산을 막는다.
///
/// 변경이 1건이라도 발생하면 `state.save()` 를 한 번 호출하여 디스크 sync.
fn reconcile_on_start(
    state: &mut ClaudeState,
    scanner: &Arc<Mutex<ErrorScanner>>,
    host: &HostHandle,
) {
    let resp = match host.call("surface.list", json!({})) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("claude reconcile skip: surface.list IPC failed: {e}");
            return;
        }
    };
    let Some(arr) = resp.as_array() else {
        tracing::warn!("claude reconcile skip: surface.list returned non-array");
        return;
    };
    let Some(live) = live_set_or_skip(arr) else {
        return;
    };
    // reconcile 전에 dead 가 될 child surface_id 를 snapshot — reconcile 이 끝나면
    // state 에서 사라져 ErrorScanner 정리 대상을 알 수 없게 된다.
    let stale_sids = state.collect_stale_child_surface_ids(&live);
    let summary = state.reconcile_with_live_surfaces(&live);
    if summary.removed_children > 0 || summary.removed_parents > 0 {
        state.save();
        // 죽은 surface 를 가리키던 background error scan 폴링 중단. lock 실패는
        // poisoned mutex (다른 thread 가 패닉) 의미이므로 warn 로 흘려보낸다.
        match scanner.lock() {
            Ok(mut s) => {
                for sid in &stale_sids {
                    s.disable(*sid);
                }
            }
            Err(e) => tracing::warn!("claude reconcile: scanner mutex poisoned: {e}"),
        }
    }
    tracing::info!(
        removed_children = summary.removed_children,
        removed_parents = summary.removed_parents,
        live_count = live.len(),
        "claude reconcile on_start"
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
        tracing::warn!("claude reconcile skip: empty surface list — likely pre-layout-restore");
        return None;
    }
    Some(
        arr.iter()
            .filter_map(|s| s.get("id").and_then(|v| v.as_u64()).map(|v| v as u32))
            .collect(),
    )
}

#[cfg(test)]
mod reconcile_tests {
    use super::*;

    #[test]
    fn live_set_or_skip_returns_none_for_empty_array() {
        // 빈 array → None → 호출자가 reconcile skip. ClaudeState 가 변경되지 않음을
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
        let mut state = ClaudeState::default();
        state.register_child(
            10,
            state::ChildEntry {
                child_surface_id: 100,
                index: 1,
                cwd: None,
                role: None,
                nickname: None,
            },
        );
        let before_children = state.list_children(10).len();
        let before_parent = state.parent_of_child(100);

        if let Some(live) = live_set_or_skip(&[]) {
            let _ = state.reconcile_with_live_surfaces(&live); // 본 분기는 실행되지 않아야 함
        }

        assert_eq!(state.list_children(10).len(), before_children);
        assert_eq!(state.parent_of_child(100), before_parent);
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ClaudePlugin::new())
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
