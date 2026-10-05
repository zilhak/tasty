//! 본체 시험이 dispatch 전체를 지나는 namespace 전달을 잴 수 있도록 여는 도구.
//! 프로세스 없이 prefix를 소유하는 stub 플러그인을 붙이고, 그 stub이 받은 요청과
//! 대기 항목의 원 요청 번호(`origin`)를 읽는다.

use std::path::PathBuf;

use tasty_ipc::server::RequestSeq;

use super::PluginManager;
use crate::process::{PluginProcess, RequestTap};

/// stub 플러그인이 받은 요청을 읽는 수신단. 놓으면 stub으로 가는 송신이 끊긴다.
pub struct NamespaceStub {
    plugin_id: String,
    tap: RequestTap,
}

impl NamespaceStub {
    /// 지금까지 받은 요청을 꺼내 (호스트 req_id, `ipc.invoke` 의 메서드 이름) 으로 돌려준다.
    pub fn drain_invokes(&self) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        while let Ok(req) = self.tap.try_recv() {
            let method = req
                .params
                .get("method")
                .and_then(|m| m.as_str())
                .unwrap_or_default()
                .to_string();
            out.push((req.id, method));
        }
        out
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }
}

impl PluginManager {
    /// `prefix` namespace를 소유하는 연결된 stub 플러그인을 붙인다. 설치 목록은 이 하나로 바뀐다.
    pub fn attach_namespace_stub_for_test(
        &mut self,
        plugin_id: &str,
        prefix: &str,
    ) -> NamespaceStub {
        let manifest: tasty_plugin_manifest::Manifest = toml::from_str(&format!(
            r#"
manifest_version = 1
id = "{plugin_id}"
name = "Namespace Stub"
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
        .expect("stub manifest should parse");
        self.set_packages_for_tests(vec![crate::PluginPackage {
            dir: PathBuf::from("/nonexistent/namespace_stub"),
            manifest,
        }]);
        let (process, tap) = PluginProcess::stub_with_request_rx(plugin_id);
        self.processes.insert(plugin_id.to_string(), process);
        NamespaceStub {
            plugin_id: plugin_id.to_string(),
            tap,
        }
    }

    /// `plugin_id` 앞 대기 항목의 (호스트 req_id, 원 요청 번호), req_id 순.
    pub fn pending_origins_for_test(&self, plugin_id: &str) -> Vec<(u64, Option<RequestSeq>)> {
        let mut out: Vec<_> = self
            .pending_requests
            .iter()
            .filter(|(_, p)| p.to == plugin_id)
            .map(|(id, p)| (*id, p.origin))
            .collect();
        out.sort_by_key(|(id, _)| *id);
        out
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc};

    use tasty_terminal::waker_factory::NoopWakerFactory;

    use super::*;

    #[test]
    fn a_forward_reaches_the_stub_with_its_origin() {
        let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
        let stub = mgr.attach_namespace_stub_for_test("com.example.stub", "stubns");
        assert!(mgr.owns_namespace("stubns.run"));
        let seq = RequestSeq::next();
        let (tx, _rx) = mpsc::sync_channel(1);
        mgr.forward_namespace_call(
            "stubns.run",
            serde_json::json!({}),
            None,
            serde_json::json!(1),
            tx,
            Some(seq),
        );
        let sent = stub.drain_invokes();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1, "stubns.run");
        assert_eq!(
            mgr.pending_origins_for_test(stub.plugin_id()),
            [(sent[0].0, Some(seq))]
        );
    }
}
