//! tiny_http bind + accept 스레드 + 연결당 스레드.
//!
//! 본체 웹훅과 같은 HTTP 레이어를 쓴다(`tiny_http`, blocking, ADR-0048). SDK 가 async 를
//! 지원하지 않아 plugin 도 blocking 서버 + 전용 스레드가 자연스러운 선택이고, 같은
//! 크레이트를 쓰면 판단 근거가 본체와 일치한다. ADR-0048 의 재검토 트리거 중 "롱-커넥션
//! 트래픽" 이 여기 해당하지만, 구독자는 FE 서버 한둘이라 연결당 스레드가 부담이 되는
//! 규모가 아니다 — 그 전제가 깨지면 ADR-0100 의 재검토 조건으로 다시 본다.
//!
//! 스레드 구성:
//!
//! - **accept 스레드 1 개** — `recv_timeout` 으로 돌며 stop 플래그를 본다. 타임아웃을
//!   쓰는 이유는 종료 신호를 받을 지점을 만들기 위해서다.
//! - **연결당 스레드 1 개** — 구독 후 자기 큐만 보며 프레임을 쓴다. 레지스트리 락은
//!   연결 시작 시 replay 를 읽을 때 한 번만 잡는다.

use std::collections::BTreeMap;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;
use tiny_http::{Method, Request, Response, Server};

use crate::registry::{Replay, StreamRegistry};
use crate::sse::hub::{SseHub, SubOptions, Subscription};
use crate::sse::{ServeConfig, frame, request};

/// accept 루프가 종료 신호를 확인하는 주기.
const ACCEPT_POLL: Duration = Duration::from_millis(250);

/// 연결 스레드가 종료 신호를 확인하는 주기.
const STREAM_POLL: Duration = Duration::from_millis(250);

/// 이 시간 동안 보낼 것이 없으면 keep-alive 주석을 흘린다.
const KEEP_ALIVE_EVERY: Duration = Duration::from_secs(15);

/// 종료 시 연결 스레드가 빠지길 기다리는 상한.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// 소비자가 끊겼을 때 재접속까지 기다릴 시간(ms) 힌트. 끊김이 정상 경로라 명시한다.
const RETRY_HINT_MS: u64 = 3000;

/// 연결 핸들 리스트 락의 poison 복구 공용 보고 좌표(첫-1 회). 담는 것은 `JoinHandle`
/// 목록뿐이라 복구가 안전하다 — 틀린 것은 흔적이 없다는 것이었다.
const CONNECTIONS_WHAT: &str = "the SSE connection-handle list";
static CONNECTIONS_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// 스트림 레지스트리(replay 버퍼) 락의 poison 복구 공용 보고 좌표(첫-1 회).
const STREAM_REGISTRY_WHAT: &str = "the SSE stream registry";
static STREAM_REGISTRY_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// SSE 응답 헤더 + 재접속 힌트. `Content-Length` 가 없고 연결을 닫지 않는다.
const STREAM_PREAMBLE: &str = concat!(
    "HTTP/1.1 200 OK\r\n",
    "Content-Type: text/event-stream\r\n",
    "Cache-Control: no-cache\r\n",
    "Connection: close\r\n",
    // nginx 류 중간 프록시의 응답 버퍼링을 끈다 — 버퍼링되면 이벤트가 뭉쳐서 도착한다.
    "X-Accel-Buffering: no\r\n",
    "\r\n",
);

type Shared = Arc<Mutex<StreamRegistry>>;

/// 연결 스레드가 공유하는 것들.
struct ConnCtx {
    hub: Arc<SseHub>,
    registry: Shared,
    token: Option<String>,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
}

/// 살아 있는 SSE 서버 한 대.
pub struct SseServer {
    config: ServeConfig,
    /// 실제로 bind 된 주소. 설정과 같지만 커널이 확정한 값을 노출한다.
    bound: Option<SocketAddr>,
    hub: Arc<SseHub>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    active: Arc<AtomicUsize>,
}

impl std::fmt::Debug for SseServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SseServer")
            .field("config", &self.config)
            .field("bound", &self.bound)
            .finish_non_exhaustive()
    }
}

/// 설정대로 bind 하고 accept 스레드를 띄운다. bind 실패는 **폴백 없이** 그대로 올린다.
pub fn start(config: ServeConfig, hub: Arc<SseHub>, registry: Shared) -> Result<SseServer, String> {
    let addr = format!("{}:{}", config.bind, config.port);
    let server = Server::http(addr.as_str()).map_err(|e| e.to_string())?;
    let bound = server.server_addr().to_ip();
    let stop = Arc::new(AtomicBool::new(false));
    let connections: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::new(Mutex::new(Vec::new()));
    let active = Arc::new(AtomicUsize::new(0));
    let ctx = Arc::new(ConnCtx {
        hub: hub.clone(),
        registry,
        token: config.token.clone(),
        stop: stop.clone(),
        active: active.clone(),
    });
    let accept = std::thread::Builder::new()
        .name("agent-stream-sse".into())
        .spawn({
            let connections = connections.clone();
            move || accept_loop(server, ctx, connections)
        })
        .map_err(|e| e.to_string())?;
    tracing::info!("agent-stream: SSE endpoint listening on http://{addr}/events");
    Ok(SseServer {
        config,
        bound,
        hub,
        stop,
        accept: Some(accept),
        connections,
        active,
    })
}

impl SseServer {
    /// `agent_stream.serve_info` 응답. 토큰은 담지 않는다.
    ///
    /// `running` 은 **stop 플래그에서 도출한다** — accept 루프가 스스로 죽으면
    /// (`accept_loop` 의 Err 분기) 리스너 소켓은 이미 닫혔는데 `SseServer` 값은 남는다.
    /// 그때 `true` 를 고정으로 넣으면 "떠 있다고 보고하는데 아무도 붙을 수 없는" 상태가
    /// 되고, 운영자가 그것을 알아챌 유일한 창이 이 응답이다.
    pub fn to_json(&self) -> Value {
        let (subs, total_dropped) = self.hub.stats();
        let host = match self.bound {
            Some(addr) => addr.to_string(),
            None => format!("{}:{}", self.config.bind, self.config.port),
        };
        let mut info = self.config.to_public_json();
        let map = info.as_object_mut().expect("to_public_json object");
        map.insert(
            "running".into(),
            Value::from(!self.stop.load(Ordering::SeqCst)),
        );
        map.insert("url".into(), Value::from(format!("http://{host}/events")));
        map.insert(
            "subscribers".into(),
            Value::from(subs.iter().map(|s| s.to_json()).collect::<Vec<_>>()),
        );
        map.insert("total_dropped".into(), Value::from(total_dropped));
        info
    }

    /// 종료: 신호 → 구독 전부 끊기 → accept 스레드 join → 연결 스레드 join(한정 대기).
    ///
    /// 연결 스레드를 무한정 join 하지 않는 이유는, 소켓 송신 버퍼가 막힌 클라이언트에서
    /// `write` 가 오래 걸릴 수 있기 때문이다. 이 함수는 IPC 핸들러(dispatch 스레드)에서
    /// 불리므로 여기서 멈추면 healthcheck 무응답 → 강제 재시작으로 번진다.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 송신단을 없애 연결 스레드의 `recv` 를 즉시 깨운다.
        self.hub.close_all();
        if let Some(handle) = self.accept.take()
            && handle.join().is_err()
        {
            tracing::warn!("agent-stream: the SSE accept thread panicked");
        }
        self.join_connections();
    }

    fn join_connections(&self) {
        self.wait_for_idle_connections();
        let stuck = join_finished(self.take_connection_handles());
        if stuck > 0 {
            tracing::warn!(
                "agent-stream: {stuck} SSE connection thread(s) did not finish within {}s — detached (their sockets close when the process exits)",
                SHUTDOWN_GRACE.as_secs()
            );
        }
    }

    /// 연결 스레드가 전부 빠질 때까지 기다린다(상한 [`SHUTDOWN_GRACE`]).
    fn wait_for_idle_connections(&self) {
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        while self.active.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn take_connection_handles(&self) -> Vec<JoinHandle<()>> {
        let mut list = tasty_utils::poison::recover_mutex(
            self.connections.lock(),
            CONNECTIONS_WHAT,
            &CONNECTIONS_POISON_REPORTED,
        );
        list.drain(..).collect()
    }
}

/// 이미 끝난 스레드만 join 한다. 반환값은 아직 안 끝나 떼어놓은 개수 — 소켓 송신이
/// 막힌 클라이언트에서 `write` 가 오래 걸릴 수 있어, 여기서 무한정 기다리면 이 함수를
/// 호출한 dispatch 스레드가 막히고 healthcheck 무응답으로 번진다.
fn join_finished(handles: Vec<JoinHandle<()>>) -> usize {
    let mut stuck = 0usize;
    for handle in handles {
        if handle.is_finished() {
            join_one(handle);
        } else {
            stuck += 1;
        }
    }
    stuck
}

fn join_one(handle: JoinHandle<()>) {
    if handle.join().is_err() {
        tracing::warn!("agent-stream: an SSE connection thread panicked");
    }
}

impl Drop for SseServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(server: Server, ctx: Arc<ConnCtx>, connections: Arc<Mutex<Vec<JoinHandle<()>>>>) {
    while !ctx.stop.load(Ordering::SeqCst) {
        match server.recv_timeout(ACCEPT_POLL) {
            Ok(Some(req)) => spawn_connection(req, &ctx, &connections),
            // 타임아웃 — stop 플래그를 다시 본다.
            Ok(None) => {}
            Err(e) => {
                // 리스너가 더 못 돈다 — `serve_info` 가 계속 `running: true` 로 보이지
                // 않도록 stop 을 세우고 나간다(연결 스레드도 같은 플래그로 빠진다).
                ctx.stop.store(true, Ordering::SeqCst);
                tracing::warn!("agent-stream: SSE accept failed: {e} — the listener stops");
                return;
            }
        }
        reap_finished(&connections);
    }
}

fn spawn_connection(
    req: Request,
    ctx: &Arc<ConnCtx>,
    connections: &Arc<Mutex<Vec<JoinHandle<()>>>>,
) {
    ctx.active.fetch_add(1, Ordering::SeqCst);
    let active = ctx.active.clone();
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new()
        .name("agent-stream-sse-conn".into())
        .spawn(move || {
            handle_request(req, &ctx);
            ctx.active.fetch_sub(1, Ordering::SeqCst);
        });
    match spawned {
        Ok(handle) => tasty_utils::poison::recover_mutex(
            connections.lock(),
            CONNECTIONS_WHAT,
            &CONNECTIONS_POISON_REPORTED,
        )
        .push(handle),
        Err(e) => {
            active.fetch_sub(1, Ordering::SeqCst);
            tracing::warn!("agent-stream: cannot spawn an SSE connection thread: {e}");
        }
    }
}

/// 이미 끝난 연결 스레드를 걷어낸다 — 오래 뜬 서버에서 핸들이 무한히 쌓이지 않게.
fn reap_finished(connections: &Arc<Mutex<Vec<JoinHandle<()>>>>) {
    let mut list = tasty_utils::poison::recover_mutex(
        connections.lock(),
        CONNECTIONS_WHAT,
        &CONNECTIONS_POISON_REPORTED,
    );
    let mut kept = Vec::with_capacity(list.len());
    for handle in list.drain(..) {
        if handle.is_finished() {
            join_one(handle);
        } else {
            kept.push(handle);
        }
    }
    *list = kept;
}

fn handle_request(req: Request, ctx: &ConnCtx) {
    let url = req.url().to_string();
    let headers = collect_headers(&req);
    let query = request::parse_query(&url);
    if request::path_of(&url) != request::STREAM_PATH {
        respond_empty(req, 404);
        return;
    }
    if req.method() != &Method::Get {
        respond_empty(req, 405);
        return;
    }
    if !request::authorize(ctx.token.as_deref(), &headers, &query) {
        // 거부 응답은 **빈 바디**다 — 어떤 내부 상태도 싣지 않는다.
        tracing::warn!("agent-stream: rejected an unauthenticated SSE subscription");
        respond_empty(req, 401);
        return;
    }
    stream(req, ctx, &headers, &query);
}

fn collect_headers(req: &Request) -> BTreeMap<String, String> {
    req.headers()
        .iter()
        .map(|h| {
            (
                h.field.as_str().as_str().to_ascii_lowercase(),
                h.value.as_str().to_string(),
            )
        })
        .collect()
}

fn respond_empty(req: Request, status: u16) {
    if let Err(e) = req.respond(Response::empty(status)) {
        tracing::debug!("agent-stream: cannot send the {status} response: {e}");
    }
}

fn stream(
    req: Request,
    ctx: &ConnCtx,
    headers: &BTreeMap<String, String>,
    query: &BTreeMap<String, String>,
) {
    let opts = request::sub_options(query);
    let resume = request::resume_from(headers, query);
    // **구독을 먼저 등록하고** replay 를 읽는다. 반대 순서면 그 사이 이벤트가 어느 쪽에도
    // 안 잡혀 조용히 사라진다. 이 순서에서는 겹치기만 하고, 겹친 것은 seq 로 접는다.
    let sub = ctx.hub.subscribe(opts);
    let replay = collect_replay(&ctx.registry, resume, opts);

    let mut writer = req.into_writer();
    if !write_str(&mut writer, STREAM_PREAMBLE) {
        return;
    }
    if !write_str(&mut writer, &format!("retry: {RETRY_HINT_MS}\n\n")) {
        return;
    }
    let mut last_seq = 0u64;
    if let Some((from, to)) = replay.gap {
        // 재전송할 수 없는 구간을 **먼저** 알린다. `id` 는 소비자가 보낸 커서 그대로다 —
        // 갭 통지가 커서를 전진시키면 그 뒤 재연결에서 남은 이벤트까지 건너뛴다.
        let payload = format!(r#"{{"kind":"gap","from":{from},"to":{to}}}"#);
        if !write_str(
            &mut writer,
            &frame::encode(from.saturating_sub(1), "gap", &payload),
        ) {
            return;
        }
    }
    for event in replay.events {
        if !write_str(&mut writer, &event.frame) {
            return;
        }
        last_seq = event.seq;
    }
    pump_stream(&mut writer, &sub, ctx, last_seq);
}

/// 재개 커서가 있으면 수집 버퍼에서 그 뒤의 이벤트를 꺼낸다. 없으면 재전송하지 않는다.
fn collect_replay(registry: &Shared, resume: Option<u64>, opts: SubOptions) -> Replay {
    let Some(after_seq) = resume else {
        return Replay::default();
    };
    let reg = tasty_utils::poison::recover_mutex(
        registry.lock(),
        STREAM_REGISTRY_WHAT,
        &STREAM_REGISTRY_POISON_REPORTED,
    );
    reg.replay_after(after_seq, opts)
}

fn pump_stream(
    writer: &mut Box<dyn Write + Send + 'static>,
    sub: &Subscription,
    ctx: &ConnCtx,
    mut last_seq: u64,
) {
    let mut idle_since = Instant::now();
    while !ctx.stop.load(Ordering::SeqCst) {
        match sub.rx.recv_timeout(STREAM_POLL) {
            Ok(event) => {
                // replay 와 겹친 구간은 건너뛴다 — seq 가 단조 증가라 이 비교로 충분하다.
                if event.seq <= last_seq {
                    continue;
                }
                if !write_str(writer, &event.frame) {
                    return;
                }
                last_seq = event.seq;
                idle_since = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {
                if idle_since.elapsed() < KEEP_ALIVE_EVERY {
                    continue;
                }
                if !write_str(writer, frame::KEEP_ALIVE) {
                    return;
                }
                idle_since = Instant::now();
            }
            // 허브가 이 구독을 끊었다(shutdown 또는 연속 drop 임계 초과).
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// 소켓에 쓰고 즉시 flush 한다. 실패는 "소비자가 끊었다" 는 정상 종료 신호다.
fn write_str(writer: &mut Box<dyn Write + Send + 'static>, text: &str) -> bool {
    if let Err(e) = writer.write_all(text.as_bytes()) {
        tracing::debug!("agent-stream: SSE subscriber went away while writing: {e}");
        return false;
    }
    if let Err(e) = writer.flush() {
        tracing::debug!("agent-stream: SSE subscriber went away while flushing: {e}");
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_preamble_declares_an_unbuffered_event_stream_without_a_content_length() {
        assert!(STREAM_PREAMBLE.contains("Content-Type: text/event-stream"));
        assert!(STREAM_PREAMBLE.contains("Cache-Control: no-cache"));
        assert!(STREAM_PREAMBLE.contains("X-Accel-Buffering: no"));
        assert!(
            !STREAM_PREAMBLE
                .to_ascii_lowercase()
                .contains("content-length")
        );
        assert!(STREAM_PREAMBLE.ends_with("\r\n\r\n"));
    }

    #[test]
    fn a_bad_bind_address_fails_loudly_instead_of_falling_back_to_another_port() {
        let registry = Arc::new(Mutex::new(StreamRegistry::new(None)));
        let hub = Arc::new(SseHub::default());
        // 존재하지 않는 로컬 주소 — 커널이 bind 를 거부한다.
        let config = ServeConfig {
            bind: "192.0.2.1".into(),
            port: 9,
            token: Some("t".into()),
        };
        let err = start(config, hub, registry).expect_err("bind must fail");
        assert!(!err.is_empty());
    }

    #[test]
    fn a_started_server_reports_its_url_without_leaking_the_token() {
        let registry = Arc::new(Mutex::new(StreamRegistry::new(None)));
        let hub = Arc::new(SseHub::default());
        let config = ServeConfig {
            bind: "127.0.0.1".into(),
            // 0 은 `validate()` 가 막지만, 테스트는 커널 할당 포트로 bind 자체를 검증한다.
            port: 0,
            token: Some("super-secret".into()),
        };
        let mut server = start(config, hub, registry).expect("bind");
        let info = server.to_json();
        assert_eq!(info["running"], json!(true));
        assert_eq!(info["auth"], json!(true));
        assert!(!info.to_string().contains("super-secret"));
        assert!(
            info["url"].as_str().expect("url").ends_with("/events"),
            "{info}"
        );
        server.shutdown();
    }

    #[test]
    fn a_stopped_listener_no_longer_reports_itself_as_running() {
        let (mut server, _port, _registry) = serve_on_ephemeral_port(None);
        assert_eq!(server.to_json()["running"], json!(true));

        // `shutdown` 은 stop 을 세우고 accept 스레드를 join 한다 — 그 반환으로 tiny_http
        // `Server` 가 drop 되어 리스너 소켓이 닫힌다. `SseServer` 값은 그대로 남으므로,
        // `running` 을 상수로 두면 "떠 있다고 보고하는데 아무도 붙을 수 없는" 상태가 되고
        // 운영자가 그것을 알아챌 유일한 창이 이 응답이다. accept 루프가 스스로 죽는
        // 경로(`accept_loop` 의 Err 분기)도 같은 플래그를 세우므로 같은 판정을 탄다.
        server.shutdown();

        let info = server.to_json();
        assert_eq!(info["running"], json!(false), "{info}");
    }

    // ── 소켓 왕복 e2e ────────────────────────────────────────────────────
    // 프레이밍/인증/스트리밍이 실제 HTTP 연결 위에서 맞는지 본다. 단위 테스트가
    // 통과해도 헤더를 잘못 쓰거나 flush 를 빠뜨리면 소비자에겐 아무것도 안 온다.

    use std::io::Read;
    use std::net::TcpStream;

    use crate::record::{EventKind, StreamEvent};

    fn serve_on_ephemeral_port(token: Option<&str>) -> (SseServer, u16, Shared) {
        let registry = Arc::new(Mutex::new(StreamRegistry::new(None)));
        let hub = registry.lock().expect("lock").hub();
        let config = ServeConfig {
            bind: "127.0.0.1".into(),
            port: 0,
            token: token.map(str::to_string),
        };
        let server = start(config, hub, registry.clone()).expect("bind");
        let port = server.bound.expect("ip addr").port();
        (server, port, registry)
    }

    fn get(port: u16, target: &str) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        let mut writer = &stream;
        write!(writer, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").expect("request");
        writer.flush().expect("flush");
        stream
    }

    /// `needle` 이 보일 때까지 읽는다. 타임아웃/EOF 면 지금까지 읽은 것을 돌려준다.
    fn read_until(stream: &mut TcpStream, needle: &str) -> String {
        let mut seen = String::new();
        let mut buf = [0u8; 1024];
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    seen.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if seen.contains(needle) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        seen
    }

    fn push(registry: &Shared, seq_text: &str) {
        let mut event = StreamEvent::turn_end("placeholder");
        event.kind = EventKind::Text;
        event.reason = None;
        event.text = Some(seq_text.to_string());
        registry.lock().expect("lock").push_event(7, "sess", event);
    }

    #[test]
    fn a_subscriber_gets_an_event_stream_response_and_then_live_frames() {
        let (mut server, port, registry) = serve_on_ephemeral_port(None);
        let mut stream = get(port, "/events");
        let head = read_until(&mut stream, "retry:");
        assert!(head.contains("200 OK"), "{head}");
        assert!(head.contains("text/event-stream"), "{head}");

        push(&registry, "hello-from-the-agent");
        let body = read_until(&mut stream, "hello-from-the-agent");
        assert!(body.contains("event: text"), "{body}");
        assert!(body.contains("data: "), "{body}");
        assert!(
            body.contains("\n\n"),
            "frames must end with a blank line: {body}"
        );
        server.shutdown();
    }

    #[test]
    fn thinking_is_only_streamed_when_the_subscription_asks_for_it() {
        let (mut server, port, registry) = serve_on_ephemeral_port(None);
        let mut plain = get(port, "/events");
        let mut full = get(port, "/events?thinking=1");
        read_until(&mut plain, "retry:");
        read_until(&mut full, "retry:");

        let mut thought = StreamEvent::turn_end("placeholder");
        thought.kind = EventKind::Thinking;
        thought.reason = None;
        thought.text = Some("private-reasoning".into());
        registry.lock().expect("lock").push_event(7, "s", thought);
        push(&registry, "public-answer");

        let opted_in = read_until(&mut full, "public-answer");
        assert!(opted_in.contains("private-reasoning"), "{opted_in}");
        let default_view = read_until(&mut plain, "public-answer");
        assert!(
            !default_view.contains("private-reasoning"),
            "{default_view}"
        );
        server.shutdown();
    }

    #[test]
    fn last_event_id_replays_from_the_collection_buffer_without_duplicating_live_frames() {
        let (mut server, port, registry) = serve_on_ephemeral_port(None);
        push(&registry, "before-one");
        push(&registry, "before-two");

        let mut stream = get(port, "/events");
        // seq 1 뒤부터 — 두 번째 이벤트만 재전송된다.
        let mut resumed = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        resumed
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        write!(
            resumed,
            "GET /events HTTP/1.1\r\nHost: localhost\r\nLast-Event-ID: 1\r\n\r\n"
        )
        .expect("request");
        let replayed = read_until(&mut resumed, "before-two");
        assert!(replayed.contains("id: 2"), "{replayed}");
        assert!(!replayed.contains("before-one"), "{replayed}");

        // 재개하지 않은 구독자에게는 과거 이벤트가 가지 않는다.
        let live_only = read_until(&mut stream, "retry:");
        assert!(!live_only.contains("before-two"), "{live_only}");

        push(&registry, "after-resume");
        let live = read_until(&mut resumed, "after-resume");
        // replay 로 이미 보낸 seq 2 가 라이브로 한 번 더 나오지 않는다.
        assert_eq!(live.matches("before-two").count(), 0, "{live}");
        server.shutdown();
    }

    #[test]
    fn a_cursor_that_fell_out_of_the_buffer_gets_a_gap_frame_before_the_replay() {
        let (mut server, port, registry) = serve_on_ephemeral_port(None);
        for i in 0..(crate::registry::EVENT_BUFFER_CAP + 5) {
            push(&registry, &format!("e{i}"));
        }

        // surface 필터로 재전송 본문은 비우고 갭 통지만 본다 — 갭 판정은 필터와 무관하다.
        let mut resumed = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        resumed
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        write!(
            resumed,
            "GET /events?surface=999 HTTP/1.1\r\nHost: localhost\r\nLast-Event-ID: 0\r\n\r\n"
        )
        .expect("request");
        let seen = read_until(&mut resumed, "\"to\":5");
        assert!(seen.contains("event: gap"), "{seen}");
        assert!(seen.contains("\"from\":1"), "{seen}");
        // 커서를 전진시키지 않는다 — 갭 프레임의 id 는 소비자가 보낸 커서 그대로다.
        assert!(seen.contains("id: 0\nevent: gap"), "{seen}");
        server.shutdown();
    }

    #[test]
    fn a_missing_or_wrong_token_is_rejected_with_an_empty_body() {
        let (mut server, port, _registry) = serve_on_ephemeral_port(Some("s3cret"));
        for target in ["/events", "/events?token=wrong"] {
            let mut stream = get(port, target);
            let response = read_until(&mut stream, "\r\n\r\n");
            assert!(response.contains("401"), "{target}: {response}");
            // 거부 응답에 내부 데이터가 실리지 않는다.
            assert!(!response.contains("data:"), "{target}: {response}");
            assert!(!response.contains("s3cret"), "{target}: {response}");
        }
        let mut ok = get(port, "/events?token=s3cret");
        assert!(read_until(&mut ok, "retry:").contains("200 OK"));
        server.shutdown();
    }

    #[test]
    fn any_other_path_is_a_plain_404() {
        let (mut server, port, _registry) = serve_on_ephemeral_port(None);
        let mut stream = get(port, "/");
        let response = read_until(&mut stream, "\r\n\r\n");
        assert!(response.contains("404"), "{response}");
        server.shutdown();
    }
}
