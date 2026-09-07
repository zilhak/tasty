//! 웹훅 리스너 싱글턴 라우터 — tiny_http 단일 포트 + std::thread accept.
//!
//! 기존 IPC 서버(`tcp_ipc_server.rs`)의 std::thread accept + 요청별 worker 패턴을
//! 미러링한다. 매칭된 요청은 (path) → 웹훅 → IpcSequence 로 라우팅되어
//! `HostIpcInjector` 로 메인루프에 주입된다(별도 waker 불필요 — injector 가 내장).
//!
//! ## 흐름 (단방향 불변식)
//! `요청 파싱 → 매칭 → build_ack 즉시 응답 → (별도) IpcSequence fire-and-forget`.
//! ACK 는 실행 전/무관하게 확정되며 실행 결과에 닿지 않는다.

use std::collections::BTreeMap;
use std::thread;

use serde_json::Value;

use super::WebhookInitReport;
use super::abuse;
use super::ack::{AckStatus, build_ack};
use super::registry::{self, MatchResult};
use crate::adapters::ipc::host_call::HostIpcInjector;
use crate::hook_handler::{SubstitutionContext, execute_sequence};

/// 리스너 init — runtime 주입 후 tiny_http 를 bind 하고 accept thread 를 띄운다.
///
/// 포트는 **설정값 only**(자동 폴백 bind 없음). bind 실패는 삼키지 않고 경고 —
/// 사용자가 포트/방화벽을 직접 조치한다(다중 bind 가드로 중복 호출 무해). 결과를
/// [`WebhookInitReport`] 로 반환해 caller 가 UI/로그로 노출한다.
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

/// bind 성공 시 registry 를 bound 로 표시하고 accept 스레드를 띄운다.
fn on_bind_success(server: tiny_http::Server, addr: &str) -> WebhookInitReport {
    registry::mark_bound();
    tracing::info!("webhook listener bound on {addr}");
    spawn_accept_thread(server);
    WebhookInitReport::Bound
}

/// bind 실패를 경고 로그로 남기고(자동 폴백 없음 — 사용자가 직접 조치) 보고서로 변환.
fn on_bind_failed(error: String, addr: &str, port: u16) -> WebhookInitReport {
    tracing::warn!(
        "webhook listener bind {addr} failed: {error} — set a free port and check firewall (no auto-fallback)"
    );
    WebhookInitReport::BindFailed { port, error }
}

/// accept 스레드 스폰. 실패해도 bind 자체는 이미 성공했으므로 경고만 남긴다
/// (재시도 없음 — 다중 bind 가드가 재호출을 무해하게 만들 뿐 자동 복구는 안 함).
fn spawn_accept_thread(server: tiny_http::Server) {
    if let Err(e) = thread::Builder::new()
        .name("webhook-accept".into())
        .spawn(move || accept_loop(server))
    {
        tracing::warn!("webhook accept thread spawn failed: {e}");
    }
}

/// accept 루프 — 요청별 worker thread 로 넘겨 IpcSequence dispatch 가 accept 를
/// 막지 않게 한다(tcp_ipc_server 패턴).
fn accept_loop(server: tiny_http::Server) {
    for request in server.incoming_requests() {
        thread::spawn(move || handle_request(request));
    }
}

/// 한 요청 처리: (남용차단) → 파싱 → 매칭 → ACK 응답 → fire-and-forget 실행.
fn handle_request(request: tiny_http::Request) {
    // 출처 IP(포트 제외 — 스캐너는 IP 를 재사용하며 포트만 바꾼다. 포트를 키에 넣으면
    // 한 발신자가 요청마다 새 키를 만들어 카운터를 우회한다. 대가는 NAT 뒤 공유 —
    // `docs/adr/0196-abuse-thresholds-and-source-key.md`).
    let source = request.remote_addr().map(|a| a.ip().to_string());

    let Some(mut request) = reject_if_abusive(request, source.as_deref()) else {
        return;
    };

    let url = request.url().to_string();
    let method = request.method().to_string().to_ascii_uppercase();

    let (path_raw, query_str) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url.as_str(), ""),
    };
    let path = path_raw.trim_start_matches('/').to_string();
    let query = parse_query(query_str);

    let headers = collect_headers(&request);
    let body = read_json_body(&mut request);

    let (ack, exec) = resolve_ack(&path, &method, &headers, &query, &body);

    // 매칭·인증 실패는 출처 실패로 집계(임계치 초과 시 다음 요청부터 쿨다운 429).
    // 무엇을 세는지는 `abuse::counts_as_failure` 가 정한다 — 무엇이 남용인가는
    // 남용차단의 정책이고, 여기 인라인 조건으로 두면 그 답이 두 곳에 생긴다.
    if abuse::counts_as_failure(ack)
        && let Some(src) = source.as_deref()
    {
        abuse::record_failure(src);
    }

    // 단방향: ACK 를 실행 전/무관하게 즉시 확정·응답.
    if let Err(e) = request.respond(build_ack(ack)) {
        tracing::debug!("webhook ack respond failed: {e}");
    }

    // fire-and-forget 실행 — 실행 결과는 응답 경로에 절대 닿지 않는다.
    if let Some((calls, Some(injector))) = exec {
        let ctx = SubstitutionContext {
            body,
            headers,
            query,
        };
        execute_sequence(&injector, &calls, &ctx);
    }
}

/// 남용 차단(쿨다운 중) 출처면 즉시 429 로 응답하고 `None`(요청 소비 완료,
/// 호출자는 더 진행하지 않음)을 반환한다. 아니면 `request` 소유권을 그대로
/// 돌려준다.
fn reject_if_abusive(
    request: tiny_http::Request,
    source: Option<&str>,
) -> Option<tiny_http::Request> {
    if let Some(src) = source
        && abuse::is_source_blocked(src)
    {
        if let Err(e) = request.respond(build_ack(AckStatus::TooManyRequests)) {
            tracing::debug!("webhook 429 respond failed: {e}");
        }
        return None;
    }
    Some(request)
}

/// 요청 헤더를 소문자 필드명 맵으로 수집.
fn collect_headers(request: &tiny_http::Request) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for h in request.headers() {
        headers.insert(
            h.field.as_str().as_str().to_ascii_lowercase(),
            h.value.as_str().to_string(),
        );
    }
    headers
}

/// 요청 바디를 읽어 JSON 으로 파싱. 읽기 실패/비-JSON 바디는 `Value::Null`.
fn read_json_body(request: &mut tiny_http::Request) -> Value {
    let mut body_str = String::new();
    if let Err(e) = request.as_reader().read_to_string(&mut body_str) {
        tracing::debug!("webhook body read failed: {e}");
    }
    serde_json::from_str::<Value>(&body_str).unwrap_or(Value::Null)
}

/// 매칭된 웹훅 실행에 필요한 (호출 시퀀스, injector). injector 는 아직 준비되지
/// 않았을 수 있어(주입 전) `Option`.
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
    // 인증 술어는 레지스트리가 lock 안에서 부른다 — 차감이 인증보다 앞서지 않게
    // 하려는 것이다. 검증은 ACK 상태코드 선택에만 관여하고 실행/응답바디에
    // 데이터를 싣지 않는다(단방향 불변식).
    match registry::match_request(path, method, |a| a.verify(headers, query, body)) {
        MatchResult::NotFound => (AckStatus::NotFound, None),
        MatchResult::MethodNotAllowed => (AckStatus::MethodNotAllowed, None),
        MatchResult::Expired => (AckStatus::Gone, None),
        MatchResult::Unauthorized => (AckStatus::Unauthorized, None),
        MatchResult::Matched { calls, injector } => (AckStatus::Received, Some((calls, injector))),
    }
}

/// `key=val&k2=v2` 쿼리 문자열 파싱(MVP — percent-decode 없음).
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

    /// 401 은 통을 태우지 않는다 — `registry` 쪽 405 짝
    /// (`method_mismatch_does_not_consume_count`)의 대칭 자리다. 그쪽이 registry 에
    /// 사는 것은 405 판정이 `match_request` 안에서 끝나기 때문이고, 401 판정은
    /// [`resolve_ack`] 에서 나므로 술어도 여기에 둔다.
    ///
    /// `--count N` 이 세는 단위는 **시퀀스를 돌린 횟수**이고 401 은 그것을 0 번
    /// 돌린다. 그리고 통의 키는 토큰이 아니라 **등록**이라, 차감하면 인증을 통과하지
    /// 못한 익명 발신자가 owner 의 예산을 태운다 — `remaining` 이 작으면 등록 자체가
    /// 사라지고 owner 에게 가는 신호는 없다.
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
            "401 은 시퀀스를 0 번 돌렸으므로 통을 태우지 않는다"
        );
    }

    #[test]
    fn parse_query_empty() {
        assert!(parse_query("").is_empty());
    }
}
