use serde::{Deserialize, Serialize};

// 전송 계층이 직접 내는 오류 코드의 자리 — `-32060..-32069`.
//
// 이 구역이 도메인 코드(`-32000..-32059`)와 갈리는 이유는 **호출자가 다음에 할 일이
// 다르기 때문**이다. 도메인 코드는 요청이 handler 까지 갔고 거기서 거절됐다는 뜻이라
// 인자·주체·상태를 본다. 이 구역은 요청이 **handler 에 닿지도 못했다**는 뜻이라 볼
// 곳이 요청의 모양이나 서버의 수용 여력이다. 두 사실을 같은 구역에 섞으면 재시도
// 정책을 고를 수 없다 — 앞의 것은 고쳐야 다시 되고, 뒤의 것은 그대로 다시 해도 된다.

/// 요청 한 줄이 서버의 줄 상한(`MAX_REQUEST_LINE_BYTES`)을 넘었다. 넘긴 줄의 나머지가
/// 소켓에 남아 있으므로 그 연결은 이 응답 **뒤에 닫힌다** — 계속 읽으면 한 줄이 여러
/// 요청으로 쪼개져 들어가 상한이 다시 없는 것이 된다. 상한 값은 응답 문구에 실린다.
pub const ERR_REQUEST_LINE_TOO_LONG: i32 = -32060;

/// 요청이 실은 응답 대기 상한이 만료됐다. **그 요청이 실행됐는지 안 됐는지는 이 답으로 알 수
/// 없다** — 호스트는 응답 통로를 놓았을 뿐이고 요청 자체는 메인 스레드에서 계속 실행될 수
/// 있다. 그래서 이 코드는 실패가 아니라 **결과 불명**을 뜻한다.
///
/// 이 구분이 코드 하나를 따로 쓸 만한 이유: 호출자가 다음에 할 일이 그 값에 달렸다.
/// 부수효과가 남는 메서드(`MethodEffect::Mutate`)를 그냥 재전송하면 **두 번째 효과**가
/// 남는다. 기존 코드 중에 이 사실을 뜻하는 것이 없었다 — `-32603`(내부 오류)도
/// `-32000`(서버 사정)도 "안 됐다" 로 읽힌다.
pub const ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN: i32 = -32061;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: Option<serde_json::Value>,
    /// 자식 agent (claude.spawn 등으로 호스트가 띄운 프로세스) 가
    /// 호스트에 IPC 호출 시 자기 신원을 증명하는 token. 64-char lowercase hex.
    /// 호스트는 [`crate::ipc::session::SessionStore`] 로 resolve 해 `CallerContext::Agent`
    /// 를 만든다. 토큰이 invalid/expired/revoked 면 `permission_denied` 로 거부 —
    /// Local 로 fallback 하지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// 이 요청의 응답을 호출자가 기다릴 최대 시간(밀리초). 서버는 그 시간이 지나면
    /// 응답 통로를 놓고 [`ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN`] 으로 답한다.
    ///
    /// **없으면 상한이 없다.** 그것이 이 필드 이전의 동작이고, 기본값이 상한이 되면
    /// 사람의 결재를 기다리는 호출(`approval.await`)이 잘린다 — 사람에게는 상한을 둘 수
    /// 없다. `Some(0)` 도 같은 뜻으로 읽는다(무한). 레포의 기존 관례가 그렇다:
    /// `approval.await` 와 `agent.task_await` 의 `timeout_ms` 파라미터가 0 을 무한으로
    /// 읽는다. **이 필드는 그 관례를 봉투 수준으로 올린 것**이고, 그래서 메서드마다
    /// 다시 정하지 않아도 모든 메서드에 같은 뜻으로 붙는다.
    ///
    /// 구 서버는 이 키를 **조용히 무시한다** — 이 구조체에 `deny_unknown_fields` 가 없다.
    /// 그래서 새 client 가 보내도 깨지지 않고, 대신 상한이 안 걸린다. 서버가 이것을
    /// 읽는지 확인하려면 `system.info` 의 capability 목록에서 `ipc.request-deadline` 을
    /// 본다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: serde_json::Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }

    pub fn method_not_found(id: serde_json::Value, method: &str) -> Self {
        Self::error(id, -32601, format!("Method not found: {}", method))
    }

    /// 외부(CLI / 네트워크 IPC) 호출자가 dispatch 끝까지 못 닿은 이름에 대한 답.
    ///
    /// `-32601`("그런 메서드 없다")은 호출자를 **이름을 의심하는 쪽**으로 보낸다 —
    /// 오타를 고치거나 표를 다시 읽는다. 그 방향에 고칠 것이 없는 경우가 셋이고, 셋 다
    /// **이름은 맞다**. 무엇이 다른지가 호출자가 다음에 할 일을 가른다:
    ///
    /// | 사실 | 코드 | 호출자가 다음에 할 일 |
    /// |------|------|----------------------|
    /// | 부를 수 있는 주체가 다르다 | `-32016` | 호출 주체를 본다 |
    /// | 이 플랫폼에서 안 된다 | `-32015` | 플랫폼을 본다 |
    /// | 이 바이너리에 안 들어 있다 | `-32017` | 조합(헤드리스/release)을 본다 |
    /// | 이름이 틀렸다 | `-32601` | 이름을 고친다 |
    ///
    /// 앞의 둘은 [ADR-0163](../../../docs/adr/0163-a-registered-name-answers-who-not-whether.md)
    /// 과 [ADR-0154](../../../docs/adr/0154-a-platform-gated-dispatch-arm-answers-why-not-what.md)
    /// 가 결정했고, 셋째가 이 함수의 마지막 갈래다.
    ///
    /// ## 셋째 갈래의 술어가 왜 [`is_registered_name`] 인가
    ///
    /// [`method_meta`] 로 물으면 안 된다. 그 함수는 **런타임 등록 plugin prefix** 까지
    /// 해소하므로 설치된 plugin 의 이름과 그 아래 오타까지 `Some` 을 준다 — 그것으로
    /// 갈래를 타면 plugin 으로 갈 호출이 host 의 답을 받는다(실측 근거는
    /// [`is_registered_name`] 에 있다).
    ///
    /// [`is_registered_name`]: crate::method_meta::is_registered_name
    /// [`method_meta`]: crate::method_meta::method_meta
    pub fn unrouted_for_external_caller(id: serde_json::Value, method: &str) -> Self {
        match crate::method_meta::method_meta(method) {
            Some(m) if m.plugin_only => Self::error(
                id,
                -32016,
                format!(
                    "method '{method}' is plugin-only: only the plugin host-call path \
                     dispatches it, so CLI and network IPC callers have no entry point"
                ),
            ),
            _ if crate::method_meta::is_registered_name(method) => Self::error(
                id,
                -32017,
                format!(
                    "method '{method}' is registered but this binary has no dispatch \
                     arm for it: it is gated out of this build combination \
                     (headless / release)"
                ),
            ),
            _ => Self::method_not_found(id, method),
        }
    }

    pub fn invalid_params(id: serde_json::Value, msg: impl Into<String>) -> Self {
        Self::error(id, -32602, msg)
    }

    pub fn internal_error(id: serde_json::Value, msg: impl Into<String>) -> Self {
        Self::error(id, -32603, msg)
    }

    /// `error` 와 동일하되 `error.data` 도 함께 싣는다 — 호출자가 에러 메시지
    /// 문자열 파싱 없이 구조화된 부가정보(예: 참조 중인 task id 목록)를 받을 때.
    pub fn error_with_data(
        id: serde_json::Value,
        code: i32,
        message: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: Some(data),
            }),
            id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "workspace.list".into(),
            params: serde_json::json!({}),
            id: Some(serde_json::json!(1)),
            session_token: None,
            response_timeout_ms: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.method, "workspace.list");
        assert_eq!(parsed.jsonrpc, "2.0");
        assert!(parsed.session_token.is_none());
    }

    #[test]
    fn request_session_token_roundtrip() {
        // 토큰이 None 이면 wire 에 안 나가야 한다(공간/노이즈 절감).
        let req_none = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "x.y".into(),
            params: serde_json::json!({}),
            id: Some(serde_json::json!(1)),
            session_token: None,
            response_timeout_ms: None,
        };
        let json_none = serde_json::to_string(&req_none).unwrap();
        assert!(!json_none.contains("session_token"));

        // Some(_) 면 직렬화 + 역직렬화 보존.
        let token = "a".repeat(64);
        let req_some = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "x.y".into(),
            params: serde_json::json!({}),
            id: Some(serde_json::json!(1)),
            session_token: Some(token.clone()),
            response_timeout_ms: None,
        };
        let json_some = serde_json::to_string(&req_some).unwrap();
        assert!(json_some.contains("session_token"));
        let parsed: JsonRpcRequest = serde_json::from_str(&json_some).unwrap();
        assert_eq!(parsed.session_token.as_deref(), Some(token.as_str()));
    }

    /// 표에 `plugin_only` 로 등재된 이름은 "없다" 가 아니라 "부를 수 있는 주체가
    /// 다르다" 로 답한다. 두 답은 호출자가 **다음에 할 일**이 다르다 — `-32601` 은
    /// 이름을 고치게 하고, `-32016` 은 호출 주체를 보게 한다.
    #[test]
    fn a_plugin_only_method_is_not_answered_as_missing() {
        let resp =
            JsonRpcResponse::unrouted_for_external_caller(serde_json::json!(1), "banner.open");
        let err = resp.error.expect("에러여야 한다");
        assert_eq!(err.code, -32016, "plugin-only 인데 -32601 로 답했다");
        assert!(
            err.message.contains("plugin-only"),
            "사유가 메시지에 없다: {}",
            err.message
        );
    }

    /// 등재되지 않은 이름은 그대로 `-32601` 이다 — 이 갈래가 무너지면 오타가
    /// "주체가 다르다" 로 보고돼 호출자가 영영 못 고친다.
    #[test]
    fn an_unregistered_name_is_still_method_not_found() {
        let resp = JsonRpcResponse::unrouted_for_external_caller(
            serde_json::json!(1),
            "no.such.method.exists",
        );
        assert_eq!(resp.error.expect("에러여야 한다").code, -32601);
    }

    /// 등재됐지만 `plugin_only` 가 아닌 이름이 여기까지 왔다면 **이 조합에 arm 이 없는
    /// 것**이고, 그렇게 답한다.
    ///
    /// 이 자리는 원래 `-32601` 이었다. 그 근거는 "안 닿는 것은 다른 이유(플랫폼·조합
    /// 게이트)이고 답도 그 층이 낸다" 였는데, 실행해 보면 **그 층은 답하지 않는다** —
    /// 조합 게이트는 `match` 팔을 통째로 없애므로 호출은 `_` 로 떨어져 바로 여기 온다.
    /// 그래서 오타와 구분이 안 됐다(실측 2026-09-05: 헤드리스에서 `window.creat` 와
    /// `window.create` 의 응답이 바이트 단위로 같았다).
    #[test]
    fn a_registered_name_with_no_arm_here_says_so() {
        let resp =
            JsonRpcResponse::unrouted_for_external_caller(serde_json::json!(1), "system.info");
        let err = resp.error.expect("에러여야 한다");
        assert_eq!(err.code, -32017, "등재된 이름인데 -32601 로 답했다");
        assert!(
            err.message.contains("no dispatch arm"),
            "사유가 메시지에 없다: {}",
            err.message
        );
    }

    /// 오타와 등재된 이름이 **다른 코드**로 갈린다.
    ///
    /// 이 축의 결함이 정확히 이 구분의 부재였다. 한 이름만 보면 어느 쪽이 틀렸는지 알 수
    /// 없으므로 **짝으로** 본다 — 한 글자만 다른 두 이름을 같은 함수에 넣는다.
    #[test]
    fn a_typo_and_a_registered_name_do_not_get_the_same_answer() {
        let real =
            JsonRpcResponse::unrouted_for_external_caller(serde_json::json!(1), "workspace.create");
        let typo =
            JsonRpcResponse::unrouted_for_external_caller(serde_json::json!(1), "workspace.creat");
        let real = real.error.expect("에러여야 한다");
        let typo = typo.error.expect("에러여야 한다");
        assert_eq!(real.code, -32017);
        assert_eq!(typo.code, -32601);
        assert_ne!(
            real.code, typo.code,
            "오타와 등재된 이름이 같은 답을 받는다 — 호출자가 무엇을 고쳐야 할지 모른다"
        );
    }

    #[test]
    fn response_success() {
        let resp = JsonRpcResponse::success(serde_json::json!(1), serde_json::json!({"ok": true}));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert!(parsed.result.is_some());
    }

    #[test]
    fn response_error() {
        let resp = JsonRpcResponse::error(serde_json::json!(1), -32601, "Method not found");
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32601);
    }

    #[test]
    fn response_method_not_found() {
        let resp = JsonRpcResponse::method_not_found(serde_json::json!(1), "foo.bar");
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().code, -32601);
        assert!(resp.error.as_ref().unwrap().message.contains("foo.bar"));
    }

    #[test]
    fn response_roundtrip() {
        let resp = JsonRpcResponse::success(serde_json::json!(42), serde_json::json!({"count": 5}));
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: JsonRpcResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, serde_json::json!(42));
        assert_eq!(parsed.result.unwrap()["count"], 5);
    }
}
