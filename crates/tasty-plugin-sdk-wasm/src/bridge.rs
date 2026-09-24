//! WASM 컴포넌트가 호스트 기능을 호출할 때 사용하는 인터페이스.
//! 현재 실험 실행기는 StubBridge를 사용한다.

use anyhow::Result;

/// WASM 컴포넌트에 제공하는 호출·로그·번역 기능.
/// 런타임은 Arc<dyn HostBridge + Send + Sync>로 전달받는다.
pub trait HostBridge {
    /// host IPC generic call. method 예: "tool.clipboard.list".
    /// params: JSON object 직렬화 문자열.
    /// 반환: Ok(JSON 문자열) | Err(에러 메시지).
    fn host_call(&self, method: &str, params_json: &str) -> Result<String, String>;

    /// 구조화 로그.
    fn log(&self, level: &str, msg: &str);

    /// 번역 키와 언어에 해당하는 문자열을 반환한다. StubBridge는 키를 그대로 반환한다.
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
        // 이유: 로그 잠금이 poison되면 이 실험 실행기는 해당 로그를 저장하지 않는다.
        if let Ok(mut g) = self.logs.lock() {
            g.push((level.into(), msg.into()));
        }
    }

    fn tr(&self, key: &str, _locale: &str) -> String {
        key.into()
    }
}
