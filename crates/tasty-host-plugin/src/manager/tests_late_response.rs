//! 요청 만료를 먼저 확인한 뒤 늦은 응답을 넣어 후속 호출·회신이 반복되지 않는지 검사한다.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tasty_plugin_manifest::{HookMode, IpcHookDecl};
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::{FinalCaller, PendingRequest, PendingRequestKind, PluginManager, TargetOutcome};
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

/// 만료 뒤 target 응답이 와도 post-hook을 부르지 않는다. 호출자는 이미 오류를 받았다.
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
                    origin: None,
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

/// pre-hook이 늦게 응답해도 fail-open으로 이미 호출한 target을 다시 부르지 않는다.
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
                    origin: None,
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

/// post-hook이 늦게 응답해도 이미 보낸 target 결과를 다시 회신하지 않는다.
#[test]
fn a_late_post_hook_answer_does_not_answer_the_caller_a_second_time() {
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    let (target, target_rx) = PluginProcess::stub_with_request_rx(TARGET);
    let (ext, _ext_rx) = PluginProcess::stub_with_request_rx(EXT);
    mgr.processes.insert(TARGET.into(), target);
    mgr.processes.insert(EXT.into(), ext);

    let (tx, caller_rx) = mpsc::sync_channel(4);
    mgr.pending_requests.insert(
        70,
        PendingRequest::now(
            EXT,
            PendingRequestKind::ExtensionPostIpcHook {
                extension_plugin_id: EXT.into(),
                method: "target.do".into(),
                post_hook_mode: HookMode::Transform,
                target_outcome: TargetOutcome::Ok(serde_json::json!({ "from": "target" })),
                final_caller: FinalCaller::Local {
                    response_tx: tx,
                    original_id: serde_json::json!(3),
                    origin: None,
                },
                deadline: past(),
            },
        ),
    );

    mgr.sweep_expired_requests(Instant::now());
    let first = caller_rx
        .try_recv()
        .expect("fail-open 이 target 결과를 caller 에 보내야 한다");
    assert_eq!(first.id, serde_json::json!(3));
    assert_eq!(first.result, Some(serde_json::json!({ "from": "target" })));

    mgr.handle_plugin_response(
        EXT,
        PluginResponse {
            id: 70,
            result: Some(serde_json::json!({ "modified_payload": { "from": "hook" } })),
            error: None,
            error_code: None,
        },
    );

    assert!(
        caller_rx.try_recv().is_err(),
        "늦은 post-hook 응답이 caller 에 두 번째 답을 보냈다"
    );
    assert!(
        target_rx.try_recv().is_err(),
        "늦은 post-hook 응답이 target 을 불렀다"
    );
    assert!(
        mgr.pending_requests.is_empty(),
        "늦은 응답이 pending 을 되살렸다"
    );
}
