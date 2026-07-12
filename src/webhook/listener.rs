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
        Ok(server) => {
            registry::mark_bound();
            tracing::info!("webhook listener bound on {addr}");
            if let Err(e) = thread::Builder::new()
                .name("webhook-accept".into())
                .spawn(move || accept_loop(server))
            {
                tracing::warn!("webhook accept thread spawn failed: {e}");
            }
            WebhookInitReport::Bound
        }
        Err(e) => {
            let error = e.to_string();
            tracing::warn!(
                "webhook listener bind {addr} failed: {error} — set a free port and check firewall (no auto-fallback)"
            );
            WebhookInitReport::BindFailed { port, error }
        }
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
fn handle_request(mut request: tiny_http::Request) {
    // 출처 IP(포트 제외 — 스캐너는 IP 를 재사용하며 포트만 바꾼다).
    let source = request.remote_addr().map(|a| a.ip().to_string());

    // 남용 차단: 쿨다운 중 출처는 파싱/실행 전 즉시 429 로 거부한다.
    if let Some(src) = source.as_deref()
        && abuse::is_source_blocked(src)
    {
        if let Err(e) = request.respond(build_ack(AckStatus::TooManyRequests)) {
            tracing::debug!("webhook 429 respond failed: {e}");
        }
        return;
    }

    let url = request.url().to_string();
    let method = request.method().to_string().to_ascii_uppercase();

    let (path_raw, query_str) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url.as_str(), ""),
    };
    let path = path_raw.trim_start_matches('/').to_string();
    let query = parse_query(query_str);

    let mut headers = BTreeMap::new();
    for h in request.headers() {
        headers.insert(
            h.field.as_str().as_str().to_ascii_lowercase(),
            h.value.as_str().to_string(),
        );
    }

    let mut body_str = String::new();
    if let Err(e) = request.as_reader().read_to_string(&mut body_str) {
        tracing::debug!("webhook body read failed: {e}");
    }
    let body = serde_json::from_str::<Value>(&body_str).unwrap_or(Value::Null);

    let (ack, exec) = match registry::match_request(&path, &method) {
        MatchResult::NotFound => (AckStatus::NotFound, None),
        MatchResult::MethodNotAllowed => (AckStatus::MethodNotAllowed, None),
        MatchResult::Expired => (AckStatus::Gone, None),
        MatchResult::Matched {
            calls,
            injector,
            auth,
        } => {
            // 인증이 설정된 웹훅은 실행 전 토큰을 검증한다. 미설정(None)은 통과.
            // 검증은 ACK 상태코드 선택에만 관여하고 실행/응답바디에 데이터를 싣지 않음.
            match &auth {
                Some(a) if !a.verify(&headers, &query, &body) => (AckStatus::Unauthorized, None),
                _ => (AckStatus::Received, Some((calls, injector))),
            }
        }
    };

    // 404/405 는 출처 실패로 집계(임계치 초과 시 다음 요청부터 쿨다운 429).
    // 정상 매칭(200)은 집계하지 않으므로 정상 웹훅 트래픽은 영향받지 않는다.
    if matches!(ack, AckStatus::NotFound | AckStatus::MethodNotAllowed)
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

    #[test]
    fn parse_query_empty() {
        assert!(parse_query("").is_empty());
    }
}
