//! Production adapter — `TcpIpcServer`. TCP listener + mpsc channel + accept
//! thread 로 JSON-RPC 요청을 받는다. Hub 가 `Box<dyn IpcServerPort>` 로 보유.
//!
//! wire 타입 (`IpcCommand` / `IpcWaker` / `send_response`) 은 옛
//! 위치 (`crate::ipc::server`) 에 잔존 — wire 형식과 강결합이라 trait 옆이 아니라
//! wire 모듈에 두는 게 자연스럽다 (verify 자율 결정).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::adapters::production::stream_hub::{StreamClientId, StreamContext, StreamInbound};
use crate::ipc::port_file;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::ipc::server::{IpcCommand, IpcWaker};
use crate::ipc::stream::{self, StreamAck, StreamFrame, StreamTag};
use crate::ports::ipc_server::IpcServerPort;

/// 한 요청 줄이 읽어 들일 수 있는 최대 바이트.
///
/// `read_line` 은 개행을 만날 때까지 `String` 을 키운다. 상한이 없으면 **개행을 끝내
/// 보내지 않는 peer 하나가** 호스트 메모리를 끝까지 먹는다. 스트림 업그레이드 경로에는
/// 대응하는 상한이 이미 둘 있다 — 프레임 길이 접두사를 자르는 [`stream::MAX_FRAME_LEN`]
/// 과 [`TcpIpcServer::arm_stream_read_timeout`] 의 read timeout. 일반 RPC 경로에만
/// 없었고, 이 상수가 그 비대칭을 없앤다.
///
/// 값의 근거: 호스트가 받아들이는 가장 큰 요청 payload 는 `memory.set` 의 값이고 그
/// 상한은 `tasty_memory::MemoryConfig::entry_max_bytes`(기본 1 MiB)다. 그 값이 한 줄에
/// 실릴 때의 팽창률은 두 갈래다 — `value_b64` 는 base64 라 4/3 배, `value` 문자열은 JSON
/// escape 가 최악에 바이트당 `\u00XX` 6 자라 6 배. 그래서 **정상** 요청의 상한은 6 MiB +
/// 봉투이고 8 MiB 는 그 위의 여유다. 이 수를 줄이려면 `entry_max_bytes` 를 먼저 줄여야
/// 한다 — 지금 통과하는 요청을 거절하게 된다.
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024 * 1024;

/// 한 줄 읽기의 결과. `read_line` 의 `Ok(n)` 하나로 뭉뚱그려지던 것을 갈래로 나눈다 —
/// 특히 **상한 초과**는 정상적으로 읽은 줄과 구분돼야 한다.
enum LineRead {
    /// 개행까지 한 줄을 읽었다.
    Line,
    /// peer 가 연결을 닫았다.
    Eof,
    /// 개행 없이 [`MAX_REQUEST_LINE_BYTES`] 를 채웠다 — 연결을 닫는다.
    TooLong,
    /// 소켓 오류 또는 비-UTF-8.
    Failed,
}

/// 동시에 살아 있을 수 있는 IPC 연결 수의 상한.
///
/// accept 루프는 연결마다 스레드를 하나 낸다. 상한이 없으면 연결을 여는 것만으로
/// 스레드와 스택이 무한히 늘어난다 — 각 연결이 요청을 하나도 안 보내도 그렇다.
///
/// 값의 근거는 **정상 인구의 상한**이다: 번들 plugin 은 아홉이고 각자 host-call 연결을
/// 들며, attach/mesh/bulk 스트림은 워크스페이스당 몇 개 단위이고, CLI 호출은 요청 하나마다
/// 열고 닫는 단명 연결이다. 그 합은 수십 규모라 256 은 그 위의 자리수 여유다.
/// **실행 인스턴스에서 세어 보정한 값은 아니다** — 정상 사용이 이 수에 닿으면 그것이
/// 보정 신호다(닿는 순간 `warn` 이 한 줄 남는다).
const MAX_CONCURRENT_CONNECTIONS: usize = 256;

/// 살아 있는 연결 하나의 자리. 스레드가 어떻게 끝나든(정상·조기 return·패닉) `Drop`
/// 이 자리를 돌려준다 — 회수를 `handle_connection` 의 제어흐름에 맡기지 않는다.
struct ConnectionSlot {
    live: Arc<AtomicUsize>,
}

impl ConnectionSlot {
    /// 상한 안이면 자리를 잡고 `Some`, 아니면 되돌리고 `None`.
    ///
    /// `saturated` 는 **로그 폭주를 막기 위한 것**이다. 거절은 상대가 이미 상한만큼
    /// 연결을 쥐고 있을 때만 나는데, 그 상대가 재시도 루프를 돌면 거절마다 warn 한 줄이
    /// 나가 로그 파일을 채운다. 그래서 포화로 **들어가는 순간**만 warn 이고 그 뒤로는
    /// debug 다. 자리가 하나라도 반납되면 다시 warn 할 수 있게 풀린다.
    fn try_acquire(live: &Arc<AtomicUsize>, saturated: &AtomicBool) -> Option<Self> {
        let prev = live.fetch_add(1, Ordering::Relaxed);
        if prev >= MAX_CONCURRENT_CONNECTIONS {
            live.fetch_sub(1, Ordering::Relaxed);
            if saturated.swap(true, Ordering::Relaxed) {
                tracing::debug!("IPC connection refused (still at {MAX_CONCURRENT_CONNECTIONS})");
            } else {
                tracing::warn!(
                    "IPC connection refused — {MAX_CONCURRENT_CONNECTIONS} concurrent connections already live"
                );
            }
            return None;
        }
        saturated.store(false, Ordering::Relaxed);
        Some(Self { live: live.clone() })
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

/// 스트리밍 핸드셰이크 params 에서 추출한 attach 대상(surface/workspace 중 하나).
struct StreamHandshake {
    /// client 가 선언한 스트림 프로토콜 버전. `STREAM_PROTO` 와 다르면 attach 를
    /// **dispatch 하지 않는다** — 상세는 `validate_stream_proto`.
    proto: u32,
    attach_target: Option<u32>,
    attach_workspace: Option<u32>,
}

/// TCP-backed IPC server. listening on 127.0.0.1:{dynamic} + writing port to
/// `~/.tasty/tasty.port` 등 외부 통신 표면.
pub struct TcpIpcServer {
    command_rx: mpsc::Receiver<IpcCommand>,
    /// Sender 사본 — host→plugin sync dispatch 시 외부 thread 가 직접 push.
    command_tx: mpsc::Sender<IpcCommand>,
    port: u16,
    /// Shutdown flag to signal the accept thread to stop.
    shutdown: Arc<AtomicBool>,
    /// Custom port file path (overrides default if set).
    custom_port_file: Option<std::path::PathBuf>,
}

impl TcpIpcServer {
    /// Start the IPC server with an optional custom port file path and waker.
    /// The waker is called whenever an IPC command is enqueued, so the event
    /// loop can wake up and process it immediately.
    pub fn start_with_port_file(
        port_file_override: Option<String>,
        waker: Option<IpcWaker>,
        stream_ctx: StreamContext,
    ) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        tracing::info!("IPC server listening on 127.0.0.1:{}", port);

        let custom_port_file = port_file_override.map(std::path::PathBuf::from);

        // Write port file so CLI clients can find us
        port_file::write_port_file_to(port, custom_port_file.as_deref())?;

        // 이 인스턴스가 자기 데이터 루트의 주인이 된 순간, 과거 완료 알림 로그
        // (`notify/`)를 통째로 청소한다. surface_id 는 재시작마다 새로 발급되므로
        // 이전 프로세스가 남긴 로그 파일들은 이 인스턴스에선 모두 죽은 surface 의
        // 것 — 읽을 reader 가 없다. 부팅 latency 에 영향을 주지 않도록 결과를
        // 기다리지 않는 fire-and-forget 으로 던진다(다음 append 시 create_dir_all
        // 이 알아서 재생성한다).
        Self::spawn_notify_dir_cleanup();

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        // Accept connections in a background thread with non-blocking + shutdown check
        let shutdown_clone = shutdown.clone();
        let accept_tx = cmd_tx.clone();
        // 살아 있는 연결 수 + 포화 로그 게이트. accept 스레드만 읽고 쓰지만 자리 반납은
        // 각 연결 스레드의 `Drop` 이 하므로 공유 소유가 필요하다.
        let live_connections = Arc::new(AtomicUsize::new(0));
        let saturated = Arc::new(AtomicBool::new(false));
        listener.set_nonblocking(true)?;
        thread::spawn(move || {
            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        // 상한 초과면 소켓을 그대로 닫는다. 거절을 **응답으로** 알리는
                        // 것은 wire 가 걸리는 별개 결정이라 여기서는 하지 않는다 —
                        // 연결이 즉시 닫히는 것이 이 회차의 거절 신호다.
                        let Some(slot) = ConnectionSlot::try_acquire(&live_connections, &saturated)
                        else {
                            drop(stream);
                            continue;
                        };
                        let cmd_tx = accept_tx.clone();
                        let waker = waker.clone();
                        let stream_ctx = stream_ctx.clone();
                        thread::spawn(move || {
                            Self::handle_connection(stream, cmd_tx, waker, stream_ctx, slot);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        tracing::warn!("IPC accept error: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            command_rx: cmd_rx,
            command_tx: cmd_tx,
            port,
            shutdown,
            custom_port_file,
        })
    }

    /// 자기 데이터 루트 밑 `notify/` 디렉토리를 별도 스레드에서 통째로 삭제한다
    /// (fire-and-forget — join 하지 않아 부팅 흐름을 막지 않는다). 홈 미확인 시
    /// 아무것도 하지 않는다.
    fn spawn_notify_dir_cleanup() {
        let Some(dir) = tasty_utils::path::tasty_home().map(|home| home.join("notify")) else {
            return;
        };
        thread::spawn(move || Self::clear_notify_dir(&dir));
    }

    /// `notify/` 디렉토리를 통째로 삭제한다. 디렉토리가 애초에 없으면(NotFound)
    /// 정상 상황이라 무시하고, 그 외 에러만 로그한다. 재생성은 하지 않는다 —
    /// 다음 `append_notify_line` 이 `create_dir_all` 로 알아서 만든다.
    fn clear_notify_dir(dir: &std::path::Path) {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!("failed to clear notify dir {}: {}", dir.display(), e);
            }
        }
    }

    ///
    /// `_slot` 은 살아 있는 연결 수의 자리다 — 인자로 받아 이 함수의 수명에 묶는다.
    /// 반납을 본문의 제어흐름(조기 return 이 넷)에 맡기지 않으려는 것이다.
    fn handle_connection(
        stream: std::net::TcpStream,
        cmd_tx: mpsc::Sender<IpcCommand>,
        waker: Option<IpcWaker>,
        stream_ctx: StreamContext,
        _slot: ConnectionSlot,
    ) {
        let Some((mut reader, mut writer, peer)) = Self::prepare_stream(stream) else {
            return;
        };

        // Read the first line manually so the BufReader retains any bytes
        // buffered after it. On a streaming-channel upgrade those buffered bytes
        // are the start of the binary frames following the handshake line.
        let Some(mut line) = Self::read_first_line(&mut reader, peer) else {
            return;
        };

        // Streaming upgrade: first line is `{"method":"stream.open",...}`. The
        // connection leaves the request-response model and becomes a framed
        // bidirectional pipe.
        if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(line.trim())
            && req.method == stream::STREAM_OPEN_METHOD
        {
            Self::handle_stream_connection(reader, writer, req, stream_ctx, peer);
            return;
        }

        Self::run_request_response_loop(&mut reader, &mut writer, &mut line, &cmd_tx, &waker, peer);

        tracing::debug!("IPC client disconnected from {:?}", peer);
    }

    /// Listener 는 accept polling 을 위해 non-blocking 이지만, 각 연결의
    /// request-response 루프는 blocking I/O 를 요구한다. peer addr 로그 + blocking
    /// 전환 + reader/writer 분리(같은 소켓의 clone)를 담당. 실패 시 이미 로그를
    /// 남기고 `None` 반환(호출자는 그대로 연결을 종료).
    fn prepare_stream(
        stream: std::net::TcpStream,
    ) -> Option<(
        BufReader<std::net::TcpStream>,
        std::net::TcpStream,
        Option<std::net::SocketAddr>,
    )> {
        let peer = stream.peer_addr().ok();
        tracing::debug!("IPC client connected from {:?}", peer);

        if !Self::configure_socket(&stream) {
            return None;
        }

        let reader = BufReader::new(match stream.try_clone() {
            Ok(s) => s,
            Err(_) => return None,
        });
        let writer = stream;
        Some((reader, writer, peer))
    }

    /// 새 연결 소켓의 옵션을 건다. blocking 전환 실패는 치명적(`false` → 연결 종료),
    /// `TCP_NODELAY` 실패는 지연만 늘어날 뿐이라 로그만 남기고 계속한다.
    ///
    /// Nagle 을 끄는 이유: 이 소켓이 나르는 것은 요청-응답 한 줄 또는 attach 프레임
    /// 한 개이고, 둘 다 "한 번 보내고 상대 응답을 기다리는" 상호작용 단위다 — Nagle 이
    /// 켜져 있으면 세그먼트가 쪼개진 순간 상대의 delayed ACK(~40ms)까지 뒷조각이
    /// 붙잡혀 매 상호작용에 그만큼이 그대로 얹힌다(attach 는 입력·출력 양방향이라
    /// 왕복당 2 회). 상세: `docs/dev-guide/attach-behavior.md` "프레임 전송 지연".
    fn configure_socket(stream: &std::net::TcpStream) -> bool {
        if let Err(e) = stream.set_nonblocking(false) {
            tracing::warn!("Failed to set stream to blocking mode: {}", e);
            return false;
        }
        if let Err(e) = stream.set_nodelay(true) {
            tracing::warn!("Failed to set TCP_NODELAY on IPC stream: {}", e);
        }
        true
    }

    /// `reader` 에서 한 줄을 [`MAX_REQUEST_LINE_BYTES`] 안에서 읽는다.
    ///
    /// 상한 초과를 **거절 응답 없이 연결 종료**로 처리하는 이유는 둘이다. 하나는 이
    /// 상한이 wire 를 바꾸지 않는다는 것이고, 다른 하나가 더 중요하다 — 초과한 줄의
    /// **나머지가 소켓에 그대로 남아 있다.** 잘린 JSON 에 parse error 를 돌려주고 계속
    /// 읽는 기존 갈래(`send_parse_error` 는 연결을 유지한다)를 그대로 쓰면 한 줄이 여러
    /// 요청으로 쪼개져 들어가고, 상한은 다시 없는 것이 된다.
    ///
    /// `R` 로 일반화한 이유는 시험 때문이다 — 상한 판정 자체는 소켓과 무관한데,
    /// `BufReader<TcpStream>` 으로 못 박으면 그 판정을 재려고 실제 연결을 띄워야 한다.
    fn read_line_capped<R: BufRead>(
        reader: &mut R,
        line: &mut String,
        peer: Option<std::net::SocketAddr>,
    ) -> LineRead {
        match reader
            .by_ref()
            .take(MAX_REQUEST_LINE_BYTES as u64)
            .read_line(line)
        {
            Ok(0) => LineRead::Eof,
            // 상한을 정확히 채웠는데 개행이 없다 = 줄이 상한보다 길다. 상한이 개행에서
            // 딱 끝난 경우는 정상이므로 길이만으로 판정하지 않는다.
            Ok(n) if n == MAX_REQUEST_LINE_BYTES && !line.ends_with('\n') => {
                tracing::warn!(
                    "IPC request line from {:?} exceeded {} bytes — closing the connection",
                    peer,
                    MAX_REQUEST_LINE_BYTES
                );
                LineRead::TooLong
            }
            Ok(_) => LineRead::Line,
            Err(e) => {
                tracing::warn!("IPC read error from {:?}: {}", peer, e);
                LineRead::Failed
            }
        }
    }

    /// 스트리밍 업그레이드 판별을 위해 첫 줄을 수동으로 읽는다 — `BufReader` 가
    /// 그 뒤에 이미 버퍼링된 바이트(업그레이드 시 핸드셰이크 뒤에 오는 바이너리
    /// 프레임의 시작)를 보존하게 하기 위함. EOF/에러는 이미 로그 후 `None`.
    fn read_first_line(
        reader: &mut BufReader<std::net::TcpStream>,
        peer: Option<std::net::SocketAddr>,
    ) -> Option<String> {
        let mut line = String::new();
        match Self::read_line_capped(reader, &mut line, peer) {
            LineRead::Line => Some(line),
            LineRead::Eof => {
                tracing::debug!("IPC client disconnected (eof) {:?}", peer);
                None
            }
            // 둘 다 이미 warn 을 남겼다 — 호출자는 연결을 종료한다.
            LineRead::TooLong | LineRead::Failed => None,
        }
    }

    /// 일반 request-response 연결: 이미 읽은 첫 줄을 처리한 뒤, 연결이 닫히거나
    /// 처리가 `false` 를 반환할 때까지 계속 다음 줄을 읽어 처리한다.
    fn run_request_response_loop(
        reader: &mut BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        line: &mut String,
        cmd_tx: &mpsc::Sender<IpcCommand>,
        waker: &Option<IpcWaker>,
        peer: Option<std::net::SocketAddr>,
    ) {
        if !Self::process_request_line(line, cmd_tx, waker, writer, peer) {
            return;
        }
        loop {
            line.clear();
            match Self::read_line_capped(reader, line, peer) {
                LineRead::Line => {}
                // EOF·상한 초과·오류 모두 이 연결의 끝이다(뒤의 둘은 이미 warn 을 남겼다).
                LineRead::Eof | LineRead::TooLong | LineRead::Failed => break,
            }
            if !Self::process_request_line(line, cmd_tx, waker, writer, peer) {
                break;
            }
        }
    }

    /// Drive an upgraded streaming connection: a write thread drains the client's
    /// push sink to the socket, while this thread reads inbound frames and
    /// forwards them to the main loop. Returns when the client detaches or the
    /// socket closes.
    ///
    /// No authentication: the streaming channel trusts SSH + 127.0.0.1 loopback
    /// (decisions.md #5). `session_token` in the handshake is ignored.
    fn handle_stream_connection(
        mut reader: BufReader<std::net::TcpStream>,
        writer: std::net::TcpStream,
        req: JsonRpcRequest,
        ctx: StreamContext,
        peer: Option<std::net::SocketAddr>,
    ) {
        Self::arm_stream_read_timeout(&reader);

        let client_id = ctx.hub.alloc_id();
        let sink_rx = ctx.hub.register(client_id);
        tracing::debug!("stream client {} upgraded from {:?}", client_id, peer);

        let handshake = Self::parse_stream_handshake(&ctx, client_id, req);
        let write_handle = Self::spawn_stream_write_thread(writer, sink_rx);
        if !Self::validate_stream_proto(&ctx, client_id, &handshake, peer) {
            // 점유를 잡지 않은 채 연결만 정리한다 — attach dispatch 로 가지 않는다.
            Self::finish_stream_connection(&ctx, client_id, write_handle, peer);
            return;
        }
        Self::push_stream_ack(&ctx, client_id);
        Self::dispatch_stream_attach(&ctx, client_id, &handshake);
        Self::run_stream_read_loop(&ctx, client_id, &mut reader);
        Self::finish_stream_connection(&ctx, client_id, write_handle, peer);
    }

    /// 조용한(FIN/RST 없는) 네트워크 단절 감지용 read timeout. reader/writer 는 같은
    /// 소켓의 clone(둘 다 원래 `stream`에서 파생)이라 옵션이 공유돼 write 쪽에는 영향
    /// 없다 — read 만 타임아웃 대상. write thread가 이 주기 이내에 Ping 을 흘려
    /// idle 세션에서도 상대측 read timeout 이 갱신되게 한다.
    fn arm_stream_read_timeout(reader: &BufReader<std::net::TcpStream>) {
        if let Err(e) = reader
            .get_ref()
            .set_read_timeout(Some(stream::HEARTBEAT_TIMEOUT))
        {
            tracing::warn!("stream client: failed to set read timeout: {e}");
        }
    }

    /// 핸드셰이크의 프로토콜 버전을 검증한다. 맞으면 `true`(정상 진행), 다르면
    /// **거절 ack**(`ok:false`)을 밀어 넣고 `false` 를 돌려준다.
    ///
    /// **이 검증이 attach dispatch 앞에 있어야 하는 이유**: attach 점유는 핸드셰이크
    /// params 만 보고 잡힌다(`dispatch_stream_attach` → `attach_workspace_for_stream`).
    /// 프로토콜이 안 맞는 client 는 그 점유를 **쓸 수 없는데도** 잡게 되고, 서버는
    /// 그 client 가 연결을 닫아 주기 전까지(구버전/hung peer 면 heartbeat TTL 만료
    /// 20 초) 그 workspace 를 붙잡아 정상 attach 를 `already_attached` 로 거절한다.
    /// 버전 불일치는 원격 attach 의 흔한 실패 경로라, 실패가 확정된 시점에 점유를
    /// 아예 잡지 않는 것이 유일하게 확실한 처리다. 근거:
    /// `docs/adr/0116-attach-handshake-validated-before-occupancy.md`.
    ///
    /// 거절은 프로토콜에 이미 있는 모양을 쓴다 — `StreamAck{ok:false, error}` 는
    /// client(`StreamConnection::open_with`)가 이미 검사해 그 `error` 문구로
    /// bail 하므로, 새 wire 형식 없이 실패 사유가 사용자에게 그대로 전달된다.
    fn validate_stream_proto(
        ctx: &StreamContext,
        client_id: StreamClientId,
        handshake: &StreamHandshake,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        if handshake.proto == stream::STREAM_PROTO {
            return true;
        }
        tracing::warn!(
            "stream client {client_id} from {peer:?}: proto {} != server {} — attach 를 dispatch 하지 않고 거절합니다(점유 미획득).",
            handshake.proto,
            stream::STREAM_PROTO,
        );
        let ack = StreamAck {
            ok: false,
            client_id: Some(client_id),
            proto: stream::STREAM_PROTO,
            error: Some(format!(
                "unsupported stream proto {} (server speaks {})",
                handshake.proto,
                stream::STREAM_PROTO
            )),
        };
        let ack_bytes = serde_json::to_vec(&ack).unwrap_or_default();
        let _ = ctx // best-effort 거절 ack — client 가 이미 끊겼으면 무해(연결은 바로 정리된다).
            .hub
            .push(client_id, StreamFrame::new(StreamTag::Control, ack_bytes));
        false
    }

    /// Handshake ack — pushed through the sink so the single write thread owns
    /// all socket writes.
    fn push_stream_ack(ctx: &StreamContext, client_id: StreamClientId) {
        let ack = StreamAck {
            ok: true,
            client_id: Some(client_id),
            proto: stream::STREAM_PROTO,
            error: None,
        };
        let ack_bytes = serde_json::to_vec(&ack).unwrap_or_default();
        let _ = ctx // best-effort ack push — PushResult(Result 아님) 무시: client 끊겼으면 무해.
            .hub
            .push(client_id, StreamFrame::new(StreamTag::Control, ack_bytes));
    }

    /// read loop 종료 후 정리: sink 등록 해제(write thread 자연 종료) → join →
    /// 메인루프에 disconnect 통지(attach lock 해제 best-effort).
    fn finish_stream_connection(
        ctx: &StreamContext,
        client_id: StreamClientId,
        write_handle: thread::JoinHandle<()>,
        peer: Option<std::net::SocketAddr>,
    ) {
        ctx.hub.unregister(client_id); // drops the sink sender → write thread exits
        let _ = write_handle.join(); // writer 스레드 join 실패(패닉) 무시 — 종료 경로
        // Notify the main loop so it releases any attach locks this client held
        // (attach/detach step 3). Best-effort: if the main loop is gone, nothing
        // to release anyway.
        if ctx
            .inbound_tx
            .send(StreamInbound::Disconnected { client_id })
            .is_ok()
        {
            (ctx.waker)();
        }
        tracing::debug!("stream client {} disconnected from {:?}", client_id, peer);
    }

    /// attach 대상을 핸드셰이크 params 에서 추출. surface(단계 4) 또는 workspace
    /// (단계 6) 둘 중 하나. bulk 전용 연결(ADR-0054)이면 hub 에 결속을 등록한다 —
    /// 이 연결은 mirror/attach 를 하지 않고(= holder 가 되지 않고) 파일 청크만
    /// 나른다. 여기서 hub 에 bulk 로 태깅하면 read 루프가 프레임을 보내기 전에
    /// 결속이 서므로, 이후 pump_inbound 가 이 연결의 Data 를 파일 청크로
    /// 분류한다(연결-단위 태깅). attach 분기와 상호배타.
    fn parse_stream_handshake(
        ctx: &StreamContext,
        client_id: StreamClientId,
        req: JsonRpcRequest,
    ) -> StreamHandshake {
        let open_params = serde_json::from_value::<stream::StreamOpenParams>(req.params).ok();
        let attach_target = open_params.as_ref().and_then(|p| p.target);
        let attach_workspace = open_params.as_ref().and_then(|p| p.target_workspace);
        let bulk_workspace = open_params.as_ref().and_then(|p| p.bulk_workspace);
        if let Some(ws) = bulk_workspace {
            ctx.hub.register_bulk(client_id, ws);
        }
        StreamHandshake {
            proto: open_params.as_ref().map(|p| p.proto).unwrap_or_default(),
            attach_target,
            attach_workspace,
        }
    }

    /// Write thread: drain the push sink (fed by the main loop) to the socket.
    /// `recv_timeout` 대신 blocking iterator 를 쓰던 옛 구현은 sink 가 idle 이면
    /// 소켓에 아무것도 안 나가 client 쪽 read timeout 이 결국 만료된다 — sink 가
    /// HEARTBEAT_INTERVAL 동안 조용하면 빈 Ping 프레임을 대신 흘려보낸다. 실제
    /// Data/Control 트래픽이 있으면 그 자체가 liveness 라 Ping 은 나가지 않는다.
    fn spawn_stream_write_thread(
        writer: std::net::TcpStream,
        sink_rx: mpsc::Receiver<StreamFrame>,
    ) -> thread::JoinHandle<()> {
        let mut w = writer;
        thread::spawn(move || {
            loop {
                match sink_rx.recv_timeout(stream::HEARTBEAT_INTERVAL) {
                    Ok(frame) => {
                        if stream::write_frame(&mut w, frame.tag, &frame.payload).is_err() {
                            break;
                        }
                        if frame.tag == StreamTag::Detach {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if stream::write_frame(&mut w, StreamTag::Ping, &[]).is_err() {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break, // unregister 됨
                }
            }
        })
    }

    /// attach 요청이면 메인루프로 위임(엔진은 메인루프 단일소유 → accept thread 가
    /// 직접 acquire 불가). 메인루프가 lock 획득 + 스냅샷 push + 출력 tap 결선한다.
    /// attach 결과(성공/거부)는 별도 Control 프레임으로 client 에 통지된다.
    /// workspace 우선(둘 다 지정은 비정상이지만 안전 분기).
    fn dispatch_stream_attach(
        ctx: &StreamContext,
        client_id: StreamClientId,
        handshake: &StreamHandshake,
    ) {
        if let Some(target_workspace_id) = handshake.attach_workspace {
            if ctx
                .inbound_tx
                .send(StreamInbound::AttachWorkspaceRequest {
                    client_id,
                    target_workspace_id,
                })
                .is_ok()
            {
                (ctx.waker)();
            }
        } else if let Some(target_surface_id) = handshake.attach_target
            && ctx
                .inbound_tx
                .send(StreamInbound::AttachRequest {
                    client_id,
                    target_surface_id,
                })
                .is_ok()
        {
            (ctx.waker)();
        }
    }

    /// Read loop: forward inbound frames to the main loop (which echoes them
    /// back in debug builds; later steps interpret them as input/resize).
    fn run_stream_read_loop(
        ctx: &StreamContext,
        client_id: StreamClientId,
        reader: &mut BufReader<std::net::TcpStream>,
    ) {
        loop {
            match stream::read_frame(reader) {
                Ok(frame) if frame.tag == StreamTag::Detach => break,
                Ok(frame) => {
                    if ctx
                        .inbound_tx
                        .send(StreamInbound::Frame { client_id, frame })
                        .is_err()
                    {
                        break; // main loop gone
                    }
                    (ctx.waker)();
                }
                Err(_) => break, // EOF / oversize / unknown tag
            }
        }
    }

    /// Handle one request line of a request-response connection. Returns `false`
    /// when the connection should be torn down (send/recv/write failure), `true`
    /// to keep reading (including for empty or unparseable lines).
    fn process_request_line(
        line: &str,
        cmd_tx: &mpsc::Sender<IpcCommand>,
        waker: &Option<IpcWaker>,
        writer: &mut std::net::TcpStream,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return true;
        }

        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                Self::send_parse_error(writer, e);
                return true;
            }
        };

        Self::dispatch_and_await(request, cmd_tx, waker, writer, peer)
    }

    /// JSON 한 줄을 쓰고 flush 를 시도한다(write 가 실패해도 flush 는 그대로
    /// 시도된다 — 두 결과를 각각 반환해 호출자가 로깅/제어흐름을 결정한다).
    fn write_json_line(
        writer: &mut std::net::TcpStream,
        json: &str,
    ) -> (std::io::Result<()>, std::io::Result<()>) {
        let write_result = writeln!(writer, "{}", json);
        let flush_result = writer.flush();
        (write_result, flush_result)
    }

    /// JSON 파싱 실패 시 JSON-RPC parse error(-32700) 응답을 회신한다. 클라이언트
    /// 로 향한 응답이라 실패해도 연결은 끊지 않는다(trace 로그만).
    fn send_parse_error(writer: &mut std::net::TcpStream, e: serde_json::Error) {
        let err_resp = JsonRpcResponse::error(
            serde_json::Value::Null,
            -32700,
            format!("Parse error: {}", e),
        );
        let json = serde_json::to_string(&err_resp).unwrap();
        let (write_result, flush_result) = Self::write_json_line(writer, &json);
        if let Err(e) = write_result {
            tracing::trace!("IPC parse-error response write failed: {e}");
        }
        if let Err(e) = flush_result {
            tracing::trace!("IPC parse-error response flush failed: {e}");
        }
    }

    /// 요청을 메인 스레드로 보내고 응답을 기다려 클라이언트로 회신한다. 반환값은
    /// 연결 유지 여부.
    fn dispatch_and_await(
        request: JsonRpcRequest,
        cmd_tx: &mpsc::Sender<IpcCommand>,
        waker: &Option<IpcWaker>,
        writer: &mut std::net::TcpStream,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        let (resp_tx, resp_rx) = mpsc::sync_channel(1);

        let cmd = IpcCommand {
            request,
            response_tx: resp_tx,
        };

        // Send command to main thread
        if cmd_tx.send(cmd).is_err() {
            tracing::warn!("IPC cmd_tx.send failed (main thread shut down?)");
            return false;
        }

        // Wake the event loop so it processes the command immediately
        if let Some(waker) = waker {
            waker();
        }

        // Wait for response from main thread
        match resp_rx.recv() {
            Ok(response) => Self::write_dispatch_response(writer, &response, peer),
            Err(e) => {
                tracing::warn!(
                    "IPC resp_rx.recv failed: {} (response_tx dropped without sending)",
                    e
                );
                false
            }
        }
    }

    /// 메인 스레드가 만든 응답을 직렬화해 클라이언트로 회신. 반환값은 연결 유지 여부.
    fn write_dispatch_response(
        writer: &mut std::net::TcpStream,
        response: &JsonRpcResponse,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        let json = serde_json::to_string(response).unwrap();
        let (write_result, flush_result) = Self::write_json_line(writer, &json);
        if !Self::log_dispatch_write_result(write_result, flush_result) {
            return false;
        }
        tracing::debug!("IPC response sent for {:?}", peer);
        true
    }

    /// write/flush 결과를 각각 확인해 실패 시 warn 로그. 둘 다 성공해야 `true`.
    fn log_dispatch_write_result(
        write_result: std::io::Result<()>,
        flush_result: std::io::Result<()>,
    ) -> bool {
        if let Err(e) = write_result {
            tracing::warn!("IPC write error: {}", e);
            return false;
        }
        if let Err(e) = flush_result {
            tracing::warn!("IPC flush error: {}", e);
            return false;
        }
        true
    }

    /// Get the effective port file path for this instance.
    fn effective_port_file_path(&self) -> Option<std::path::PathBuf> {
        self.custom_port_file
            .clone()
            .or_else(port_file::port_file_path)
    }
}

impl IpcServerPort for TcpIpcServer {
    fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError> {
        self.command_rx.try_recv()
    }

    fn port(&self) -> u16 {
        self.port
    }

    fn command_sender(&self) -> mpsc::Sender<IpcCommand> {
        self.command_tx.clone()
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    fn read(input: &[u8]) -> (LineRead, String) {
        let mut reader = BufReader::new(input);
        let mut line = String::new();
        let outcome = TcpIpcServer::read_line_capped(&mut reader, &mut line, None);
        (outcome, line)
    }

    // 평범한 한 줄은 개행까지 그대로 읽힌다. 뒤에 다른 줄이 있어도 한 줄에서 멈춘다.
    #[test]
    fn a_normal_line_is_read_whole() {
        let (outcome, line) = read(b"{\"method\":\"system.info\"}\nnext\n");
        assert!(matches!(outcome, LineRead::Line));
        assert_eq!(line, "{\"method\":\"system.info\"}\n");
    }

    // 빈 입력은 상한 초과가 아니라 EOF 다.
    #[test]
    fn eof_is_told_apart_from_a_refusal() {
        let (outcome, line) = read(b"");
        assert!(matches!(outcome, LineRead::Eof));
        assert!(line.is_empty());
    }

    // 개행 없이 상한을 넘기면 거절한다 — 이것이 없으면 peer 하나가 호스트 메모리를
    // 끝까지 먹는다.
    #[test]
    fn a_line_longer_than_the_cap_is_refused() {
        let input = vec![b'a'; MAX_REQUEST_LINE_BYTES + 1];
        let (outcome, _) = read(&input);
        assert!(matches!(outcome, LineRead::TooLong));
    }

    // 경계: 개행이 상한의 **마지막 바이트**에 딱 오는 줄은 정상이다. 길이만으로
    // 판정하면(`n == 상한` 이면 무조건 거절) 이 줄이 거절된다.
    #[test]
    fn a_line_that_ends_exactly_at_the_cap_is_not_refused() {
        let mut input = vec![b'a'; MAX_REQUEST_LINE_BYTES - 1];
        input.push(b'\n');
        assert_eq!(input.len(), MAX_REQUEST_LINE_BYTES);
        let (outcome, line) = read(&input);
        assert!(
            matches!(outcome, LineRead::Line),
            "상한에서 개행으로 끝나는 줄은 초과가 아니다"
        );
        assert_eq!(line.len(), MAX_REQUEST_LINE_BYTES);
    }

    fn slots() -> (Arc<AtomicUsize>, AtomicBool) {
        (Arc::new(AtomicUsize::new(0)), AtomicBool::new(false))
    }

    // 자리는 잡히고, 스레드가 어떻게 끝나든 Drop 이 돌려준다.
    #[test]
    fn a_slot_is_returned_when_it_drops() {
        let (live, sat) = slots();
        {
            let _held = ConnectionSlot::try_acquire(&live, &sat).expect("첫 자리는 잡힌다");
            assert_eq!(live.load(Ordering::Relaxed), 1);
        }
        assert_eq!(
            live.load(Ordering::Relaxed),
            0,
            "Drop 이 자리를 반납해야 한다"
        );
    }

    // 상한을 넘는 요청은 거절되고, **거절이 계수를 밀어 올리지 않는다.**
    // try_acquire 는 먼저 fetch_add 하고 초과면 되돌리는데, 그 되돌림이 빠지면
    // 계수가 영구히 상한 위로 떠서 자리가 다시는 안 열린다.
    #[test]
    fn a_refusal_does_not_leak_the_count() {
        let (live, sat) = slots();
        let held: Vec<_> = (0..MAX_CONCURRENT_CONNECTIONS)
            .map(|_| ConnectionSlot::try_acquire(&live, &sat).expect("상한까지는 잡힌다"))
            .collect();
        assert_eq!(live.load(Ordering::Relaxed), MAX_CONCURRENT_CONNECTIONS);

        assert!(
            ConnectionSlot::try_acquire(&live, &sat).is_none(),
            "상한을 넘는 연결은 거절된다"
        );
        assert_eq!(
            live.load(Ordering::Relaxed),
            MAX_CONCURRENT_CONNECTIONS,
            "거절은 계수를 그대로 두어야 한다"
        );
        drop(held);
        assert_eq!(live.load(Ordering::Relaxed), 0);
    }

    // 자리가 하나 반납되면 다음 연결이 들어온다 — 상한이 영구 차단이 아니다.
    #[test]
    fn a_returned_slot_lets_the_next_one_in() {
        let (live, sat) = slots();
        let mut held: Vec<_> = (0..MAX_CONCURRENT_CONNECTIONS)
            .map(|_| ConnectionSlot::try_acquire(&live, &sat).expect("상한까지는 잡힌다"))
            .collect();
        assert!(ConnectionSlot::try_acquire(&live, &sat).is_none());

        held.pop();
        assert!(
            ConnectionSlot::try_acquire(&live, &sat).is_some(),
            "반납된 자리로 다음 연결이 들어와야 한다"
        );
    }

    // 상한이 어디서 나왔는지를 값으로 고정한다. 호스트가 받아들이는 가장 큰 요청
    // payload 는 저장소 항목 하나이고, 그것이 한 줄에 실릴 때의 최악 팽창은 JSON
    // escape 의 6 배다. 상한을 그 아래로 내리면 **지금 통과하는 memory.set 이 거절된다.**
    #[test]
    fn the_cap_clears_the_largest_payload_the_store_accepts() {
        let worst_case_on_the_wire = tasty_memory::MAX_VALUE_BYTES * 6;
        assert!(
            MAX_REQUEST_LINE_BYTES >= worst_case_on_the_wire,
            "상한 {MAX_REQUEST_LINE_BYTES} 가 저장소 상한의 escape 최악치 \
             {worst_case_on_the_wire} 보다 작다 — 정상 요청을 거절하게 된다"
        );
    }
}

#[cfg(test)]
mod notify_cleanup_tests {
    use super::*;

    // 더미 파일이 든 notify/ 를 clear 하면 디렉토리가 통째로 사라진다.
    #[test]
    fn clear_removes_populated_notify_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        std::fs::create_dir_all(&notify).unwrap();
        for id in ["11.log", "42.log", "1337.log"] {
            std::fs::write(notify.join(id), b"spawn done\n").unwrap();
        }
        assert!(notify.exists());

        TcpIpcServer::clear_notify_dir(&notify);

        assert!(!notify.exists(), "notify dir should be removed");
    }

    // 애초에 없는 디렉토리를 clear 해도 에러/패닉 없이 no-op.
    #[test]
    fn clear_is_noop_when_dir_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        assert!(!notify.exists());

        // NotFound 를 삼키므로 패닉 없이 반환해야 한다.
        TcpIpcServer::clear_notify_dir(&notify);

        assert!(!notify.exists());
    }

    // fire-and-forget spawn 경로도 스레드를 join 해 실제로 비워지는지 확인.
    #[test]
    fn spawned_thread_clears_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        std::fs::create_dir_all(&notify).unwrap();
        std::fs::write(notify.join("7.log"), b"x\n").unwrap();

        // spawn_notify_dir_cleanup 은 tasty_home() 에 의존하므로 여기선 직접
        // 스레드를 띄워 프로덕션과 동일한 fire-and-forget 경로(spawn→clear)를
        // 재현하고, 테스트에서만 join 으로 완료를 기다린다.
        let dir = notify.clone();
        let handle = thread::spawn(move || TcpIpcServer::clear_notify_dir(&dir));
        handle.join().unwrap();

        assert!(!notify.exists());
    }
}

impl Drop for TcpIpcServer {
    fn drop(&mut self) {
        // 종료 Drop tail 계측(S5d). 저비용이지만 **시점**이 중요하다 —
        // `~/.tasty/tasty.port` 제거는 오직 여기서만 일어나므로, Drop tail 이 길면
        // 그만큼 stale 포트 파일이 남는 시간이 길어진다. 그 창을 로그로 재는 게
        // 이 마커의 목적이다.
        let t_drop = std::time::Instant::now();
        // Signal the accept thread to stop
        self.shutdown.store(true, Ordering::Relaxed);
        // Clean up port file. 파일이 이미 사라졌거나 권한이 없는 케이스도 정상 종료
        // 흐름에서 발생 가능 — trace 레벨로만 기록한다.
        if let Some(path) = self.effective_port_file_path()
            && let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::trace!("port file {} remove failed: {e}", path.display());
        }
        tracing::info!(
            target: "tasty::shutdown",
            ms = t_drop.elapsed().as_secs_f64() * 1000.0,
            "S5d ipc_server_drop (accept stop + port file 제거)"
        );
    }
}

#[cfg(test)]
mod handshake_tests {
    use std::sync::Arc;
    use std::sync::mpsc;

    use super::*;
    use crate::adapters::production::stream_hub::StreamHub;

    fn ctx() -> (StreamContext, mpsc::Receiver<StreamInbound>) {
        let (inbound_tx, inbound_rx) = mpsc::channel();
        let ctx = StreamContext {
            hub: StreamHub::new(),
            inbound_tx,
            waker: Arc::new(|| {}),
        };
        (ctx, inbound_rx)
    }

    /// 서버와 같은 proto 는 그대로 통과하고, 거절 ack 를 밀어 넣지 않는다.
    #[test]
    fn matching_proto_passes_without_pushing_a_rejection() {
        let (ctx, _rx) = ctx();
        let client_id = ctx.hub.alloc_id();
        let sink = ctx.hub.register(client_id);
        let hs = StreamHandshake {
            proto: stream::STREAM_PROTO,
            attach_target: None,
            attach_workspace: Some(7),
        };

        assert!(TcpIpcServer::validate_stream_proto(
            &ctx, client_id, &hs, None
        ));
        assert!(
            sink.try_recv().is_err(),
            "정상 proto 에는 거절 ack 가 나가면 안 된다"
        );
    }

    /// proto 가 다르면 거절 ack(`ok:false` + 사유)를 밀고 `false` — 호출부가 attach
    /// dispatch 를 건너뛰므로 점유가 잡히지 않는다.
    #[test]
    fn mismatched_proto_is_rejected_with_an_error_ack() {
        let (ctx, _rx) = ctx();
        let client_id = ctx.hub.alloc_id();
        let sink = ctx.hub.register(client_id);
        let hs = StreamHandshake {
            proto: stream::STREAM_PROTO + 41,
            attach_target: None,
            attach_workspace: Some(7),
        };

        assert!(!TcpIpcServer::validate_stream_proto(
            &ctx, client_id, &hs, None
        ));

        let frame = sink.try_recv().expect("거절 ack 가 sink 에 실려야 한다");
        assert_eq!(frame.tag, StreamTag::Control);
        let ack: StreamAck = serde_json::from_slice(&frame.payload).expect("ack 역직렬화");
        assert!(!ack.ok, "거절은 ok:false 로 표현된다: {ack:?}");
        assert_eq!(
            ack.proto,
            stream::STREAM_PROTO,
            "client 가 서버 버전을 알 수 있어야 한다"
        );
        let err = ack.error.expect("거절 사유가 실려야 한다");
        assert!(
            err.contains(&(stream::STREAM_PROTO + 41).to_string()),
            "사유에 client 가 보낸 값이 있어야 한다: {err}"
        );
    }

    /// proto 필드가 아예 없는 핸드셰이크(구형/오작성 client)는 serde default 로 0 이
    /// 되어 거절된다 — "모르는 버전은 통과" 로 새는 구멍이 없는지 고정한다.
    #[test]
    fn missing_proto_field_defaults_to_zero_and_is_rejected() {
        let params = serde_json::json!({ "target_workspace": 7 });
        let parsed: stream::StreamOpenParams = serde_json::from_value(params).expect("parse");
        assert_eq!(parsed.proto, 0, "생략된 proto 는 0 이다");

        let (ctx, _rx) = ctx();
        let client_id = ctx.hub.alloc_id();
        let _sink = ctx.hub.register(client_id);
        let hs = StreamHandshake {
            proto: parsed.proto,
            attach_target: None,
            attach_workspace: parsed.target_workspace,
        };
        assert!(!TcpIpcServer::validate_stream_proto(
            &ctx, client_id, &hs, None
        ));
    }
}
