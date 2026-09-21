//! 만료로 이미 끝난 요청에 **늦게** 도착한 응답은 아무것도 다시 진행시키지 않는다
//! (`PluginManager::settle_late_response`, ADR-0311 2026-09-21 보강).
//!
//! 두 시험 모두 "만료가 한 번 끝을 냈다" 를 먼저 관측하고 나서 늦은 응답을 넣는다. 만료
//! 쪽 관측이 없으면 늦은 응답이 아무 일도 안 한 것인지 애초에 할 일이 없었던 것인지
//! 가려지지 않는다.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tasty_plugin_manifest::{HookMode, IpcHookDecl};
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::{FinalCaller, PendingRequest, PendingRequestKind, PluginManager};
use crate::process::PluginProcess;
use crate::protocol::PluginResponse;

const TARGET: &str = "com.example.target";
const EXT: &str = "com.example.ext";

fn answer(id: u64) -> PluginResponse {
    PluginResponse {
        id,
        result: Some(serde_json::json!({ "late": true })),
        error: None,
        error_code: None,
    }
}

fn post_hook() -> IpcHookDecl {
    IpcHookDecl {
        method: "target.do".into(),
        modifies: Vec::new(),
        mode: HookMode::Observe,
        timeout_ms: 100,
    }
}

fn past() -> Instant {
    Instant::now() - Duration::from_secs(1)
}

/// target 응답이 만료 뒤에 오면 **post-hook 을 부르지 않는다**. caller 는 이미 `-32004`
/// 를 받았고, post-hook 은 그 결과를 바꾸는 단계라 결과가 나간 뒤에는 바꿀 대상이 없다 —
/// 부르면 extension 에 효과만 남는다.
#[test]
fn a_late_target_answer_does_not_run_the_post_hook_or_answer_twice() {
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    let (target, _target_rx) = PluginProcess::stub_with_request_rx(TARGET);
    let (ext, ext_rx) = PluginProcess::stub_with_request_rx(EXT);
    mgr.processes.insert(TARGET.into(), target);
    mgr.processes.insert(EXT.into(), ext);

    let (tx, caller_rx) = mpsc::sync_channel(4);
    mgr.pending_requests.insert(
        50,
        PendingRequest::now(
            TARGET,
            PendingRequestKind::NamespaceInvokeWithPostHook {
                target_plugin_id: TARGET.into(),
                method: "target.do".into(),
                extension_plugin_id: EXT.into(),
                post_hook_decl: post_hook(),
                final_caller: FinalCaller::Local {
                    response_tx: tx,
                    original_id: serde_json::json!(1),
                },
                deadline: past(),
            },
        ),
    );

    mgr.sweep_expired_requests(Instant::now());
    let expired = caller_rx.try_recv().expect("만료가 caller 에 답해야 한다");
    assert_eq!(expired.error.as_ref().map(|e| e.code), Some(-32004));

    mgr.handle_plugin_response(TARGET, answer(50));

    assert!(
        ext_rx.try_recv().is_err(),
        "caller 가 이미 실패를 받은 호출에 post-hook 이 불렸다"
    );
    assert!(
        caller_rx.try_recv().is_err(),
        "같은 caller 가 같은 요청에 두 번 답을 받았다"
    );
    assert!(
        mgr.pending_requests.is_empty(),
        "늦은 응답이 pending 을 되살렸다"
    );
}

/// pre-hook 응답이 만료 뒤에 오면 **target 을 다시 부르지 않는다** — fail-open 이 원본
/// payload 로 이미 불렀다. 다시 부르면 같은 호출이 두 번 실행된다.
#[test]
fn a_late_pre_hook_answer_does_not_invoke_the_target_a_second_time() {
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    let (target, target_rx) = PluginProcess::stub_with_request_rx(TARGET);
    let (ext, _ext_rx) = PluginProcess::stub_with_request_rx(EXT);
    mgr.processes.insert(TARGET.into(), target);
    mgr.processes.insert(EXT.into(), ext);

    let (tx, _caller_rx) = mpsc::sync_channel(4);
    mgr.pending_requests.insert(
        60,
        PendingRequest::now(
            EXT,
            PendingRequestKind::ExtensionPreIpcHook {
                target_plugin_id: TARGET.into(),
                extension_plugin_id: EXT.into(),
                method: "target.do".into(),
                params: serde_json::json!({ "n": 1 }),
                pre_hook_mode: HookMode::Transform,
                final_caller: FinalCaller::Local {
                    response_tx: tx,
                    original_id: serde_json::json!(2),
                },
                post_hook: None,
                deadline: past(),
            },
        ),
    );

    mgr.sweep_expired_requests(Instant::now());
    let first = target_rx
        .try_recv()
        .expect("fail-open 이 target 을 원본 payload 로 불러야 한다");
    assert_eq!(first.method, tasty_plugin_protocol::METHOD_IPC_INVOKE);

    mgr.handle_plugin_response(
        EXT,
        PluginResponse {
            id: 60,
            result: Some(serde_json::json!({ "modified_payload": { "params": { "n": 2 } } })),
            error: None,
            error_code: None,
        },
    );

    assert!(
        target_rx.try_recv().is_err(),
        "늦은 pre-hook 응답이 target 을 한 번 더 불렀다"
    );
}
