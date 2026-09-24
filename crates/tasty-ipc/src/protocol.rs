use serde::{Deserialize, Serialize};

// 전송·대기·멱등 키 처리의 오류 코드. 실행 여부는 코드마다 다르므로 각 계약을 따른다.

/// 요청 한 줄이 서버의 줄 상한(`MAX_REQUEST_LINE_BYTES`)을 넘었다. 넘긴 줄의 나머지가
/// 소켓에 남아 있으므로 그 연결은 이 응답 **뒤에 닫힌다** — 계속 읽으면 한 줄이 여러
/// 요청으로 쪼개져 들어가 상한이 다시 없는 것이 된다. 상한 값은 응답 문구에 실린다.
pub const ERR_REQUEST_LINE_TOO_LONG: i32 = -32060;

/// 요청을 시작한 뒤 응답 대기 상한이 지났다. 실행을 취소하지 않으며 최종 결과는 알 수 없다.
/// 시작 전에 만료된 요청은 ERR_EXPIRED_BEFORE_RUN으로 구분한다.
pub const ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN: i32 = -32061;

/// 동시 연결 한도에서 거절돼 요청은 읽거나 실행하지 않았다. 연결을 다시 시도할 수 있다.
/// 응답 쓰기는 한 번만 시도하므로 client는 EOF를 받을 수도 있다.
/// 스트림 client는 JSON 줄 대신 프레임을 기대해 unknown stream tag로 볼 수 있다.
pub const ERR_CONNECTION_LIMIT_REACHED: i32 = -32062;

/// 같은 멱등 키에 다른 메서드·params가 전달돼 이번 요청을 실행하지 않았다. 키 재사용을 확인한다.
pub const ERR_IDEMPOTENCY_KEY_CONFLICT: i32 = -32063;

/// 요청은 실행됐지만 응답이 보존 상한을 넘어 저장되지 않았다.
/// 키는 남겨 중복 실행을 막는다. 결과는 재전송 대신 상태 조회로 확인한다.
pub const ERR_IDEMPOTENT_RESULT_DISCARDED: i32 = -32064;

/// 대기 중인 요청 바이트 합이 한도에 도달해 큐에 넣지 않았다. 실행되지 않았고 연결은 유지된다.
/// 요청 한 줄의 크기 제한과는 별개이며, 잠시 뒤 다시 시도할 수 있다.
pub const ERR_COMMAND_QUEUE_FULL: i32 = -32065;

/// 연결이 열린 뒤 첫 요청 줄이 정해진 시간 안에 오지 않아 호스트가 연결을 닫는다.
/// **요청이 실행되지 않았다** — 줄을 받지 못했으므로 실행할 것이 없다.
///
/// 이 한도는 **첫 줄에만** 걸린다. 한 번이라도 요청을 보낸 연결은 요청 사이에 얼마나
/// 쉬어도 닫히지 않는다(오래 붙어 있는 client 의 호환). 이 답은 최선 노력이다 — 쓰기에도
/// 시간 상한이 있어 client 는 이 줄 대신 EOF 를 볼 수 있다. 근거는 docs/architecture/ipc-server.md#첫-요청과-응답-쓰기.
pub const ERR_FIRST_LINE_IDLE: i32 = -32066;

/// 큐에서 기한이 지나 실행되지 않았고 이후에도 실행하지 않는다. 연결은 유지한다.
/// CommandLifecycle의 시작/철회 판정으로 시작 뒤 결과 불명(-32061)과 구분한다.
pub const ERR_EXPIRED_BEFORE_RUN: i32 = -32067;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: Option<serde_json::Value>,
    /// 자식 호출자의 64자 hex 세션 토큰. 호스트가 SessionStore로 검증한다.
    /// 잘못됐거나 만료·철회된 토큰을 Local로 대신 처리하지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    /// 요청별 응답 대기 상한(밀리초). 생략 또는 0은 무한 대기다.
    /// 시작 전 만료는 -32067, 시작 후 결과 불명은 -32061로 답한다.
    /// 구 서버는 필드를 무시할 수 있어 ipc.response-timeout capability를 먼저 확인한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_timeout_ms: Option<u64>,
    /// 호출자가 같은 요청의 재시도를 식별하는 키. JSON-RPC id와는 용도가 다르다.
    /// 키 길이는 모든 라우팅 경로에서 검사한다. 호스트의 Mutate 메서드만 응답을 보존하며
    /// Read/Idempotent는 키를 보존하지 않는다. 플러그인 고유 메서드는 호스트 키 계약 밖이다.
    ///
    /// client는 key_contract와 서버의 ipc.idempotency-key 기능 버전을 전송 전에 확인해야 한다.
    /// 보관된 응답의 재사용에는 idempotent_replay가 붙는다. 키 충돌(-32063)과 응답 미보존
    /// (-32064)은 별도 결과다. system.info가 선언한 보존 시간·항목 수·재시작 범위를
    /// 벗어난 키는 새 요청처럼 다시 실행될 수 있다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: serde_json::Value,
    /// 보관된 응답을 재사용했다는 표지. false면 필드를 보내지 않아 기존 형식을 유지한다.
    #[serde(default, skip_serializing_if = "is_false")]
    pub idempotent_replay: bool,
}

/// `skip_serializing_if` 전용 — 거짓이면 키 자체를 빼서 종전 바이트를 유지한다.
fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            idempotent_replay: false,
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
            idempotent_replay: false,
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
    /// [IPC 오류 구분](../../../docs/adr/0004-ipc-discovery-and-errors.md)에 따라
    /// 셋째 제한을 이 함수의 마지막 갈래에서 판정한다.
    ///
    /// 등록 여부는 is_registered_name으로 확인한다. method_meta의 namespace fallback을
    /// 사용하면 플러그인 고유 메서드나 그 이름의 오타까지 호스트 메서드로 오인한다.
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
            idempotent_replay: false,
        }
    }

    /// 보관한 응답의 ID를 현재 요청 ID로 바꿔 JSON-RPC 응답 대응을 유지한다.
    pub fn replayed_for(&self, id: serde_json::Value) -> Self {
        Self {
            id,
            idempotent_replay: true,
            ..self.clone()
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
            idempotency_key: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.method, "workspace.list");
        assert_eq!(parsed.jsonrpc, "2.0");
        assert!(parsed.session_token.is_none());
    }

    #[test]
    fn request_session_token_roundtrip() {
        let req_none = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "x.y".into(),
            params: serde_json::json!({}),
            id: Some(serde_json::json!(1)),
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let json_none = serde_json::to_string(&req_none).unwrap();
        assert!(!json_none.contains("session_token"));

        let token = "a".repeat(64);
        let req_some = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "x.y".into(),
            params: serde_json::json!({}),
            id: Some(serde_json::json!(1)),
            session_token: Some(token.clone()),
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let json_some = serde_json::to_string(&req_some).unwrap();
        assert!(json_some.contains("session_token"));
        let parsed: JsonRpcRequest = serde_json::from_str(&json_some).unwrap();
        assert_eq!(parsed.session_token.as_deref(), Some(token.as_str()));
    }

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

    #[test]
    fn an_unregistered_name_is_still_method_not_found() {
        let resp = JsonRpcResponse::unrouted_for_external_caller(
            serde_json::json!(1),
            "no.such.method.exists",
        );
        assert_eq!(resp.error.expect("에러여야 한다").code, -32601);
    }

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
