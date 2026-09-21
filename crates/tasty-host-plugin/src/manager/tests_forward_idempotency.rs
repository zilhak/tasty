//! plugin namespace forward 는 **정확히 한 번을 약속하지 않는다**(ADR-0361).
//!
//! 호스트는 forward 에 멱등 키를 싣지도, 같은 호출을 가려내지도, 앞선 답을 되돌려
//! 주지도 않는다. 같은 호출이 두 번 오면 target plugin 이 두 번 실행한다 — 그것을 막을
//! 수 있는 자리는 target 자신뿐이다. 이 시험은 그 사실을 **고정**한다: 누가 호스트에
//! 조용한 중복 제거를 넣으면(그러면 이름 표가 "계약 밖" 이라 선언한 것과 동작이
//! 갈린다) 여기서 빨개진다.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use tasty_terminal::waker_factory::NoopWakerFactory;

use super::PluginManager;
use crate::PluginPackage;
use crate::process::PluginProcess;
use crate::protocol::PluginResponse;

const OWNER: &str = "com.example.owner";

fn mgr_owning(prefix: &str) -> PluginManager {
    let manifest: tasty_plugin_manifest::Manifest = toml::from_str(&format!(
        r#"
manifest_version = 1
id = "{OWNER}"
name = "Forward Fixture"
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
        dir: PathBuf::from("/nonexistent/forward_fixture"),
        manifest,
    }]);
    mgr
}

/// 같은 요청 id · 같은 params 로 두 번 forward 하면 target 이 **두 번** 부름을 받고,
/// caller 는 두 답을 받으며, 어느 답에도 재생 표지가 없다.
#[test]
fn the_same_forward_twice_runs_the_target_twice_and_is_never_replayed() {
    let mut mgr = mgr_owning("fwd");
    let (owner, owner_rx) = PluginProcess::stub_with_request_rx(OWNER);
    mgr.processes.insert(OWNER.into(), owner);

    let (tx, caller_rx) = mpsc::sync_channel(4);
    let params = serde_json::json!({ "n": 1 });
    for _ in 0..2 {
        mgr.forward_namespace_call(
            "fwd.run",
            params.clone(),
            None,
            serde_json::json!(7),
            tx.clone(),
        );
    }

    let first = owner_rx
        .try_recv()
        .expect("첫 forward 가 target 에 안 갔다");
    let second = owner_rx
        .try_recv()
        .expect("두 번째 forward 가 target 에 안 갔다 — 호스트가 같은 호출을 걸렀다");
    for req in [&first, &second] {
        assert_eq!(req.method, tasty_plugin_protocol::METHOD_IPC_INVOKE);
    }
    assert_ne!(first.id, second.id, "두 실행이 한 요청 id 로 묶였다");
    assert!(owner_rx.try_recv().is_err());

    for req in [&first, &second] {
        mgr.handle_plugin_response(
            OWNER,
            PluginResponse {
                id: req.id,
                result: Some(serde_json::json!({ "ran": req.id })),
                error: None,
                error_code: None,
            },
        );
    }
    let answers: Vec<_> = caller_rx.try_iter().collect();
    assert_eq!(answers.len(), 2, "caller 가 실행마다 답을 받지 않았다");
    for a in &answers {
        assert_eq!(a.id, serde_json::json!(7));
        assert!(
            !a.idempotent_replay,
            "forward 답에 재생 표지가 붙었다 — 호스트는 forward 를 재생하지 않는다"
        );
    }
    assert_ne!(
        answers[0].result, answers[1].result,
        "두 번째 답이 첫 실행의 결과를 되돌려 줬다"
    );
}
