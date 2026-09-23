//! plugin 으로 넘긴 요청이 **원 IPC 요청의 호스트 번호**를 대기 표에 싣는가(docs/architecture/ipc-server.md#느린-요청-추적).
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
        "원 요청 번호는 plugin wire 에 싣지 않는다(docs/architecture/ipc-server.md#느린-요청-추적)"
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

/// `orig.run` 에 제한 `timeout_ms` 의 pre-hook 을 선언한 확장이 붙은 매니저. 두 plugin 의 요청 수신 통로를 함께 준다.
fn mgr_with_pre_hook(
    timeout_ms: u64,
) -> (
    PluginManager,
    crate::process::RequestTap,
    crate::process::RequestTap,
) {
    let mut mgr = mgr_owning("orig");
    let ext: tasty_plugin_manifest::Manifest = toml::from_str(&format!(
        r#"
manifest_version = 1
id = "{EXT}"
name = "Origin Extension Fixture"
version = "0.0.1"
api_version = "1.0"

[entry]
type = "process"
command = "echo"
args = []

[extends]
plugin_id = "{OWNER}"
version_req = ">=0.0.1"
api_version = "1"

[[extends.pre_ipc]]
method = "orig.run"
mode = "observe"
timeout_ms = {timeout_ms}
"#
    ))
    .expect("extension fixture manifest should parse");
    mgr.packages.push(PluginPackage {
        dir: PathBuf::from("/nonexistent/origin_ext_fixture"),
        manifest: ext,
    });
    mgr.config.set_granted(EXT, vec![format!("ext:{OWNER}")]);
    let (owner, owner_rx) = PluginProcess::stub_with_request_rx(OWNER);
    let (ext_proc, ext_rx) = PluginProcess::stub_with_request_rx(EXT);
    mgr.processes.insert(OWNER.into(), owner);
    mgr.processes.insert(EXT.into(), ext_proc);
    (mgr, owner_rx, ext_rx)
}

/// pre-hook 을 선언한 확장이 붙은 namespace 로 넘기면 **첫 hop 이 확장으로** 가고, 그 대기
/// 항목도 원 요청의 번호를 든다 — 사슬의 첫 칸이 번호를 잃으면 뒤 hop 이 모두 `None` 이 된다.
#[test]
fn a_forward_through_a_pre_hook_puts_the_request_seq_on_the_hook_entry() {
    let (mut mgr, _owner_rx, ext_rx) = mgr_with_pre_hook(60_000);
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
    let hook = ext_rx
        .try_recv()
        .expect("확장에 pre-hook 호출이 먼저 가야 한다");
    let (req_id, origin) = pending_to(&mgr, EXT);
    assert_eq!(hook.id, req_id);
    assert_eq!(origin, Some(seq), "pre-hook hop 이 번호를 잃었다");
}

/// 빠른 pre-hook 뒤에 느린 target 이 오면 **한 줄에 호스트 몫과 hop 둘**이 순서대로 남는다 —
/// 빠른 pre-hook 응답에서 열린 자리를 닫으면 target hop 이 호스트 몫과 이을 자리를 잃는다.
#[test]
fn a_fast_pre_hook_and_its_slow_target_land_on_one_row_in_order() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, _owner_rx, _ext_rx) = mgr_with_pre_hook(60_000);
    mgr.set_slow_requests(log.clone());
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
    log.finish_host(fast_host_leg(seq));
    let (hook_id, _) = pending_to(&mgr, EXT);
    mgr.handle_plugin_response(EXT, ok(hook_id));
    assert!(
        log.snapshot().rows.is_empty(),
        "빠른 pre-hook 은 아직 줄이 아니다"
    );
    let (target_id, _) = pending_to(&mgr, OWNER);
    if let Some(p) = mgr.pending_requests.get_mut(&target_id) {
        p.sent_at = Instant::now() - Duration::from_millis(150);
    }
    mgr.handle_plugin_response(OWNER, ok(target_id));

    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(
        rows[0].host.is_some(),
        "호스트 몫과 이어지지 않았다: {rows:?}"
    );
    let hops: Vec<_> = rows[0]
        .plugin_hops
        .iter()
        .map(|h| (h.plugin_id.as_str(), h.host_request_id))
        .collect();
    assert_eq!(hops, [(EXT, hook_id), (OWNER, target_id)]);
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

/// 빠른 호스트 몫 — dispatch 루프가 forward 직후 채우는 값을 흉내 낸다.
fn fast_host_leg(seq: RequestSeq) -> tasty_telemetry::slow_requests::HostLeg<'static> {
    tasty_telemetry::slow_requests::HostLeg {
        request_seq: seq.get(),
        method: "orig.run",
        caller: tasty_telemetry::slow_requests::CallerKind::Local,
        queue_wait: Duration::from_micros(10),
        host: Duration::from_micros(10),
        outcome: Default::default(),
    }
}

fn forwarded(
    log: &Arc<tasty_telemetry::SlowRequestLog>,
) -> (
    PluginManager,
    RequestSeq,
    mpsc::Receiver<tasty_ipc::protocol::JsonRpcResponse>,
) {
    let mut mgr = mgr_owning("orig");
    mgr.set_slow_requests(log.clone());
    let (owner, _owner_rx) = PluginProcess::stub_with_request_rx(OWNER);
    mgr.processes.insert(OWNER.into(), owner);
    let seq = RequestSeq::next();
    let (tx, rx) = mpsc::sync_channel(1);
    mgr.forward_namespace_call(
        "orig.run",
        serde_json::json!({}),
        None,
        serde_json::json!(1),
        tx,
        Some(seq),
    );
    // 호스트 몫은 dispatch 루프가 forward 직후 채운다 — 여기서는 빠른 값으로 흉내 낸다.
    log.finish_host(fast_host_leg(seq));
    assert!(
        log.snapshot().rows.is_empty(),
        "호스트 몫만으로는 안 느리다"
    );
    (mgr, seq, rx)
}

/// plugin 이 늦게 답한 forward 는 그 대기가 **원 요청의 줄**에 붙는다 — hop 의 호스트 req_id 가
/// plugin 이 받은 JSON-RPC id 와 같아 plugin 로그를 원 요청으로 되짚을 수 있다.
#[test]
fn a_slow_answer_lands_on_the_original_requests_row() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, seq, _rx) = forwarded(&log);
    let (req_id, _) = pending_to(&mgr, OWNER);
    // 실제로 기다리지 않고 보낸 시각을 뒤로 민다 — 벽시계는 부하에 흔들린다.
    if let Some(p) = mgr.pending_requests.get_mut(&req_id) {
        p.sent_at = Instant::now() - Duration::from_millis(150);
    }
    mgr.handle_plugin_response(OWNER, ok(req_id));
    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].request_seq, seq.get());
    assert!(rows[0].host.is_some(), "호스트 몫과 이어지지 않았다");
    let hop = &rows[0].plugin_hops[0];
    assert_eq!(
        (hop.plugin_id.as_str(), hop.host_request_id, hop.outcome),
        (
            OWNER,
            req_id,
            tasty_telemetry::slow_requests::HopOutcome::Ok
        )
    );
    assert!(hop.wait_us >= 150_000, "{hop:?}");
}

/// 끝내 답이 없어 만료된 forward 도 같은 줄에 `expired` 로 붙는다 — 분포(`plugin_round_trip`)는
/// 끝점이 없어 이 건을 못 세지만, 링은 그 한 건을 남긴다.
#[test]
fn an_expired_forward_lands_on_the_row_as_expired() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, seq, rx) = forwarded(&log);
    let (req_id, _) = pending_to(&mgr, OWNER);
    mgr.sweep_expired_requests(Instant::now() + Duration::from_secs(3600));
    assert_eq!(
        rx.try_recv()
            .expect("만료가 caller 에 답한다")
            .error
            .map(|e| e.code),
        Some(-32004)
    );
    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].request_seq, seq.get());
    assert_eq!(rows[0].plugin_hops[0].host_request_id, req_id);
    assert_eq!(
        rows[0].plugin_hops[0].outcome,
        tasty_telemetry::slow_requests::HopOutcome::Expired
    );
}

/// 빨리 답한 forward 는 줄을 안 남긴다 — 링은 전 요청의 기록이 아니다.
#[test]
fn a_fast_answer_leaves_no_row() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, _seq, _rx) = forwarded(&log);
    let (req_id, _) = pending_to(&mgr, OWNER);
    mgr.handle_plugin_response(OWNER, ok(req_id));
    assert!(log.snapshot().rows.is_empty());
}

/// 답을 기다리던 plugin 이 치워지면 그 hop 이 `cancelled` 로 같은 줄에 붙는다.
#[test]
fn a_forward_cancelled_by_plugin_removal_lands_as_cancelled() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, seq, _rx) = forwarded(&log);
    let (req_id, _) = pending_to(&mgr, OWNER);
    if let Some(p) = mgr.pending_requests.get_mut(&req_id) {
        p.sent_at = Instant::now() - Duration::from_millis(150);
    }
    mgr.cancel_pending_namespace_calls(OWNER, "removed");
    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].request_seq, seq.get());
    assert_eq!(
        (
            rows[0].plugin_hops[0].host_request_id,
            rows[0].plugin_hops[0].outcome
        ),
        (
            req_id,
            tasty_telemetry::slow_requests::HopOutcome::Cancelled
        )
    );
}

/// pre-hook 이 만료돼도 fail-open 으로 target 이 불리므로 사슬은 이어진다 — 짧은 hook 의 만료가
/// 열린 자리를 닫으면 뒤의 느린 target 이 호스트 몫과 이을 자리를 잃는다.
#[test]
fn an_expired_pre_hook_keeps_the_row_open_for_its_target() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, _owner_rx, _ext_rx) = mgr_with_pre_hook(50);
    mgr.set_slow_requests(log.clone());
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
    log.finish_host(fast_host_leg(seq));
    let (hook_id, _) = pending_to(&mgr, EXT);
    // hook 제한(50 ms)은 넘기되 문턱(100 ms) 아래에서 만료시킨다.
    let sent_at = mgr.pending_requests[&hook_id].sent_at;
    mgr.sweep_expired_requests(sent_at + Duration::from_millis(60));
    assert!(
        log.snapshot().rows.is_empty(),
        "문턱 아래 만료는 아직 줄이 아니다"
    );
    let (target_id, _) = pending_to(&mgr, OWNER);
    if let Some(p) = mgr.pending_requests.get_mut(&target_id) {
        p.sent_at = Instant::now() - Duration::from_millis(150);
    }
    mgr.handle_plugin_response(OWNER, ok(target_id));

    let rows = log.snapshot().rows;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(
        rows[0].host.is_some(),
        "호스트 몫과 이어지지 않았다: {rows:?}"
    );
    let hops: Vec<_> = rows[0]
        .plugin_hops
        .iter()
        .map(|h| (h.plugin_id.as_str(), h.outcome))
        .collect();
    assert_eq!(
        hops,
        [
            (EXT, tasty_telemetry::slow_requests::HopOutcome::Expired),
            (OWNER, tasty_telemetry::slow_requests::HopOutcome::Ok)
        ]
    );
}

/// 이 스코프 동안 나가는 tracing 이벤트를 문자열로 모은다(`builtin.rs` 시험과 같은 모양).
fn capture_logs(f: impl FnOnce()) -> String {
    use std::sync::Mutex;
    #[derive(Clone)]
    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buf = Buf(Arc::new(Mutex::new(Vec::new())));
    let sink = buf.clone();
    let sub = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .finish();
    tracing::subscriber::with_default(sub, f);
    let out = buf.0.lock().unwrap().clone();
    String::from_utf8_lossy(&out).into_owned()
}

/// `needle` 을 담은 WARN 줄 하나.
fn warn_line<'a>(logs: &'a str, needle: &str) -> &'a str {
    logs.lines()
        .find(|l| l.contains("WARN") && l.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} 를 담은 WARN 줄이 없다: {logs}"))
}

/// plugin 이 오류로 답한 경고 한 줄에 호스트 req_id 와 원 요청 번호가 **함께** 있다 — plugin
/// 로그의 id 에서 원 요청으로 되짚는 열쇠다. 줄을 새로 만들지 않는다.
#[test]
fn the_error_answer_warning_names_the_host_id_and_the_request_seq_on_one_line() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, seq, _rx) = forwarded(&log);
    let (req_id, _) = pending_to(&mgr, OWNER);
    let logs = capture_logs(|| {
        mgr.handle_plugin_response(
            OWNER,
            PluginResponse {
                id: req_id,
                result: None,
                error: Some("boom".into()),
                error_code: None,
            },
        );
    });
    let line = warn_line(&logs, "response error");
    assert!(
        line.contains(&format!("id={req_id}")) && line.contains(&format!("request_seq={seq}")),
        "{line}"
    );
    assert_eq!(logs.matches("WARN").count(), 1, "경고 줄이 늘었다: {logs}");
}

/// namespace 만료 경고 한 줄에도 둘이 함께 있다. caller 에 가는 문구는 그대로다.
#[test]
fn the_expiry_warning_names_the_host_id_and_the_request_seq_on_one_line() {
    let log = Arc::new(tasty_telemetry::SlowRequestLog::default());
    let (mut mgr, seq, rx) = forwarded(&log);
    let (req_id, _) = pending_to(&mgr, OWNER);
    let logs = capture_logs(|| {
        mgr.sweep_expired_requests(Instant::now() + Duration::from_secs(3600));
    });
    let line = warn_line(&logs, "did not answer");
    assert!(
        line.contains(&format!("id={req_id}")) && line.contains(&format!("request_seq={seq}")),
        "{line}"
    );
    let answer = rx.try_recv().expect("caller answer").error.expect("error");
    assert!(
        !answer.message.contains("request_seq"),
        "caller 문구가 바뀌었다: {}",
        answer.message
    );
}
