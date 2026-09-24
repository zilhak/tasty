//! 단일 HTTP 포트에서 요청별 스레드로 웹훅을 처리한다.
//! 남용 차단·입력 크기·매칭·인증을 확인하고 고정 ACK로 응답한 뒤 시퀀스를 실행한다.
//! ACK는 실행 결과를 기다리지 않는다. 메인 루프 전달과 깨우기는 HostIpcInjector가 담당한다.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::OnceLock;
use std::thread;

use serde_json::Value;

use super::WebhookInitReport;
use super::abuse;
use super::ack::{AckStatus, build_ack};
use super::registry::{self, MatchResult};
use crate::hook_handler::{SequenceOrigin, SubstitutionContext, execute_sequence};
use tasty_ipc::host_call::HostIpcInjector;

/// 설정 포트에 bind한다. 실패하면 다른 포트로 재시도하지 않고 보고서를 반환한다.
pub fn init(injector: HostIpcInjector, bind_addr: &str, port: u16) -> WebhookInitReport {
    registry::set_runtime(injector, bind_addr, Some(port));
    if registry::is_bound() {
        tracing::debug!("webhook listener already bound; skip re-init");
        return WebhookInitReport::Bound;
    }
    let addr = format!("{bind_addr}:{port}");
    match tiny_http::Server::http(addr.as_str()) {
        Ok(server) => on_bind_success(server, &addr),
        Err(e) => on_bind_failed(e.to_string(), &addr, port),
    }
}

fn on_bind_success(server: tiny_http::Server, addr: &str) -> WebhookInitReport {
    registry::mark_bound();
    tracing::info!("webhook listener bound on {addr}");
    spawn_accept_thread(server);
    WebhookInitReport::Bound
}

fn on_bind_failed(error: String, addr: &str, port: u16) -> WebhookInitReport {
    tracing::warn!(
        "webhook listener bind {addr} failed: {error} — set a free port and check firewall (no auto-fallback)"
    );
    WebhookInitReport::BindFailed {
        #[cfg(feature = "gui")]
        port,
        #[cfg(feature = "gui")]
        error,
    }
}

/// accept 스레드 생성에 실패하면 경고만 남긴다. bound 표시는 되돌리지 않아 자동 재시도하지 않는다.
fn spawn_accept_thread(server: tiny_http::Server) {
    if let Err(e) = thread::Builder::new()
        .name("webhook-accept".into())
        .spawn(move || accept_loop(server))
    {
        tracing::warn!("webhook accept thread spawn failed: {e}");
    }
}

fn accept_loop(server: tiny_http::Server) {
    for request in server.incoming_requests() {
        thread::spawn(move || handle_request(request));
    }
}

fn handle_request(request: tiny_http::Request) {
    // 포트를 키에 넣으면 요청마다 다른 포트로 실패 집계를 피할 수 있다.
    // IP 기준이므로 NAT 뒤의 발신자들은 카운터를 공유한다.
    let source = request.remote_addr().map(|a| a.ip().to_string());

    let Some(mut request) = reject_if_abusive(request, source.as_deref()) else {
        return;
    };

    let url = request.url();
    let method = request.method();

    let (path_raw, query_str) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url.as_str(), ""),
    };
    let path = path_raw.trim_start_matches('/').to_string();
    let query = parse_query(query_str);

    let headers = request.headers();

    let (ack, exec, body) = match request.read_json_body(max_body_bytes()) {
        Ok(body) => {
            let (ack, exec) = resolve_ack(&path, &method, &headers, &query, &body);
            (ack, exec, body)
        }
        Err(BodyTooLarge) => (AckStatus::PayloadTooLarge, None, Value::Null),
    };

    // 정책에서 정한 실패만 IP별 카운터에 반영한다.
    if abuse::counts_as_failure(ack)
        && let Some(src) = source.as_deref()
    {
        abuse::record_failure(src);
    }

    request.respond(ack);

    if let Some((calls, Some(injector))) = exec {
        let ctx = SubstitutionContext {
            body,
            headers,
            query,
        };
        execute_sequence(SequenceOrigin::Webhook, &injector, &calls, &ctx);
    }
}

/// JSON 입력 body 가 요청당 상한을 넘었다 — 파서의 읽기를 멈췄다는 표시.
struct BodyTooLarge;

/// 요청 하나에서 JSON 처리에 허용하는 body 의 상한(바이트).
///
/// 이 값은 JSON 입력에 적용된다. 동시에 들어오는 요청 수는 제한하지 않으며,
/// 프로세스 전체 메모리 상한이 아니다. 리스너는 요청마다 스레드를 띄운다.
/// 초과 요청의 잔여 body 는 읽지 않고 연결을 닫는다([`Screened::respond`]).
/// ([ADR-0032](../../docs/adr/0032-webhook-admission.md)).
///
/// 기본 1 MiB. `TASTY_WEBHOOK_MAX_BODY_BYTES` 로 오버라이드한다(0·파싱 실패는 기본값).
fn max_body_bytes() -> usize {
    const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;
    static CACHED: OnceLock<usize> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("TASTY_WEBHOOK_MAX_BODY_BYTES")
            .ok()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_MAX_BODY_BYTES)
    })
}

/// 남용 차단을 통과해 JSON 입력을 읽을 수 있는 요청.
/// HTTP 라이브러리의 사전 body 버퍼링은 이 타입이 제어하지 않는다.
struct Screened(tiny_http::Request);

impl Screened {
    fn url(&self) -> String {
        self.0.url().to_string()
    }

    fn method(&self) -> String {
        self.0.method().to_string().to_ascii_uppercase()
    }

    /// 요청 헤더를 소문자 필드명 맵으로 수집.
    fn headers(&self) -> BTreeMap<String, String> {
        let mut headers = BTreeMap::new();
        for h in self.0.headers() {
            headers.insert(
                h.field.as_str().as_str().to_ascii_lowercase(),
                h.value.as_str().to_string(),
            );
        }
        headers
    }

    /// JSON 입력으로 limit+1바이트까지만 읽어 초과 여부를 구분한다.
    /// Content-Length가 이미 limit보다 크면 읽지 않고 거부한다.
    /// 읽기 실패·비-JSON은 Null, 상한 초과는 BodyTooLarge다.
    /// 남은 body를 HTTP 계층에서도 읽지 않게 닫는 처리는 respond가 맡는다.
    fn read_json_body(&mut self, limit: usize) -> Result<Value, BodyTooLarge> {
        if self
            .0
            .body_length()
            .is_some_and(|declared| declared > limit)
        {
            return Err(BodyTooLarge);
        }
        let mut body_bytes = Vec::new();
        let capped = limit.saturating_add(1) as u64;
        if let Err(e) = self.0.as_reader().take(capped).read_to_end(&mut body_bytes) {
            tracing::debug!("webhook body read failed: {e}");
        }
        // UTF-8 변환 실패로 바이트 수를 잃기 전에 초과를 판정한다.
        if body_bytes.len() > limit {
            return Err(BodyTooLarge);
        }
        let body_str = match String::from_utf8(body_bytes) {
            Ok(body) => body,
            Err(e) => {
                tracing::debug!("webhook body UTF-8 decode failed: {e}");
                return Ok(Value::Null);
            }
        };
        Ok(serde_json::from_str::<Value>(&body_str).unwrap_or(Value::Null))
    }

    /// 고정 ACK로 응답한다. 413에서는 남은 body를 읽지 않도록 연결을 닫는다.
    /// 저장소의 tiny_http 패치를 사용하며 기본 Drop의 잔여 body 읽기를 피한다.
    fn respond(self, ack: AckStatus) {
        let response = build_ack(ack);
        let result = if ack == AckStatus::PayloadTooLarge {
            self.0.respond_and_close(response)
        } else {
            self.0.respond(response)
        };
        if let Err(e) = result {
            tracing::debug!("webhook ack respond failed: {e}");
        }
    }
}

/// 차단된 출처면 body를 읽지 않고 429로 응답해 연결을 닫는다.
/// 통과한 요청만 Screened로 반환한다.
fn reject_if_abusive(request: tiny_http::Request, source: Option<&str>) -> Option<Screened> {
    if let Some(src) = source
        && abuse::is_source_blocked(src)
    {
        if let Err(e) = request.respond_and_close(build_ack(AckStatus::TooManyRequests)) {
            tracing::debug!("webhook 429 respond failed: {e}");
        }
        return None;
    }
    Some(Screened(request))
}

/// 실행 시퀀스와 선택적 injector. 초기화 전에는 injector가 없을 수 있다.
type PendingExec = (Vec<crate::hook_handler::IpcCall>, Option<HostIpcInjector>);

/// path/method 매칭 + (설정돼 있으면) 인증 검증까지 수행해 응답할 ACK 상태와,
/// 실행할 게 있으면 그 `(calls, injector)` 를 반환한다.
fn resolve_ack(
    path: &str,
    method: &str,
    headers: &BTreeMap<String, String>,
    query: &BTreeMap<String, String>,
    body: &Value,
) -> (AckStatus, Option<PendingExec>) {
    // 인증과 횟수 차감을 같은 락 안에서 처리해 거부된 요청은 횟수를 쓰지 않게 한다.
    match registry::match_request(path, method, |a| a.verify(headers, query, body)) {
        MatchResult::NotFound => (AckStatus::NotFound, None),
        MatchResult::MethodNotAllowed => (AckStatus::MethodNotAllowed, None),
        MatchResult::Expired => (AckStatus::Gone, None),
        MatchResult::Unauthorized => (AckStatus::Unauthorized, None),
        MatchResult::Matched { calls, injector } => (AckStatus::Received, Some((calls, injector))),
    }
}

/// key=value 쿼리 파싱. percent-decode는 하지 않는다.
fn parse_query(q: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for pair in q.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(k.to_string(), v.to_string());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_basic() {
        let m = parse_query("token=abc&x=1");
        assert_eq!(m.get("token"), Some(&"abc".to_string()));
        assert_eq!(m.get("x"), Some(&"1".to_string()));
    }

    // 인증에 실패한 요청이 등록 횟수를 소모해 소유자의 웹훅을 없애면 안 된다.
    #[test]
    fn unauthorized_does_not_consume_count() {
        use crate::webhook::auth::{AuthLocation, WebhookAuth};
        use crate::webhook::lifetime::{Lifetime, Limit, Persistence};
        use crate::webhook::registry;

        let id = registry::register(
            vec!["POST".to_string()],
            None,
            vec![],
            Lifetime {
                persistence: Persistence::Temporary,
                limit: Limit::CountLimit { remaining: 2 },
            },
            Some(WebhookAuth {
                location: AuthLocation::QueryKey { key: "t".into() },
                token: "right".into(),
            }),
        )
        .id;

        let mut wrong = BTreeMap::new();
        wrong.insert("t".to_string(), "nope".to_string());
        let (ack, exec) = resolve_ack(&id, "POST", &BTreeMap::new(), &wrong, &Value::Null);

        assert_eq!(ack, AckStatus::Unauthorized);
        assert!(exec.is_none(), "401 은 시퀀스를 넘기지 않는다");

        let (entry, _) = registry::info(&id).expect("401 로 등록이 사라지면 안 된다");
        assert_eq!(
            entry.lifetime.limit,
            Limit::CountLimit { remaining: 2 },
            "인증 실패는 등록 횟수를 차감하지 않아야 한다"
        );
    }

    #[test]
    fn parse_query_empty() {
        assert!(parse_query("").is_empty());
    }
}

#[cfg(test)]
#[path = "listener_body_tests.rs"]
mod body_tests;
