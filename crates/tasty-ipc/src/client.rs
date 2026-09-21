//! 클라이언트 측 IPC 연결 — 실행 중인 tasty 인스턴스에 붙는 쪽.
//!
//! 서버 측([`crate::server`] / [`crate::session`])과 wire 프레이밍
//! ([`crate::protocol`] / [`crate::stream`])이 이 크레이트에 있으므로,
//! 그 짝인 클라이언트 연결도 같은 곳에 둔다. CLI 와 본체 GUI 가 함께 쓴다.
//!
//! - [`IpcConnection`] — BufReader 를 유지하는 JSON-RPC request-response 연결
//! - [`StreamConnection`] — attach/bulk 스트림 연결

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use anyhow::Result;

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// 호스트가 JSON-RPC 오류로 답했다 — **코드를 데이터로 들고 있는** 실패.
///
/// 이전에는 `anyhow::bail!("Error ({code}): {message}")` 로 곧장 문자열이 됐고, 그래서
/// 호출자가 코드를 쓰려면 자기가 만든 문장을 자기가 다시 파싱해야 했다. `message` 는
/// 답한 쪽(호스트 또는 plugin)이 만든 산문이라 **로케일을 탈 수 있는** 반면 `code` 는
/// 프로토콜 값이라 안 탄다 — 그 둘을 갈라 두는 것이 이 타입의 전부다.
///
/// `Display` 는 종전 문자열 그대로다. 기존 호출자의 출력은 한 글자도 바뀌지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonRpcCallError {
    pub code: i32,
    pub message: String,
}

impl std::fmt::Display for JsonRpcCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error ({}): {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcCallError {}

/// 상대가 그 계약을 선언하지 않았다 — **요청은 아직 안 나갔다.**
///
/// 이 타입이 [`JsonRpcCallError`] 와 갈라져 있는 이유가 그 한 줄이다. 저쪽은 호스트가
/// 답한 실패라 "무엇이 일어났는지" 가 이미 정해졌고, 이쪽은 **아무것도 일어나지
/// 않았음**을 뜻한다. 부수효과가 남는 메서드에서는 그 차이가 재시도 판단을 통째로
/// 가른다 — 이 오류를 받은 호출자는 그대로 다시 걸 수 있다(다만 같은 서버라면 또 같은
/// 답이다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedCapability {
    pub name: String,
    pub required: u32,
    /// 서버가 그 이름을 **다른 판으로** 선언했으면 그 값. 아예 없으면 `None`.
    pub found: Option<u32>,
}

impl std::fmt::Display for UnsupportedCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.found {
            Some(v) => write!(
                f,
                "the tasty instance declares '{}' at version {v}, but version {} is required; \
                 nothing was sent",
                self.name, self.required
            ),
            None => write!(
                f,
                "the tasty instance does not declare '{}' (required version {}); nothing was sent",
                self.name, self.required
            ),
        }
    }
}

impl std::error::Error for UnsupportedCapability {}

/// 그 메서드는 멱등 키 계약 **밖**이다 — **요청은 아직 안 나갔다.**
///
/// [`UnsupportedCapability`] 와 같은 성질의 거절이다. 저쪽은 "서버가 키를 못 읽는다",
/// 이쪽은 "서버가 키를 읽어도 **이 메서드에서는** 안 지킨다" 다. 어느 쪽이든 키를 실어
/// 보내면 호출자는 계약이 걸린 줄 알고 재시도하고, 그 재시도는 두 번째 실행이 된다.
/// 판정은 [`crate::method_meta::key_contract`] 가 하고, 지금 이 값을 내는 것은 plugin
/// namespace forward 다(ADR-0361).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyOutsideContract {
    pub method: String,
}

impl std::fmt::Display for KeyOutsideContract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "'{}' is outside the idempotency-key contract — the host does not replay it for a \
             key, so a retry runs it again; nothing was sent",
            self.method
        )
    }
}

impl std::error::Error for KeyOutsideContract {}

/// 키를 실어 보내기 **전에** 그 메서드가 계약 밖이라고 선언됐는지 본다. 연결이 필요 없다.
fn refuse_key_outside_contract(method: &str) -> Result<()> {
    match crate::method_meta::key_contract(method) {
        crate::method_meta::KeyContract::Outside => Err(KeyOutsideContract {
            method: method.to_string(),
        }
        .into()),
        crate::method_meta::KeyContract::Undeclared => Ok(()),
    }
}

/// `system.info` 응답에서 capability 이름 → 판 을 뽑는다.
///
/// 키가 아예 없는 구 서버는 **빈 map** 이다. 그것이 "아무것도 선언하지 않았다" 의 옳은
/// 표현이고, 그래서 이 함수의 실패 갈래는 없다 — 모양이 어긋난 항목은 조용히 빠진다.
/// 어긋난 항목을 오류로 올리면 서버가 나중에 더한 모양 하나가 **기존에 되던 조회까지**
/// 깨뜨린다(이 목록의 요점이 추가는 안전하다는 것이었다).
pub fn parse_capabilities(info: &serde_json::Value) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    let Some(items) = info.get("capabilities").and_then(|v| v.as_array()) else {
        return out;
    };
    for item in items {
        if let (Some(name), Some(version)) = (
            item.get("name").and_then(|v| v.as_str()),
            item.get("version").and_then(|v| v.as_u64()),
        ) {
            out.insert(name.to_string(), version as u32);
        }
    }
    out
}

pub mod stream;

pub use stream::StreamConnection;

/// A reusable IPC connection that keeps a single BufReader across multiple requests.
pub struct IpcConnection {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    /// 한 번 물어 둔 서버의 선언. 연결 수명 동안 안 바뀐다 — 그 값을 정하는 것은
    /// 상대 프로세스의 빌드이고, 연결이 살아 있는 한 그 프로세스도 그대로다.
    capabilities: Option<BTreeMap<String, u32>>,
}

impl IpcConnection {
    /// 읽기 타임아웃은 걸지 않는다 — 프로토콜에 **의도적으로 무한 대기하는 호출**이
    /// 있기 때문이다(`tasty agent task-await --timeout-ms 0`, 사용자 응답을 기다리는
    /// approval 등). 한 값으로 자르면 그 호출들이 정상 동작 중에 끊긴다. 응답 없는
    /// 상대를 감지하는 몫은 [`IpcConnection::send`] 의 EOF 판정이 진다.
    ///
    /// 기다림에 상한을 두려는 호출자는 **소켓이 아니라 요청 봉투에 싣는다**
    /// ([`crate::protocol::JsonRpcRequest::response_timeout_ms`]). 서버가 그 시간에 응답
    /// 통로를 놓고 `-32061`(결과 불명)로 답하므로, 봉투마다 정해지는 그 상한은
    /// 무한 대기하는 호출을 같이 자르지 않는다 — 소켓 한 값으로는 그 구분이 안 된다는
    /// 것이 위 문단의 이유였다. 이 연결은 그 필드를 스스로 채우지 않는다 — 싣는 것은 요청을
    /// 만드는 호출자다(CLI 는 루트 플래그 `--response-timeout-ms` 로 단발 요청에만 싣는다).
    /// 안 실은 요청에는 EOF 판정이 여전히 유일한 감지 수단이다.
    pub fn new(stream: TcpStream) -> Result<Self> {
        // Nagle 해제 — 요청 한 줄을 보내고 응답을 기다리는 순수 request-response 라
        // 지연시켜 합칠 뒷 데이터가 애초에 없다. Nagle 이 켜져 있으면 요청 줄이 두
        // 세그먼트로 쪼개지는 순간(`writeln!` 은 본문과 개행을 나눠 쓴다) 뒷조각이
        // 상대의 delayed ACK(~40ms)까지 붙잡혀 **모든 CLI 명령**에 그만큼이 얹힌다.
        // 실패해도 연결 자체는 유효하므로 에러로 올리지 않는다.
        if let Err(e) = stream.set_nodelay(true) {
            tracing::warn!("IPC 연결: TCP_NODELAY 설정 실패(지연 증가 가능): {e}");
        }
        let writer = stream.try_clone()?;
        let reader = BufReader::new(stream);
        Ok(Self {
            writer,
            reader,
            capabilities: None,
        })
    }

    /// Send a JSON-RPC request and read the response.
    pub fn send(&mut self, request: &JsonRpcRequest) -> Result<serde_json::Value> {
        let json = serde_json::to_string(request)?;
        // 개행까지 한 버퍼에 담아 **한 번의 write** 로 보낸다 — `writeln!` 은 본문과
        // 개행을 각각 write 해 세그먼트를 쪼갠다(위 `TCP_NODELAY` 주석 참고).
        let mut line = json.into_bytes();
        line.push(b'\n');
        self.writer.write_all(&line)?;
        self.writer.flush()?;

        loop {
            let mut line = String::new();
            // EOF(`read_line` 이 0 을 반환)를 반드시 먼저 걸러낸다. EOF 는 이후로
            // 영원히 0 을 반환하므로, 빈 줄 skip 으로 흘려보내면 이 루프가 코어
            // 하나를 100% 로 태우며 무한 스핀한다(커널 블록이 아니라 유저스페이스
            // 스핀이라 겉보기 hang 과 구분도 어렵다). 호스트가 종료 중이거나
            // 크래시/SIGKILL 로 죽으면 실제로 밟는 경로다.
            let n = self.reader.read_line(&mut line)?;
            if n == 0 {
                anyhow::bail!(
                    "tasty instance closed the connection without responding \
                     (host may be shutting down)"
                );
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let response: JsonRpcResponse = serde_json::from_str(trimmed)?;

            if let Some(error) = response.error {
                return Err(JsonRpcCallError {
                    code: error.code,
                    message: error.message,
                }
                .into());
            }

            return Ok(response.result.unwrap_or(serde_json::Value::Null));
        }
    }

    /// 서버가 선언한 기능 목록. 연결마다 **한 번만** 묻는다.
    ///
    /// `session_token` 은 부를 요청의 것을 그대로 쓴다 — 토큰이 있는 호출자가 토큰 없이
    /// 물으면 그 조회는 `Local` 로 판정돼, 실제로 요청을 보낼 주체와 **다른 주체의**
    /// 답을 보게 된다.
    pub fn capabilities(&mut self, session_token: Option<&str>) -> Result<&BTreeMap<String, u32>> {
        if self.capabilities.is_none() {
            let probe = JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                method: "system.info".to_string(),
                params: serde_json::Value::Null,
                id: Some(serde_json::Value::from(0)),
                session_token: session_token.map(str::to_string),
                response_timeout_ms: None,
                idempotency_key: None,
            };
            let info = self.send(&probe)?;
            self.capabilities = Some(parse_capabilities(&info));
        }
        Ok(self.capabilities.as_ref().expect("직전 분기가 채웠다"))
    }

    /// 그 계약을 상대가 아는지 **부수효과가 나기 전에** 판정한다.
    ///
    /// 판이 요구값 **이상**이면 통과다. 기능 목록의 판은 올라가는 방향으로만 움직이고
    /// 뜻이 좁아질 때만 올라가므로(그 규약은 [`crate::capability`] 에 있다), 더 높은
    /// 판은 "이 이름을 더 잘 안다" 는 뜻이다.
    pub fn require_capability(
        &mut self,
        name: &str,
        min_version: u32,
        session_token: Option<&str>,
    ) -> Result<()> {
        let found = self.capabilities(session_token)?.get(name).copied();
        match found {
            Some(v) if v >= min_version => Ok(()),
            other => Err(UnsupportedCapability {
                name: name.to_string(),
                required: min_version,
                found: other,
            }
            .into()),
        }
    }

    /// 멱등 키를 실어 보낸다 — 단, **보내기 전에** 상대가 그 계약을 아는지 묻는다.
    ///
    /// 그냥 필드만 실으면 구 서버는 키를 조용히 버리고 요청을 그대로 실행한다. 그때
    /// 호출자는 계약이 걸린 줄 알고 재시도하므로 **두 번째 효과**가 남는다 — 키를 실은
    /// 목적과 정확히 반대다. 그래서 이 함수의 값은 키를 싣는 것이 아니라 **못 싣는
    /// 상대에게 안 보내는 것**이다.
    ///
    /// 확인 자체는 `system.info` 라 부수효과가 없고 연결마다 한 번이다.
    ///
    /// **확인은 둘이다.** 먼저 그 메서드가 계약 밖이라고 **선언됐는지** 본다
    /// ([`crate::method_meta::key_contract`]) — plugin namespace forward 가 그렇다(ADR-0361).
    /// 그러면 연결을 쓰지도 않고 [`KeyOutsideContract`] 로 끝난다. 그다음 서버가 필드를
    /// 읽는지 capability 로 묻는다.
    ///
    /// ★ **두 확인이 답하는 것은 "선언된 계약 밖이 아니고, 서버가 이 필드를 읽는다" 까지다.**
    /// "이 메서드가 그 계약에 걸린다" 는 여전히 아무도 선언하지 않는다 — 호스트의 보존소가
    /// engine 라우터 한 자리에 있어 그 앞에서 끝나는 **App 층** 메서드는 키를 실어도 그냥
    /// 실행되는데, 그 목록은 아직 표에 안 적혀 있다(`KeyContract::Undeclared`). 그래서 그
    /// 메서드들은 이 함수를 통과하고, **거기서는 재시도가 두 번째 효과를 남긴다.**
    ///
    /// 호출자가 지금 쓸 수 있는 유일한 사후 표지는 응답의
    /// [`JsonRpcResponse::idempotent_replay`] 다. 그 표지는 **한 방향으로만** 답한다 —
    /// 붙어 있으면 계약이 걸린 것이지만, 안 붙은 것은 계약 밖이라는 뜻이 아니다.
    /// `-32063`·`-32064` 는 에러라 표지가 `false` 인데 그 코드 자신이 계약이 개입했다는
    /// 증거이고, 보존 범위 밖으로 밀려난 키도 표지 없이 다시 실행된다. 규칙을 문자 그대로
    /// 적용한 호출자는 `-32063` 을 보고 "이 메서드는 계약 밖" 으로 읽어 **키 없이
    /// 재전송**하는데, 그것은 계약이 막아 준 두 번째 효과를 스스로 실행하는 일이다.
    /// 그래서 "표지 없는 재시도 = 계약 밖" 은 **성공 응답들 사이에서, 선언된 보존 범위
    /// 안에서만** 읽어야 한다. 메서드 단위 선언은 그 세 자리가 배선된 뒤에 붙일 값이다.
    pub fn send_idempotent(
        &mut self,
        request: &JsonRpcRequest,
        key: &str,
    ) -> Result<serde_json::Value> {
        refuse_key_outside_contract(&request.method)?;
        self.require_capability(
            IDEMPOTENCY_CAPABILITY,
            IDEMPOTENCY_CAPABILITY_VERSION,
            request.session_token.as_deref(),
        )?;
        let keyed = JsonRpcRequest {
            idempotency_key: Some(key.to_string()),
            ..request.clone()
        };
        self.send(&keyed)
    }
}

/// [`crate::protocol::JsonRpcRequest::idempotency_key`] 를 서버가 읽는다는 선언의 이름.
pub const IDEMPOTENCY_CAPABILITY: &str = "ipc.idempotency-key";
/// 이 client 가 요구하는 최소 판.
pub const IDEMPOTENCY_CAPABILITY_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    /// capability 키가 아예 없는 **구 서버**는 빈 선언이다. "모른다" 를 "안다" 로 읽으면
    /// 그 서버에 키를 실어 보내게 되고, 그것이 이 축의 결함이다.
    #[test]
    fn an_old_server_without_the_key_declares_nothing() {
        let info = serde_json::json!({ "version": "0.6.0", "scope": "engine" });
        assert!(parse_capabilities(&info).is_empty());
    }

    /// plugin namespace 이름은 client 프로세스에서 표가 모르는 이름이다 — 계약 밖으로 읽혀
    /// **보내기 전에** 거절된다. 호스트 메서드는 선언이 없어 통과한다(서버에 묻는 다음 단계로).
    #[test]
    fn a_key_is_refused_before_sending_for_a_name_the_host_table_does_not_own() {
        let refused = refuse_key_outside_contract("markdown.recent").unwrap_err();
        let typed = refused
            .downcast_ref::<KeyOutsideContract>()
            .expect("타입으로 갈라져야 호출자가 '안 나갔다' 를 안다");
        assert_eq!(typed.method, "markdown.recent");
        assert!(refused.to_string().contains("nothing was sent"));

        refuse_key_outside_contract("workspace.create").expect("호스트 메서드는 다음 단계로 간다");
    }

    /// 거절은 **연결을 한 바이트도 쓰지 않는다** — 상대가 무엇이든 요청이 안 나간다.
    #[test]
    fn a_refused_key_never_touches_the_connection() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stream = TcpStream::connect(addr).unwrap();
        // 거절이 사라져 요청이 나가면 서버가 답하지 않으므로 client 가 영영 선다 — 시험이
        // 멈추지 않고 실패하도록 읽기에 시한을 둔다.
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut conn = IpcConnection::new(stream).unwrap();
        let (mut server, _) = listener.accept().unwrap();

        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "markdown.recent".to_string(),
            params: serde_json::json!({}),
            id: Some(serde_json::Value::from(1)),
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let err = conn.send_idempotent(&req, "k-1").unwrap_err();
        assert!(err.downcast_ref::<KeyOutsideContract>().is_some(), "{err}");

        drop(conn);
        let mut got = Vec::new();
        server.read_to_end(&mut got).unwrap();
        assert!(
            got.is_empty(),
            "거절됐는데 무언가 나갔다: {}",
            String::from_utf8_lossy(&got)
        );
    }

    /// 모양이 어긋난 항목은 조용히 빠지고 나머지는 산다 — 한 항목이 전체 조회를
    /// 깨뜨리면 "추가는 안전하다" 가 거짓이 된다.
    #[test]
    fn a_malformed_entry_does_not_take_the_others_down() {
        let info = serde_json::json!({
            "capabilities": [
                { "name": "ipc.capabilities", "version": 1 },
                { "name": "no.version" },
                { "version": 2 },
                "not an object",
                { "name": "ipc.idempotency-key", "version": 3 },
            ]
        });
        let caps = parse_capabilities(&info);
        assert_eq!(caps.get("ipc.capabilities"), Some(&1));
        assert_eq!(caps.get("ipc.idempotency-key"), Some(&3));
        assert_eq!(caps.len(), 2);
    }

    /// 더 높은 판은 통과, 낮은 판과 부재는 거절 — 그리고 **부재와 낮은 판이 다른
    /// 값으로 보고된다**(호출자가 서버를 올릴지 요구를 내릴지 가른다).
    #[test]
    fn a_higher_version_satisfies_and_the_two_failures_are_told_apart() {
        let caps: BTreeMap<String, u32> = [("ipc.idempotency-key".to_string(), 2u32)]
            .into_iter()
            .collect();
        assert!(
            caps.get("ipc.idempotency-key")
                .copied()
                .is_some_and(|v| v >= 1)
        );

        let missing = UnsupportedCapability {
            name: "x".into(),
            required: 1,
            found: None,
        };
        let older = UnsupportedCapability {
            name: "x".into(),
            required: 2,
            found: Some(1),
        };
        assert_ne!(missing, older);
        assert!(missing.to_string().contains("does not declare"));
        assert!(older.to_string().contains("version 1"));
        // 어느 쪽이든 "아직 안 보냈다" 를 말한다 — 재시도 판단이 거기 달렸다.
        assert!(missing.to_string().contains("nothing was sent"));
        assert!(older.to_string().contains("nothing was sent"));
    }
}
