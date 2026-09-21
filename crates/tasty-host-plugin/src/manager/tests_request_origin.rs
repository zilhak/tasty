//! plugin 으로 넘긴 요청이 **원 IPC 요청의 호스트 번호**를 대기 표에 싣는가(ADR-0436).
//!
//! plugin 에게 가는 것은 hop 마다 새로 받는 호스트 req_id 뿐이다. 그 req_id 를 원 요청으로
//! 되짚는 대응표가 대기 표의 `origin` 칸이고, pre/post hook 사슬은 hop 마다 새 req_id 를
//! 받으므로 **사슬 전체가 같은 번호를 들어야** 느린 hop 이 어느 요청의 것인지 남는다.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tasty_ipc::server::RequestSeq;
use tasty_plugin_manifest::{HookMode, IpcHookDecl};
use tasty_terminal::waker_factory::NoopWakerFactory;

use super::{FinalCaller, PendingRequest, PendingRequestKind, PluginManager};
use crate::PluginPackage;
use crate::process::PluginProcess;
use crate::protocol::PluginResponse;

const OWNER: &str = "com.example.owner";
const EXT: &str = "com.example.ext";

fn mgr_owning(prefix: &str) -> PluginManager {
    let manifest: tasty_plugin_manifest::Manifest = toml::from_str(&format!(
        r#"
manifest_version = 1
id = "{OWNER}"
name = "Origin Fixture"
version = "0.0.1"
api_version = "1.0"

[entry]
type = "process"
command = "echo"
args = []

[[contributes.ipc_namespace]]
prefix = "{prefix}"
"#
    ))
    .expect("fixture manifest should parse");
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    mgr.set_packages_for_tests(vec![PluginPackage {
        dir: PathBuf::from("/nonexistent/origin_fixture"),
        manifest,
    }]);
    mgr
}

fn ok(id: u64) -> PluginResponse {
    PluginResponse {
        id,
        result: Some(serde_json::json!({})),
        error: None,
        error_code: None,
    }
}

/// 대기 표에서 `to` 앞으로 걸린 항목 하나의 (req_id, origin).
fn pending_to(mgr: &PluginManager, to: &str) -> (u64, Option<RequestSeq>) {
    let found: Vec<_> = mgr
        .pending_requests
        .iter()
        .filter(|(_, p)| p.to == to)
        .map(|(id, p)| (*id, p.origin))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "{to} 앞 대기 항목이 하나여야 한다: {found:?}"
    );
    found[0]
}

/// IPC 요청을 plugin namespace 로 넘기면 그 대기 항목이 원 요청의 번호를 든다. 번호가 없는
/// 호출(IPC 큐를 안 지난 것)은 `None` 이다.
#[test]
fn a_forward_puts_the_original_request_seq_on_its_pending_entry() {
    let mut mgr = mgr_owning("orig");
    let (owner, owner_rx) = PluginProcess::stub_with_request_rx(OWNER);
    mgr.processes.insert(OWNER.into(), owner);
    let seq = RequestSeq::next();
    let (tx, _rx) = mpsc::sync_channel(1);
    mgr.forward_namespace_call(
        "orig.run",
        serde_json::json!({}),
        None,
        serde_json::json!(1),
        tx,
        Some(seq),
    );
    let (req_id, origin) = pending_to(&mgr, OWNER);
    assert_eq!(origin, Some(seq));
    let sent = owner_rx
        .try_recv()
        .expect("target 에 ipc.invoke 가 가야 한다");
    assert_eq!(sent.id, req_id, "plugin 에게 가는 것은 호스트 req_id 다");
    let mut keys: Vec<&str> = sent
        .params
        .as_object()
        .expect("ipc.invoke params 는 객체다")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["caller_plugin_id", "method", "params"],
        "원 요청 번호는 plugin wire 에 싣지 않는다(ADR-0436)"
    );

    let (tx, _rx) = mpsc::sync_channel(1);
    mgr.handle_plugin_response(OWNER, ok(req_id));
    mgr.forward_namespace_call(
        "orig.run",
        serde_json::json!({}),
        None,
        serde_json::Value::Null,
        tx,
        None,
    );
    assert_eq!(pending_to(&mgr, OWNER).1, None);
}

/// pre-hook 응답이 target 을 부르면 target 대기 항목이 **같은 번호**를 든다 — hop 마다 req_id
/// 는 새로 받지만 원 요청은 하나다. target 응답이 post-hook 을 부를 때도 같다.
#[test]
fn every_hop_of_a_hook_chain_carries_the_same_request_seq() {
    let mut mgr = mgr_owning("orig");
    let (owner, _owner_rx) = PluginProcess::stub_with_request_rx(OWNER);
    let (ext, _ext_rx) = PluginProcess::stub_with_request_rx(EXT);
    mgr.processes.insert(OWNER.into(), owner);
    mgr.processes.insert(EXT.into(), ext);
    let seq = RequestSeq::next();
    let decl = IpcHookDecl {
        method: "orig.run".into(),
        modifies: Vec::new(),
        mode: HookMode::Observe,
        timeout_ms: 60_000,
    };
    let (tx, _caller_rx) = mpsc::sync_channel(4);
    mgr.pending_requests.insert(
        70,
        PendingRequest::now(
            EXT,
            PendingRequestKind::ExtensionPreIpcHook {
                target_plugin_id: OWNER.into(),
                extension_plugin_id: EXT.into(),
                method: "orig.run".into(),
                params: serde_json::json!({}),
                pre_hook_mode: HookMode::Observe,
                final_caller: FinalCaller::Local {
                    response_tx: tx,
                    original_id: serde_json::json!(1),
                    origin: Some(seq),
                },
                post_hook: Some(decl),
                deadline: Instant::now() + Duration::from_secs(60),
            },
        )
        .for_request(Some(seq)),
    );

    mgr.handle_plugin_response(EXT, ok(70));
    let (target_id, origin) = pending_to(&mgr, OWNER);
    assert_eq!(origin, Some(seq), "pre-hook → target hop 이 번호를 잃었다");

    mgr.handle_plugin_response(OWNER, ok(target_id));
    let (post_id, origin) = pending_to(&mgr, EXT);
    assert_ne!(post_id, 70);
    assert_eq!(origin, Some(seq), "target → post-hook hop 이 번호를 잃었다");
}
