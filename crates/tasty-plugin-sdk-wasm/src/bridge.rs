//! Host bridge — wasm component 의 host imports 가 호출하는 표면.
//!
//! POC 단계에서는 trait 으로만 노출. 실제 production 통합 시점에
//! tasty-host-plugin 의 HostHandle 을 wrap 하는 어댑터로 구현 예정.

use anyhow::Result;

/// host 가 wasm plugin 에 제공하는 모든 import 의 단일 trait.
///
/// trait object 로 사용 (`Box<dyn HostBridge>`). 멀티스레드 호출 시 동기화는
/// 구현체 책임 — wasmtime store 가 single-thread 이므로 POC 에서는 Send/Sync
/// 강제 없음.
pub trait HostBridge {
    /// host IPC generic call. method 예: "tool.clipboard.list".
    /// params: JSON object 직렬화 문자열.
    /// 반환: Ok(JSON 문자열) | Err(에러 메시지).
    fn host_call(&self, method: &str, params_json: &str) -> Result<String, String>;

    /// 구조화 로그.
    fn log(&self, level: &str, msg: &str);

    /// i18n key lookup. plugin lang 카탈로그를 host 가 미리 로드해 보관 (Sub-6.5).
    /// 매치 없으면 key 자체 반환.
    fn tr(&self, key: &str, locale: &str) -> String;
}

/// 테스트/POC 용 in-memory bridge.
#[derive(Default)]
pub struct StubBridge {
    pub logs: std::sync::Mutex<Vec<(String, String)>>,
}

impl HostBridge for StubBridge {
    fn host_call(&self, method: &str, _params_json: &str) -> Result<String, String> {
        // POC: 빈 리스트 반환. clipboard-history 가 fallback 트리 그리는지 확인용.
        match method {
            "tool.clipboard.list" => Ok(r#"{"entries":[]}"#.into()),
            "tool.clipboard.paste" | "tool.clipboard.remove" | "tool.clipboard.clear" => {
                Ok("{}".into())
            }
            other => Err(format!("stub: unhandled method {other}")),
        }
    }

    fn log(&self, level: &str, msg: &str) {
        // 이유: 단일 스레드 WASM stub 이라 poison 이 생길 수 없다 — 조용히 버려도 안전하다.
        if let Ok(mut g) = self.logs.lock() {
            g.push((level.into(), msg.into()));
        }
    }

    fn tr(&self, key: &str, _locale: &str) -> String {
        key.into()
    }
}
