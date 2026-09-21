//! Production adapter — `TcpIpcServer`. TCP listener + mpsc channel + accept
//! thread 로 JSON-RPC 요청을 받는다. Hub 가 `Box<dyn IpcServerPort>` 로 보유.
//!
//! wire 타입 (`IpcCommand` / `IpcWaker` / `send_response`) 은 옛
//! 위치 (`crate::ipc::server`) 에 잔존 — wire 형식과 강결합이라 trait 옆이 아니라
//! wire 모듈에 두는 게 자연스럽다 (verify 자율 결정).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use tasty_telemetry::ConnectionStats;

use crate::ipc::port_file;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::ipc::server::{IpcCommand, IpcWaker};
use crate::ipc::stream::{self, StreamAck, StreamFrame, StreamTag};
use crate::ports::ipc_server::IpcServerPort;
use tasty_ipc::admission::{
    CommandAdmission, INJECTED_DEPTH_LIMIT, Origin, QUEUED_BYTES_LIMIT, QueueLimits,
};
use tasty_ipc::stream_hub::{StreamClientId, StreamContext, StreamInbound};

mod first_line;

/// 한 요청 줄이 읽어 들일 수 있는 최대 바이트.
///
/// `read_line` 은 개행을 만날 때까지 `String` 을 키운다. 상한이 없으면 **개행을 끝내
/// 보내지 않는 peer 하나가** 호스트 메모리를 끝까지 먹는다. 스트림 업그레이드 경로에는
/// 대응하는 상한이 이미 둘 있다 — 프레임 길이 접두사를 자르는 [`stream::MAX_FRAME_LEN`]
/// 과 [`TcpIpcServer::arm_stream_read_timeout`] 의 read timeout. 일반 RPC 경로에만
/// 없었고, 이 상수가 그 비대칭을 없앤다.
///
/// 값의 근거: 호스트가 받아들이는 가장 큰 요청 payload 는 `memory.set` 의 값이고 그
/// 상한은 `MemoryConfig::entry_max_bytes` 다. **그 값은 상수가 아니라 설정값이다** —
/// `~/.tasty/config.toml` 의 `[memory] entry_max_mb`(`MemorySettings::default()` 는 1)가
/// 부팅 때 바이트로 환산돼 들어간다. 그 값이 한 줄에 실릴 때의 팽창률은 두 갈래다 —
/// `value_b64` 는 base64 라 4/3 배, `value` 문자열은 JSON escape 가 최악에 바이트당
/// `\u00XX` 6 자라 6 배. 그래서 **기본 설정에서** 정상 요청의 상한은 6 MiB + 봉투이고
/// 8 MiB 는 그 위의 여유다.
///
/// ★ 이 파생은 기본값에서만 성립한다. 사용자가 `entry_max_mb` 를 올리면 저장소는 그
/// 크기를 정상으로 받아들이는데 이 상한은 안 따라 올라간다 — escape 최악 갈래에서
/// `entry_max_mb = 2`, base64 갈래에서 `entry_max_mb = 7` 부터 합법적인 `memory.set` 한
/// 줄이 여기 걸려 `ERR_REQUEST_LINE_TOO_LONG` 한 줄을 받고 **연결이 닫힌다** — 사유는
/// 이제 말하지만 그 요청이 못 들어간다는 것은 그대로다. 두 수를 잇는 배선은 없다(그
/// 배선은 동작 변경이라 별건이다). 갈라졌는지 재는 법은 아래
/// `admission_tests::the_cap_clears_the_largest_payload_the_store_accepts` 가
/// `MemorySettings::default()` 를 좌변으로 읽는 것이다 — 기본값이 올라가면 그 시험이
/// 죽는다. 사용자가 config 로 올린 값은 컴파일 시점에 안 보이므로 **어떤 시험도 못
/// 잡는다.**
const MAX_REQUEST_LINE_BYTES: usize = 8 * 1024 * 1024;

/// 한 줄 읽기의 결과. `read_line` 의 `Ok(n)` 하나로 뭉뚱그려지던 것을 갈래로 나눈다 —
/// 특히 **상한 초과**는 정상적으로 읽은 줄과 구분돼야 한다.
enum LineRead {
    /// 개행까지 한 줄을 읽었다.
    Line,
    /// peer 가 연결을 닫았다.
    Eof,
    /// 개행 없이 [`MAX_REQUEST_LINE_BYTES`] 를 채웠다 — 거절을 응답으로 알리고 닫는다.
    TooLong,
    /// 읽기 기한이 지났다. 기한은 첫 줄에만 걸리므로([`first_line`]) 이 갈래는 첫 줄
    /// 자리에서만 난다 — 거절을 응답으로 알리고 닫는다.
    Idle,
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
/// 보정 신호다(닿는 순간 `warn` 이 한 줄 남는다). 그 보정 신호를 로그가 아니라 값으로
/// 읽는 자리가 `system.pressure` 의 `connections` 덩어리다 — 이 상수가 그 응답의
/// `limit` 이고, 그래서 `pub(crate)` 다.
pub(crate) const MAX_CONCURRENT_CONNECTIONS: usize = 256;

/// 한 dispatch 회차가 큐에서 집어 드는 IPC 명령 수의 상한.
///
/// 이 상한이 없으면 회차의 길이를 큐가 정한다 — 두 drain 자리(`src/app/ipc.rs` 의
/// `process_ipc`, `src/boot/headless_dispatch.rs` 의 `pump_ipc`)가 둘 다 "빌 때까지
/// 모아서 전부 처리" 라, 보내는 쪽이 계속 밀어 넣으면 같은 회차가 끝나지 않고
/// 타이머·터미널 출력·창 이벤트가 그만큼 밀린다.
///
/// **값을 고르지 않고 [`MAX_CONCURRENT_CONNECTIONS`] 에서 파생한다.** 요청을 넣는
/// 쪽은 둘 다 **응답을 받을 때까지 블록한다**(`dispatch_and_await` 와
/// `tasty_ipc::host_call::HostIpcInjector::dispatch`). 그래서 살아 있는 연결 하나가
/// 큐에 동시에 올려 둘 수 있는 명령은 최대 하나이고, 연결 수는 저 상한이 자른다 —
/// 즉 이 값이 그 상한과 같으면 **TCP 쪽만으로는 회차가 잘릴 수 없다.** 잘릴 수 있는
/// 것은 호스트 자신이 주입한 몫뿐이고, 그것은 정상적으로 회수된다(아래).
///
/// **남은 것은 다음 회차가 집는다 — 별도 배선이 없다.** 두 생산자 모두 `send` 직후
/// waker 를 **정확히 한 번** 부르므로 N 개를 넣으면 wake 도 N 개가 큐에 들어간다.
/// 한 회차가 B(<N) 개만 집어 들면 남은 N-B 개의 wake 가 그대로 남아 루프를 다시
/// 들여보낸다. 이 성질은 `tests/e2e_tests.rs` 의
/// `concurrent_requests_are_all_answered` 가 잰다 — 다만 그 시험이 여는 연결은
/// 이 상한보다 적으므로, 실제로 재려면 이 값을 낮춰서 돌려야 한다(그 시험의 주석에
/// 실측을 적어 두었다).
pub(crate) const DRAIN_BUDGET_PER_ROUND: usize = MAX_CONCURRENT_CONNECTIONS;

/// 한 응답 줄을 소켓에 밀어 넣는 데 허용되는 최대 시간.
///
/// 이 값이 없으면 `write_json_line` 의 `writeln!` + `flush` 가 **무한정 블록한다.**
/// 응답을 안 읽는 peer 가 소켓 송신 버퍼를 채우면 그 연결 스레드가 거기서 멈추고,
/// 멈춘 스레드는 [`ConnectionSlot`] 을 계속 쥔다(자리는 `Drop` 에서만 돌아온다). 그런
/// peer 가 [`MAX_CONCURRENT_CONNECTIONS`] 만큼이면 정상 client 가 못 붙는다 — 즉 수신
/// 쪽 상한이 센 자리가 **쓰기 쪽에 상한이 없어서** 영구 점유된다.
///
/// **값을 고르지 않고 [`stream::HEARTBEAT_TIMEOUT`] 에서 파생한다.** 그 상수가 이미
/// 답하는 물음이 같다 — "이 loopback 소켓의 상대가 얼마나 진척을 안 내면 죽은 것으로
/// 보는가". 같은 소켓의 attach 쪽은 그 값을 read timeout 으로 걸고
/// ([`TcpIpcServer::arm_stream_read_timeout`]), 그 소켓의 **client 측은 같은 값을 자기
/// write timeout 으로 건다**(`src/app/attach_client.rs`). 그래서 여기서 새 수를 고르면
/// 같은 물음에 답이 둘이 된다. 둘이 갈라져야 할 이유가 생기면 그 이유를 여기 적고
/// 그때 가른다.
const RESPONSE_WRITE_TIMEOUT: Duration = stream::HEARTBEAT_TIMEOUT;

// 명령 큐 입장 상한과 이 파일의 상한들 사이의 관계(ADR-0391). 값은 파생이 아니고 관계만
// 고정한다 — 누가 한쪽을 옮겨 관계가 깨지면 컴파일이 멈춘다.
//
// 바이트: 최대 크기 요청 두 건이 동시에 대기할 수 있어야 하고(아니면 큰 요청 하나 뒤에 다른
// 큰 요청이 늘 거절된다), 연결 상한 × 줄 상한(이 상한 이전의 이론상 최대)보다 작아야 한다
// (아니면 상한이 한 번도 안 걸린다).
const _: () = assert!(QUEUED_BYTES_LIMIT >= 2 * MAX_REQUEST_LINE_BYTES);
const _: () = assert!(QUEUED_BYTES_LIMIT < MAX_CONCURRENT_CONNECTIONS * MAX_REQUEST_LINE_BYTES);
// 주입 깊이: 한 dispatch 회차가 주입 적체를 한 번에 비울 수 있어야 한다.
const _: () = assert!(INJECTED_DEPTH_LIMIT <= DRAIN_BUDGET_PER_ROUND);

/// 연결 스레드가 명령을 올리는 자리 — 큐의 송신단과 그 입장 장부를 한 묶음으로 든다.
///
/// 둘을 따로 넘기면 송신단만 받은 새 경로가 장부를 건너뛰어도 컴파일된다. 묶어 두면 이
/// 파일 안에서 명령을 큐에 넣는 길은 [`TcpIpcServer::dispatch_and_await`] 하나다.
struct CommandQueue {
    tx: mpsc::Sender<IpcCommand>,
    admission: Arc<CommandAdmission>,
}

/// 명령 큐 입장 상한. 제품 값은 [`QueueLimits::DEFAULT`] 이고, debug 빌드에서만 환경변수로
/// 낮출 수 있다 — 상한 하나를 다른 상한과 독립으로 넘겨 보는 시험(격리 인스턴스)이 제품
/// 값(수십 MiB)을 실제로 채우지 않고도 거절 갈래를 재게 하려는 것이다.
#[cfg(debug_assertions)]
fn queue_limits() -> QueueLimits {
    QueueLimits {
        queued_bytes: debug_env_usize("TASTY_DEBUG_IPC_QUEUE_BYTES").unwrap_or(QUEUED_BYTES_LIMIT),
        injected_depth: debug_env_usize("TASTY_DEBUG_IPC_INJECT_DEPTH")
            .unwrap_or(INJECTED_DEPTH_LIMIT),
    }
}

#[cfg(not(debug_assertions))]
fn queue_limits() -> QueueLimits {
    QueueLimits::DEFAULT
}

/// debug 전용 상한 덮어쓰기 값. 없으면 `None`, 숫자가 아니면 경고를 남기고 `None`.
#[cfg(debug_assertions)]
fn debug_env_usize(name: &str) -> Option<usize> {
    let raw = std::env::var(name).ok()?;
    match raw.trim().parse::<usize>() {
        Ok(v) => {
            tracing::warn!("{name}={v} overrides an IPC admission bound (debug build)");
            Some(v)
        }
        Err(e) => {
            tracing::warn!("{name}={raw:?} is not a number, ignored: {e}");
            None
        }
    }
}

/// 살아 있는 연결 하나의 자리. 스레드가 어떻게 끝나든(정상·조기 return·패닉) `Drop`
/// 이 자리를 돌려준다 — 회수를 `handle_connection` 의 제어흐름에 맡기지 않는다.
///
/// 자리를 세는 값은 여기가 아니라 [`ConnectionStats`] 에 있다. 그것이 `Core` 에 붙어
/// 있어야 `system.pressure` 가 **밖에서** 읽을 수 있기 때문이다 — 예전에는 이 수가
/// accept 스레드의 지역 원자값이라 상한에 닿았다는 사실이 `warn` 한 줄로만 나갔고,
/// 그 줄이 지나간 뒤에는 "지금 몇 개가 붙어 있나" 를 물어볼 자리가 없었다.
struct ConnectionSlot {
    stats: Arc<ConnectionStats>,
}

impl ConnectionSlot {
    /// 상한 안이면 자리를 잡고 `Some`, 아니면 되돌리고 `None`.
    ///
    /// `saturated` 는 **로그 폭주를 막기 위한 것**이다. 거절은 상대가 이미 상한만큼
    /// 연결을 쥐고 있을 때만 나는데, 그 상대가 재시도 루프를 돌면 거절마다 warn 한 줄이
    /// 나가 로그 파일을 채운다. 그래서 포화로 **들어가는 순간**만 warn 이고 그 뒤로는
    /// debug 다. 자리가 하나라도 반납되면 다시 warn 할 수 있게 풀린다. 거절 **횟수**는
    /// 이 게이트와 무관하게 게이지가 전부 센다 — 로그를 접는 것이 값을 접으면 안 된다.
    fn try_acquire(stats: &Arc<ConnectionStats>, saturated: &AtomicBool) -> Option<Self> {
        let limit = MAX_CONCURRENT_CONNECTIONS as u64;
        if stats.try_open(limit).is_none() {
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
        Some(Self {
            stats: stats.clone(),
        })
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.stats.close();
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
    /// 명령 큐의 입장 장부. 소켓 경로와 호스트 주입기(`HostIpcInjector`)가 같은 것을 든다.
    admission: Arc<CommandAdmission>,
}

impl TcpIpcServer {
    /// Start the IPC server with an optional custom port file path and waker.
    /// The waker is called whenever an IPC command is enqueued, so the event
    /// loop can wake up and process it immediately.
    /// `connections` 는 **호출자가 들고 있는** 게이지다. 서버가 자기 안에서 만들면
    /// `Core` 가 그것을 못 보므로 `system.pressure` 가 읽을 자리가 없다 — 그래서
    /// 방향이 반대다(`Core` 가 낳고 서버가 채운다). 서버가 안 뜨면 그 게이지는
    /// 아무도 안 올리는 값이 되고, 그때 응답은 `live: 0` 이 아니라 `accepted: 0`
    /// 으로 "연결을 받은 적이 없다" 를 말한다.
    pub fn start_with_port_file(
        port_file_override: Option<String>,
        waker: Option<IpcWaker>,
        stream_ctx: StreamContext,
        connections: Arc<ConnectionStats>,
    ) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();

        tracing::info!("IPC server listening on 127.0.0.1:{}", port);

        let custom_port_file = port_file_override.map(std::path::PathBuf::from);

        Self::clear_notify_then_publish_port(
            Self::notify_dir_to_clear(tasty_utils::path::tasty_home(), custom_port_file.as_deref()),
            port,
            custom_port_file.as_deref(),
        )?;

        // 큐 자체는 무제한 채널이다. 상한은 채널이 아니라 그 앞의 입장 장부가 건다 —
        // `sync_channel` 의 칸 수는 명령 **개수**만 자르고 바이트를 못 보며, 가득 찬 칸에서
        // 송신이 막히면 거절 대신 대기가 된다(ADR-0391).
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let admission = CommandAdmission::new(queue_limits());
        let shutdown = Arc::new(AtomicBool::new(false));

        // Accept connections in a background thread with non-blocking + shutdown check
        let shutdown_clone = shutdown.clone();
        let accept_tx = cmd_tx.clone();
        let accept_admission = admission.clone();
        let first_line_idle = first_line::first_line_idle_timeout();
        // 살아 있는 연결 수 + 포화 로그 게이트. accept 스레드만 읽고 쓰지만 자리 반납은
        // 각 연결 스레드의 `Drop` 이 하므로 공유 소유가 필요하다. 계수는 호출자가 준
        // 게이지에 쌓인다 — 포화 게이트는 로그 전용이라 여기서 만든다.
        let saturated = Arc::new(AtomicBool::new(false));
        listener.set_nonblocking(true)?;
        thread::spawn(move || {
            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let Some(slot) = ConnectionSlot::try_acquire(&connections, &saturated)
                        else {
                            Self::refuse_saturated_connection(stream);
                            continue;
                        };
                        let queue = CommandQueue {
                            tx: accept_tx.clone(),
                            admission: accept_admission.clone(),
                        };
                        let waker = waker.clone();
                        let stream_ctx = stream_ctx.clone();
                        thread::spawn(move || {
                            Self::handle_connection(
                                stream,
                                queue,
                                waker,
                                stream_ctx,
                                slot,
                                first_line_idle,
                            );
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
            admission,
        })
    }

    /// 명령 큐의 입장 장부. 호스트 주입기가 같은 장부로 판정받도록 hub 가 건네준다.
    pub(crate) fn admission(&self) -> Arc<CommandAdmission> {
        self.admission.clone()
    }

    /// 과거 완료 알림 로그를 치운 **뒤에** 포트 파일을 쓴다. 순서가 계약이다.
    ///
    /// 청소하는 이유: surface_id 는 재시작마다 새로 발급되므로 이전 프로세스가 남긴
    /// `notify/<surface>.log` 는 이 인스턴스에선 모두 죽은 surface 의 것이고 읽을
    /// reader 가 없다.
    ///
    /// **순서가 왜 계약인가**: 예전에는 부팅 지연을 피하려고 청소를 join 하지 않는
    /// 스레드로 던지고 포트 파일을 먼저 썼다. 그러면 청소가 도는 도중에 첫 완료 알림이
    /// append 될 수 있고, `remove_dir_all` 이 **방금 쓰인 줄을 지운다.** 포트 파일은 이
    /// 인스턴스의 존재를 알리는 유일한 통로다 — 그것을 쓰기 전에 청소를 끝내 두면 어떤
    /// writer 도 청소와 겹칠 수 없다. 세대 디렉토리를 도입해
    /// `<parent_home>/notify/<caller_surface>.log` 경로 규약을 바꾸지 않고도 경합이 닫힌다.
    ///
    /// 치르는 값은 부팅 경로에 `remove_dir_all` 한 번이다. 지우는 것은 완료 알림 줄
    /// 몇 개가 든 작은 파일들뿐이라 그 비용이 위 경합과 바꿀 만하다고 봤다.
    ///
    /// `notify_dir` 가 `None`(홈 미확인, 또는 포트 파일이 데이터 루트 밖 — [`Self::notify_dir_to_clear`])
    /// 이면 청소는 건너뛰고 포트 파일만 쓴다. 인자로
    /// 받는 이유는 시험이 실제 홈을 건드리지 않고 순서를 재게 하려는 것이다.
    fn clear_notify_then_publish_port(
        notify_dir: Option<std::path::PathBuf>,
        port: u16,
        custom_port_file: Option<&std::path::Path>,
    ) -> Result<()> {
        Self::clear_notify_then_publish(notify_dir, || {
            port_file::write_port_file_to(port, custom_port_file)
        })
    }

    /// 부팅 청소가 지울 `notify/` — **포트 파일과 같은 뿌리일 때만** 데이터 루트의 것을 준다.
    ///
    /// 완료 로그의 writer 는 데이터 루트(`TASTY_PARENT_HOME` = 이 호스트의 `tasty_home()`)
    /// 밑에 쓴다. 기본 포트 파일도 그 루트 바로 밑(`<루트>/tasty.port`)이라, 기본 부팅에서는
    /// 두 뿌리가 같고 청소 대상은 예전 그대로 `<루트>/notify` 다.
    ///
    /// `--port-file` 이 포트 파일을 **다른 디렉토리로** 옮기면 이 호스트는 그 데이터 루트의
    /// 주인임을 알리지 않는 것이다 — 같은 루트에 기본 포트 파일로 뜬 호스트가 따로 있을 수
    /// 있고, 그 호스트의 **살아 있는** 완료 로그를 지우게 된다(ADR-0344 가 실측한 사고).
    /// 그래서 그때는 `None`(청소 안 함)을 준다. 뿌리를 포트 파일 쪽 `notify/` 로 옮기지
    /// 않는 이유는 writer 가 그곳에 안 쓰기 때문이다 — 아무도 안 쓰는 디렉토리를 지우는
    /// 것은 청소가 아니다. 근거·대안: `docs/adr/0416-the-boot-cleanup-follows-the-port-file-root.md`.
    ///
    /// 같은 디렉토리인지는 정규화한 경로로 가린다. 어느 한쪽이 아직 없으면(첫 부팅) 정규화가
    /// 실패하므로 적힌 그대로 견준다.
    fn notify_dir_to_clear(
        data_root: Option<std::path::PathBuf>,
        custom_port_file: Option<&std::path::Path>,
    ) -> Option<std::path::PathBuf> {
        let root = data_root?;
        if let Some(port_file) = custom_port_file {
            let port_dir = match port_file.parent() {
                Some(dir) if !dir.as_os_str().is_empty() => dir,
                _ => std::path::Path::new("."),
            };
            let same = match (
                std::fs::canonicalize(port_dir),
                std::fs::canonicalize(&root),
            ) {
                (Ok(a), Ok(b)) => a == b,
                _ => port_dir == root.as_path(),
            };
            if !same {
                tracing::info!(
                    "port file {} is outside the data root {} — leaving {}/notify alone, \
                     another host may own it",
                    port_file.display(),
                    root.display(),
                    root.display()
                );
                return None;
            }
        }
        Some(root.join("notify"))
    }

    /// 청소를 끝낸 **뒤** 발행 단계를 부른다.
    ///
    /// 발행을 클로저로 받는 이유는 순서가 이 함수의 계약 **전부**이기 때문이다.
    /// 반환 뒤의 상태만 보면 두 단계가 어느 순서로 일어났든 똑같다(포트 파일 있음 ·
    /// `notify/` 없음) — 그래서 순서는 **발행 시점에 서 있는 관측자만** 잴 수 있다.
    /// 시험은 그 자리에서 `notify/` 의 부재를 단언하므로, 두 줄을 뒤집는 변경이
    /// 여기서 죽는다.
    fn clear_notify_then_publish<F>(
        notify_dir: Option<std::path::PathBuf>,
        publish: F,
    ) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        if let Some(dir) = notify_dir {
            Self::clear_notify_dir(&dir);
        }
        publish()
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
        queue: CommandQueue,
        waker: Option<IpcWaker>,
        stream_ctx: StreamContext,
        _slot: ConnectionSlot,
        first_line_idle: Duration,
    ) {
        let Some((mut reader, mut writer, peer)) = Self::prepare_stream(stream) else {
            return;
        };

        // Read the first line manually so the BufReader retains any bytes
        // buffered after it. On a streaming-channel upgrade those buffered bytes
        // are the start of the binary frames following the handshake line.
        let mut line = String::new();
        match Self::read_first_line(&mut reader, &mut line, peer, first_line_idle) {
            // 첫 줄의 기한은 여기까지다 — 걷지 못하면 요청 사이의 쉼에 상한이 남으므로 닫는다.
            LineRead::Line if !first_line::clear_read_timeout(&reader) => return,
            LineRead::Line => {}
            // 줄을 끝내 안 보냈다 — 자리를 돌려주기 전에 사유를 알린다.
            LineRead::Idle => {
                Self::refuse_idle_first_line(&mut writer, peer, first_line_idle);
                return;
            }
            // 첫 줄이 상한을 넘었다 — 이 연결은 업그레이드 판별에 닿지 못한다.
            // 핸드셰이크 줄은 상한 근처에 갈 일이 없으므로 여기서 RPC 쪽 거절로
            // 답하는 것이 맞다.
            LineRead::TooLong => {
                Self::refuse_oversized_line(&mut writer, peer);
                return;
            }
            // 둘 다 이미 로그를 남겼다 — 쓸 상대가 없거나(EOF) 소켓이 깨졌다.
            LineRead::Eof | LineRead::Failed => return,
        }

        // Streaming upgrade: first line is `{"method":"stream.open",...}`. The
        // connection leaves the request-response model and becomes a framed
        // bidirectional pipe.
        if let Ok(req) = serde_json::from_str::<JsonRpcRequest>(line.trim())
            && req.method == stream::STREAM_OPEN_METHOD
        {
            Self::handle_stream_connection(reader, writer, req, stream_ctx, peer);
            return;
        }

        Self::arm_response_write_timeout(&writer);
        Self::run_request_response_loop(&mut reader, &mut writer, &mut line, &queue, &waker, peer);

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

    /// request-response 연결의 쓰기에 [`RESPONSE_WRITE_TIMEOUT`] 을 건다.
    ///
    /// **`configure_socket` 이 아니라 여기서 거는 이유**: 옵션은 소켓 단위라
    /// `try_clone` 한 reader/writer 가 함께 받는데, 같은 소켓이 스트림 업그레이드로
    /// 갈 수도 있다. 업그레이드된 연결의 쓰기는 프레임을 나르는 **전용 write 스레드**가
    /// 하고(`spawn_stream_write_thread`), 그 스레드는 쓰기 오류 하나에 루프를 끊는다 —
    /// 거기에 시간 상한을 얹으면 출력이 몰린 attach 가 타임아웃 한 번에 끊기고, 프레임이
    /// 반만 나간 뒤 끊기면 길이 접두사 기준이 어긋나 그 뒤가 전부 오정렬된다. 업그레이드
    /// 경로의 쓰기 상한은 별개 결정이므로 여기서 앞당기지 않는다.
    ///
    /// 거는 데 실패하면 **연결을 끊지 않는다** — 그 경우 쓰기가 상한 없이 도는 이전
    /// 동작으로 돌아갈 뿐이고, 그것 때문에 지금 되는 연결을 못 쓰게 만들 이유는 없다.
    fn arm_response_write_timeout(writer: &std::net::TcpStream) {
        if let Err(e) = writer.set_write_timeout(Some(RESPONSE_WRITE_TIMEOUT)) {
            tracing::warn!("Failed to set IPC response write timeout: {e}");
        }
    }

    /// `reader` 에서 한 줄을 [`MAX_REQUEST_LINE_BYTES`] 안에서 읽는다.
    ///
    /// 상한 초과는 [`LineRead::TooLong`] 으로 갈라 돌려줄 뿐 **답하지도 닫지도 않는다** —
    /// 둘 다 호출자의 몫이다([`Self::refuse_oversized_line`] 이
    /// `ERR_REQUEST_LINE_TOO_LONG` 한 줄로 답하고, 그 뒤 호출자가 연결을 끝낸다).
    ///
    /// **답한 뒤에도 닫는 이유**가 이 함수의 판정에 딸려 있다 — 초과한 줄의 **나머지가
    /// 소켓에 그대로 남아 있다.** 잘린 JSON 에 parse error 를 돌려주고 계속 읽는
    /// 갈래(`send_parse_error` 는 쓰기가 성공하면 연결을 유지한다)를 그대로 쓰면 한 줄이
    /// 여러 요청으로 쪼개져 들어가고, 상한은 다시 없는 것이 된다.
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
                    "IPC request line from {:?} exceeded {} bytes — refusing the line and closing the connection",
                    peer,
                    MAX_REQUEST_LINE_BYTES
                );
                LineRead::TooLong
            }
            Ok(_) => LineRead::Line,
            Err(e) if first_line::is_idle_expiry(&e) => LineRead::Idle,
            Err(e) => {
                tracing::warn!("IPC read error from {:?}: {}", peer, e);
                LineRead::Failed
            }
        }
    }

    /// 스트리밍 업그레이드 판별을 위해 첫 줄을 수동으로 읽는다 — `BufReader` 가
    /// 그 뒤에 이미 버퍼링된 바이트(업그레이드 시 핸드셰이크 뒤에 오는 바이너리
    /// 프레임의 시작)를 보존하게 하기 위함.
    ///
    /// 반환은 [`LineRead`] 네 갈래다. EOF 와 소켓 오류는 로그가 이미 나갔으므로 호출자는
    /// 그냥 끝내면 되지만, [`LineRead::TooLong`] 은 **끝내기 전에 답할 일**이 남아 있다
    /// ([`Self::refuse_oversized_line`]). 그 갈래를 `Failed` 와 한 덩어리로 다루면 첫 줄
    /// 초과가 다시 무응답 종료로 돌아간다.
    ///
    /// 첫 줄에는 `within` 의 기한이 줄 전체에 걸린다([`first_line`]). 만료는
    /// [`LineRead::Idle`] 로 돌아온다.
    fn read_first_line(
        reader: &mut BufReader<std::net::TcpStream>,
        line: &mut String,
        peer: Option<std::net::SocketAddr>,
        within: Duration,
    ) -> LineRead {
        let outcome = Self::read_line_capped(
            &mut first_line::DeadlineReader::new(reader, within),
            line,
            peer,
        );
        if matches!(outcome, LineRead::Eof) {
            tracing::debug!("IPC client disconnected (eof) {:?}", peer);
        }
        outcome
    }

    /// 일반 request-response 연결: 이미 읽은 첫 줄을 처리한 뒤, 연결이 닫히거나
    /// 처리가 `false` 를 반환할 때까지 계속 다음 줄을 읽어 처리한다.
    fn run_request_response_loop(
        reader: &mut BufReader<std::net::TcpStream>,
        writer: &mut std::net::TcpStream,
        line: &mut String,
        queue: &CommandQueue,
        waker: &Option<IpcWaker>,
        peer: Option<std::net::SocketAddr>,
    ) {
        if !Self::process_request_line(line, queue, waker, writer, peer) {
            return;
        }
        loop {
            line.clear();
            match Self::read_line_capped(reader, line, peer) {
                LineRead::Line => {}
                // 상한 초과는 **답하고** 끝낸다. 나머지 둘은 답할 상대가 없다.
                LineRead::TooLong => {
                    Self::refuse_oversized_line(writer, peer);
                    break;
                }
                // `Idle` 은 요청 사이에 기한이 없으므로 여기서는 안 난다 — 나면 소켓 오류로 본다.
                LineRead::Eof | LineRead::Failed | LineRead::Idle => break,
            }
            if !Self::process_request_line(line, queue, waker, writer, peer) {
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
        sink_rx: tasty_ipc::stream_hub::SinkReceiver,
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
        queue: &CommandQueue,
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
                return Self::send_parse_error(writer, e);
            }
        };

        // 무게는 받은 줄 그대로다(개행·앞뒤 공백 제외) — 정의는 `tasty_ipc::admission`.
        Self::dispatch_and_await(request, trimmed.len(), queue, waker, writer, peer)
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

    /// JSON 파싱 실패 시 JSON-RPC parse error(-32700) 응답을 회신한다. 반환값은
    /// **연결 유지 여부**다.
    ///
    /// 예전에는 이 응답의 쓰기가 실패해도 연결을 유지했다(`trace` 한 줄). 그 판단은
    /// 쓰기가 무한정 블록하던 시절에 맞았다 — 그때 실패는 소켓이 이미 깨졌다는 뜻이라
    /// 계속 읽어도 곧 EOF 였다. [`RESPONSE_WRITE_TIMEOUT`] 이 걸린 뒤로는 **살아 있는
    /// 소켓에 한 줄이 반만 나간 상태**가 같은 갈래로 떨어진다. 그 뒤로 계속 읽으면
    /// client 는 잘린 JSON 뒤에 다음 응답이 이어 붙은 것을 본다. 그래서 지금은 닫는다.
    ///
    /// 로그가 `trace` 가 아니라 `warn` 인 이유도 같다 — 타임아웃이 **발화했다**는 것을
    /// 말하는 자리가 여기뿐이고, 기본 필터는 `trace` 를 버린다.
    fn send_parse_error(writer: &mut std::net::TcpStream, e: serde_json::Error) -> bool {
        let err_resp = JsonRpcResponse::error(
            serde_json::Value::Null,
            -32700,
            format!("Parse error: {}", e),
        );
        let json = serde_json::to_string(&err_resp).unwrap();
        let (write_result, flush_result) = Self::write_json_line(writer, &json);
        if let Err(e) = write_result {
            tracing::warn!("IPC parse-error response write failed: {e}");
            return false;
        }
        if let Err(e) = flush_result {
            tracing::warn!("IPC parse-error response flush failed: {e}");
            return false;
        }
        true
    }

    /// 연결 상한 거절을 **응답으로** 알린다. 쓰고 나면 소켓이 닫힌다.
    ///
    /// ★ **이 쓰기는 막히면 안 된다.** 거절이 일어나는 자리가 accept 스레드라, 거기서
    /// 막히는 쓰기를 하면 **그 스레드가 멈추고 자리가 나도 아무도 못 붙는다** — 상한이
    /// 지키려던 것을 상한의 통지가 깨는 모양이다. 쓰기용 스레드를 띄우는 것도 답이 아니다:
    /// 그것이 바로 이 상한이 아끼려는 자원이고, 거절은 재시도 루프에서 몰려 온다.
    ///
    /// 그래서 **non-blocking 으로 한 번만 시도하고 결과와 무관하게 닫는다.** 거절 줄은
    /// 200 바이트 안쪽이고 갓 열린 소켓의 송신 버퍼는 그보다 훨씬 크므로 보통은 한 번에
    /// 나간다. 안 나가면 client 가 보는 것은 예전과 같은 EOF 다 — **더 나빠지지 않는다.**
    /// 그 최선 노력 성질은 `ERR_CONNECTION_LIMIT_REACHED` 의 doc 에도 적혀 있다.
    ///
    /// ★ **그 개선은 request-response 인구의 것이다.** 이 자리는 업그레이드 판별 전이라
    /// 스트림 client 도 같은 바이트를 받는데, 그쪽의 첫 읽기는 프레임이라 사유 대신
    /// `unknown stream tag` 를 본다. 즉 "못 나가면 EOF" 뿐 아니라 **나갔는데 못 읽는**
    /// 갈래가 있고, 거기서 이 줄은 EOF 를 대체하지 못한다. 그래도 여기서 줄을 안 읽는
    /// 것이 이 거절의 값이라 바꾸지 않는다 — 근거는 `ERR_CONNECTION_LIMIT_REACHED` 의
    /// doc 과 ADR-0327.
    ///
    /// 로그가 `debug` 인 이유: 거절은 상대가 재시도 루프를 돌면 몰려 오고, 포화로
    /// **들어가는 순간**의 `warn` 은 [`ConnectionSlot::try_acquire`] 이 이미 낸다.
    fn refuse_saturated_connection(stream: std::net::TcpStream) {
        if let Err(e) = stream.set_nonblocking(true) {
            tracing::debug!("IPC saturation refusal could not go non-blocking: {e}");
            return;
        }
        let line = Self::saturation_refusal_line();
        let mut w = stream;
        // 한 번만 쓴다. `write_all` 은 부분 전송에서 다시 시도하므로 여기서는 안 쓴다.
        Self::log_saturation_refusal_write(w.write(line.as_bytes()), line.len());
    }

    /// 포화 거절 한 줄을 짓는다. 짓는 일과 보내는 일을 갈라 둔다.
    fn saturation_refusal_line() -> String {
        let resp = JsonRpcResponse::error(
            serde_json::Value::Null,
            crate::ipc::protocol::ERR_CONNECTION_LIMIT_REACHED,
            format!(
                "the server already holds {MAX_CONCURRENT_CONNECTIONS} concurrent connections \
                 and did not read this one — nothing ran, so retry it as is"
            ),
        );
        format!("{}\n", serde_json::to_string(&resp).unwrap())
    }

    /// 한 번의 쓰기 결과를 남긴다. 이 연결은 어차피 끝나므로 되돌릴 일은 없다.
    fn log_saturation_refusal_write(result: std::io::Result<usize>, len: usize) {
        match result {
            Ok(n) if n == len => {}
            Ok(n) => tracing::debug!("IPC saturation refusal only partly sent ({n}/{len})"),
            Err(e) => tracing::debug!("IPC saturation refusal not sent: {e}"),
        }
    }

    /// 줄 상한 초과를 **응답으로** 알린다. 쓰고 나면 호출자가 연결을 끝낸다.
    ///
    /// 예전에는 이 자리가 무응답 종료였다(ADR-0304). client 는 닫힌 소켓만 보았고, 자기
    /// 줄이 길어서인지 네트워크가 끊겨서인지 고를 수 없었다 — 두 사건의 처방이 정반대다
    /// (앞은 요청을 줄이거나 나눠 보내고, 뒤는 그대로 다시 건다). 이제 코드로 답한다.
    ///
    /// **연결은 그대로 끝난다.** 넘긴 줄의 나머지가 소켓에 남아 있어서, 계속 읽으면 한
    /// 줄이 여러 요청으로 쪼개져 들어가고 상한이 다시 없는 것이 된다.
    ///
    /// `id` 가 `Null` 인 이유: 줄이 잘려 있어 요청의 `id` 를 신뢰할 수 없다. parse error
    /// 응답과 같은 처리다.
    ///
    /// 쓰기 상한을 여기서 다시 거는 이유: 이 자리는 첫 줄 경로에서도 불리는데 그때는
    /// 아직 스트림 업그레이드 판별 전이라 [`Self::arm_response_write_timeout`] 이 안
    /// 걸려 있다. 상한 없는 쓰기로 거절을 알리면 거절이 곧 새로운 무한 블록이 된다.
    fn refuse_oversized_line(writer: &mut std::net::TcpStream, peer: Option<std::net::SocketAddr>) {
        Self::arm_response_write_timeout(writer);
        let resp = JsonRpcResponse::error(
            serde_json::Value::Null,
            crate::ipc::protocol::ERR_REQUEST_LINE_TOO_LONG,
            format!(
                "request line exceeded {MAX_REQUEST_LINE_BYTES} bytes — the connection is closed"
            ),
        );
        let json = serde_json::to_string(&resp).unwrap();
        let (write_result, flush_result) = Self::write_json_line(writer, &json);
        // 이 연결은 어차피 끝난다. 쓰기가 실패했다는 사실만 남긴다 — 상한 초과 자체는
        // `read_line_capped` 이 이미 warn 으로 적었다.
        if let Err(e) = write_result {
            tracing::debug!("IPC oversize refusal write failed for {peer:?}: {e}");
        }
        if let Err(e) = flush_result {
            tracing::debug!("IPC oversize refusal flush failed for {peer:?}: {e}");
        }
    }

    /// 첫 줄 기한 만료를 **응답으로** 알린다. 쓰고 나면 호출자가 연결을 끝낸다.
    ///
    /// 연결을 닫는 이유가 "줄이 안 왔다" 라는 것을 client 가 EOF 와 구별하게 하려는 것이다 —
    /// 네트워크 단절과 처방이 다르다(앞은 연결 직후 바로 보내면 되고, 뒤는 다시 붙는다).
    /// 쓰기 상한을 여기서 거는 이유는 [`Self::refuse_oversized_line`] 과 같다 — 업그레이드
    /// 판별 전이라 아직 안 걸려 있고, 줄을 안 보내는 peer 는 응답도 안 읽을 수 있다.
    ///
    /// 받은 바이트가 있었어도(개행 없이 흘린 경우) `id` 는 `Null` 이다 — 끝나지 않은 줄이다.
    fn refuse_idle_first_line(
        writer: &mut std::net::TcpStream,
        peer: Option<std::net::SocketAddr>,
        within: Duration,
    ) {
        tracing::warn!(
            "IPC client {peer:?} sent no complete request line within {within:?} — closing the connection"
        );
        Self::arm_response_write_timeout(writer);
        let resp = JsonRpcResponse::error(
            serde_json::Value::Null,
            crate::ipc::protocol::ERR_FIRST_LINE_IDLE,
            format!(
                "no complete request line arrived within {} ms of connecting — nothing ran; \
                 the connection is closed, send the request right after connecting",
                within.as_millis()
            ),
        );
        Self::write_final_line(writer, &resp, peer);
    }

    /// 닫기 직전의 마지막 응답 한 줄을 쓴다. 이 연결은 어차피 끝나므로 쓰기 실패는 남기기만 한다.
    fn write_final_line(
        writer: &mut std::net::TcpStream,
        resp: &JsonRpcResponse,
        peer: Option<std::net::SocketAddr>,
    ) {
        let json = serde_json::to_string(resp).unwrap();
        let (write_result, flush_result) = Self::write_json_line(writer, &json);
        if let Err(e) = write_result {
            tracing::debug!("IPC closing refusal write failed for {peer:?}: {e}");
        }
        if let Err(e) = flush_result {
            tracing::debug!("IPC closing refusal flush failed for {peer:?}: {e}");
        }
    }

    /// 요청을 메인 스레드로 보내고 응답을 기다려 클라이언트로 회신한다. 반환값은
    /// 연결 유지 여부.
    ///
    /// **기다리는 주체는 이 연결 스레드다.** 굳은 핸들러가 GUI 를 멈추지는 않지만
    /// [`ConnectionSlot`] 하나를 그동안 쥔다. 응답 통로(`response_tx`)가 **버려지면**
    /// 기다림은 즉시 끝나므로(`Err(Disconnected)`) 영구히 남는 경우는 하나뿐이다 —
    /// 그 통로를 **든 채 끝나지 않는** 실행이다. 그것이 정당한 경우(`approval.await` 는
    /// 사람의 결재를 기다린다)와 굳은 경우가 여기서는 구분되지 않는다.
    ///
    /// 그래서 상한을 **요청이 싣고 온다**([`JsonRpcRequest::response_timeout_ms`]).
    /// 없거나 0 이면 상한이 없다 — 이 필드 이전의 동작 그대로다.
    ///
    /// 큐에 넣기 **전에** 입장 장부가 판정한다. 거절이면 명령은 큐에 안 들어가고
    /// `ERR_COMMAND_QUEUE_FULL` 로 답한 뒤 **연결을 유지한다** — 줄은 끝까지 읽혔으므로
    /// 다음 요청을 같은 연결로 받아도 된다.
    fn dispatch_and_await(
        request: JsonRpcRequest,
        wire_bytes: usize,
        queue: &CommandQueue,
        waker: &Option<IpcWaker>,
        writer: &mut std::net::TcpStream,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        let (resp_tx, resp_rx) = mpsc::sync_channel(1);

        // 만료 응답이 요청의 `id` 를 되돌려줘야 호출자가 어느 요청인지 안다. `request`
        // 는 곧 명령으로 옮겨지므로 여기서 복사해 둔다.
        let rpc_id = request.id.clone().unwrap_or(serde_json::Value::Null);
        let wait_bound = request
            .response_timeout_ms
            .filter(|ms| *ms > 0)
            .map(Duration::from_millis);

        let mut cmd = IpcCommand::with_wire_bytes(request, resp_tx, wire_bytes);
        if let Err(refusal) = cmd.admit(&queue.admission, Origin::Socket) {
            return Self::answer_queue_full(writer, rpc_id, &refusal.to_string(), peer);
        }
        // 상한에서 물러날 때 "아직 시작 전이었나" 를 물을 사본 — 보내면 명령은 옮겨진다.
        let lifecycle = cmd.lifecycle();

        // Send command to main thread
        if queue.tx.send(cmd).is_err() {
            tracing::warn!("IPC cmd_tx.send failed (main thread shut down?)");
            return false;
        }

        // Wake the event loop so it processes the command immediately
        if let Some(waker) = waker {
            waker();
        }

        Self::await_dispatch_response(&resp_rx, wait_bound, &lifecycle, rpc_id, writer, peer)
    }

    /// 메인 스레드의 응답을 기다려 소켓에 쓴다. 상한이 없으면 무기한 기다린다.
    ///
    /// 보내는 일과 기다리는 일을 한 함수에 두면 갈래가 겹쳐 읽기 어렵다 — 여기는
    /// **기다리는 쪽만** 본다. 상한 만료는 실패가 아니라 결과 불명이라 연결을 유지한다.
    fn await_dispatch_response(
        resp_rx: &mpsc::Receiver<JsonRpcResponse>,
        wait_bound: Option<Duration>,
        lifecycle: &tasty_ipc::server::LifecycleHandle,
        rpc_id: serde_json::Value,
        writer: &mut std::net::TcpStream,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        let Some(bound) = wait_bound else {
            return match resp_rx.recv() {
                Ok(response) => Self::write_dispatch_response(writer, &response, peer),
                Err(e) => {
                    tracing::warn!(
                        "IPC resp_rx.recv failed: {} (response_tx dropped without sending)",
                        e
                    );
                    false
                }
            };
        };
        match resp_rx.recv_timeout(bound) {
            Ok(response) => Self::write_dispatch_response(writer, &response, peer),
            // 만료 순간 명령이 아직 큐에 있었으면 실행되지 않게 막고 "실행 안 됨" 으로 답한다
            // (ADR-0411). 이미 시작됐으면 종전 그대로 결과 불명이다.
            Err(mpsc::RecvTimeoutError::Timeout) => match lifecycle.withdraw() {
                tasty_ipc::server::Withdraw::NotRun => Self::write_dispatch_response(
                    writer,
                    &tasty_ipc::server::expired_before_run_response(rpc_id, bound),
                    peer,
                ),
                tasty_ipc::server::Withdraw::Started => {
                    Self::answer_wait_expired(writer, rpc_id, bound, peer)
                }
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!("IPC resp_rx.recv failed (response_tx dropped without sending)");
                false
            }
        }
    }

    /// 호출자가 실은 대기 상한이 만료됐다. **연결은 유지한다.**
    ///
    /// 이 답이 "실패" 가 아니라 **결과 불명**인 이유: 호스트는 응답 통로를 놓았을 뿐이고
    /// 그 요청은 메인 스레드에서 계속 실행될 수 있다. 나중에 완료되면 `send_response` 가
    /// 수신자 없음으로 조용히 실패한다 — 그래서 이 소켓에 뒤늦은 응답이 끼어들 일은
    /// 없고, 연결을 닫을 이유도 없다.
    ///
    /// 호출자가 다음에 할 일이 그 구분에 달렸다. 부수효과가 남는 메서드를 그냥 재전송하면
    /// **두 번째 효과**가 남으므로, 재전송 전에 상태를 먼저 읽어야 한다.
    fn answer_wait_expired(
        writer: &mut std::net::TcpStream,
        rpc_id: serde_json::Value,
        bound: Duration,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        tracing::warn!(
            "IPC response wait of {:?} expired for {:?} — the request may still be running",
            bound,
            peer
        );
        let resp = JsonRpcResponse::error(
            rpc_id,
            crate::ipc::protocol::ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN,
            format!(
                "the caller's response timeout of {} ms expired before the host answered; \
                 whether the request ran is unknown — read the state before resending it",
                bound.as_millis()
            ),
        );
        Self::write_dispatch_response(writer, &resp, peer)
    }

    /// 명령 큐가 차서 요청을 넣지 않았다. **연결은 유지한다.** 아무것도 실행되지 않았으므로
    /// 호출자가 할 일은 잠시 뒤 그대로 다시 거는 것이다.
    ///
    /// 로그가 `debug` 인 이유: 거절은 밀린 순간 몰려 오고, 그 수는 장부의 누계
    /// (`CommandAdmission::snapshot`)가 센다. 거절의 사유는 요청자가 응답으로 받는다.
    fn answer_queue_full(
        writer: &mut std::net::TcpStream,
        rpc_id: serde_json::Value,
        reason: &str,
        peer: Option<std::net::SocketAddr>,
    ) -> bool {
        tracing::debug!("IPC request from {peer:?} refused at the command queue: {reason}");
        let resp = JsonRpcResponse::error(
            rpc_id,
            crate::ipc::protocol::ERR_COMMAND_QUEUE_FULL,
            format!("{reason} — nothing ran; retry it as is after a pause"),
        );
        Self::write_dispatch_response(writer, &resp, peer)
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
    /// 꺼낸 명령은 입장 장부에 몫을 돌려준다 — 장부가 재는 것은 대기열이지 처리 중인
    /// 일이 아니다.
    fn try_recv(&self) -> Result<IpcCommand, mpsc::TryRecvError> {
        let mut cmd = self.command_rx.try_recv()?;
        cmd.mark_dequeued();
        Ok(cmd)
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

    // 연결 상한 거절도 **EOF 가 아니라 JSON 한 줄**로 온다.
    //
    // 재는 자리가 accept 루프가 아니라 거절 함수인 것은 한계다 — 루프까지 재려면 실
    // 인스턴스에 상한+1 개를 붙여야 하고 그 시험은 스레드를 수백 개 띄운다. 여기서
    // 재는 것은 "거절이 어떤 바이트로 나가는가" 이고, 그 함수를 루프가 부르는지는
    // 소스로 확인한다.
    #[test]
    fn a_refused_connection_is_told_why_before_it_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = std::net::TcpStream::connect(addr).expect("connect");
        let (server_side, _) = listener.accept().expect("accept");

        TcpIpcServer::refuse_saturated_connection(server_side);

        let mut got = String::new();
        BufReader::new(client)
            .read_line(&mut got)
            .expect("거절 응답을 읽어야 한다");
        assert!(
            !got.is_empty(),
            "연결이 그냥 닫혔다 — 거절이 응답으로 안 왔다"
        );
        let resp: JsonRpcResponse =
            serde_json::from_str(got.trim()).expect("JSON 한 줄이어야 한다");
        let err = resp.error.expect("에러 응답이어야 한다");
        assert_eq!(
            err.code,
            crate::ipc::protocol::ERR_CONNECTION_LIMIT_REACHED,
            "연결 상한 거절이 전송 계층 코드로 안 왔다"
        );
        assert!(
            err.message
                .contains(&MAX_CONCURRENT_CONNECTIONS.to_string()),
            "거절 문구가 상한 값을 안 싣는다: {}",
            err.message
        );
    }

    // 응답이 안 오면 **호출자가 실은 상한**에서 끝나고, 그 답은 실패가 아니라
    // "결과 불명" 이다 — 단 요청이 **이미 시작된** 경우만이다.
    //
    // 명령을 꺼내 실행을 시작한 채(`claim`) 답하지 않고 쥐고 있는 것이 이 시험의 장치다 —
    // 그러면 `response_tx` 가 **살아 있어** 기다림이 `Disconnected` 로 일찍 끝나지 않는다.
    // 그 갈래가 바로 이 상한이 없으면 영원히 안 끝나는 자리다. 아무도 명령을 안 집으면
    // 만료 순간 요청은 시작 전이라 답이 "실행 안 됨" 으로 갈린다(ADR-0411).
    fn test_queue(tx: mpsc::Sender<IpcCommand>, limits: QueueLimits) -> CommandQueue {
        CommandQueue {
            tx,
            admission: CommandAdmission::new(limits),
        }
    }

    #[test]
    fn a_wait_past_the_callers_bound_answers_unknown_outcome() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = std::net::TcpStream::connect(addr).expect("connect");
        let (mut server_side, _) = listener.accept().expect("accept");

        let (cmd_tx, cmd_rx) = mpsc::channel::<IpcCommand>();
        let queue = test_queue(cmd_tx, QueueLimits::DEFAULT);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "workspace.list".to_string(),
            params: serde_json::Value::Null,
            id: Some(serde_json::json!(7)),
            session_token: None,
            response_timeout_ms: Some(50),
            idempotency_key: None,
        };
        // 메인 스레드 역할 — 명령을 꺼내 **실행을 시작하고** 답하지 않는다. 시작된 뒤의
        // 만료만 "결과 불명" 이다(큐에 남은 채 만료되면 "실행 안 됨" 이다 — 아래 짝 시험).
        // 답 통로는 끝까지 쥔다 — 버려지면 기다림이 만료가 아니라 끊김으로 끝난다.
        let (held_tx, held_rx) = mpsc::channel();
        let taker = std::thread::spawn(move || {
            let cmd = cmd_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("the request reaches the queue");
            let stats = Arc::new(tasty_ipc::dispatch::DispatchStats::default());
            assert_eq!(cmd.claim(&stats), tasty_ipc::server::Claim::Run);
            // 받는 쪽이 이미 실패해 사라졌을 수 있다 — 그때는 시험이 다른 실패문으로 끝난다.
            let _ = held_tx.send(cmd);
        });

        // 기다림을 별도 스레드에 두고 **완료 자체에 상한을 건다.** 상한이 안 걸리는
        // 회귀에서 이 시험이 멈춰 서면 안 된다 — 그때 CI 는 실패가 아니라 hang 을 본다.
        let (done_tx, done_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let kept =
                TcpIpcServer::dispatch_and_await(request, 0, &queue, &None, &mut server_side, None);
            // 받는 쪽이 이미 실패해 사라졌을 수 있다 — 그때는 시험이 다른 실패문으로
            // 끝나므로 이 전송의 실패에 대해 할 일이 없다.
            let _ = done_tx.send(kept);
            // 소켓은 여기서 닫힌다 — client 의 읽기가 EOF 로 끝나게 한다.
        });
        let kept = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("호출자가 실은 상한이 안 걸렸다 — 기다림이 안 끝났다");
        assert!(kept, "만료는 연결을 끊는 사건이 아니다");
        waiter.join().expect("waiter");
        taker.join().expect("taker");
        drop(held_rx);

        let mut got = String::new();
        BufReader::new(client)
            .read_line(&mut got)
            .expect("만료 응답을 읽어야 한다");
        let resp: JsonRpcResponse =
            serde_json::from_str(got.trim()).expect("JSON 한 줄이어야 한다");
        assert_eq!(resp.id, serde_json::json!(7), "요청의 id 를 안 돌려줬다");
        let err = resp.error.expect("에러 응답이어야 한다");
        assert_eq!(
            err.code,
            crate::ipc::protocol::ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN,
            "만료가 '결과 불명' 코드로 안 왔다"
        );
        assert!(
            err.message.contains("unknown"),
            "문구가 결과 불명을 말하지 않는다: {}",
            err.message
        );
    }

    // 같은 상한이 **요청이 큐에서 기다리는 동안** 지나면 답은 "실행 안 됨" 이고, 그 뒤에
    // 명령을 꺼내도 실행되지 않는다 — 기다리는 쪽이 물러나며 막았기 때문이다(ADR-0411).
    #[test]
    fn a_wait_that_ends_while_queued_answers_not_run_and_the_command_is_not_run_later() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = std::net::TcpStream::connect(addr).expect("connect");
        let (mut server_side, _) = listener.accept().expect("accept");

        let (cmd_tx, cmd_rx) = mpsc::channel::<IpcCommand>();
        let queue = test_queue(cmd_tx, QueueLimits::DEFAULT);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "workspace.create".to_string(),
            params: serde_json::Value::Null,
            id: Some(serde_json::json!(9)),
            session_token: None,
            response_timeout_ms: Some(30),
            idempotency_key: None,
        };
        let kept =
            TcpIpcServer::dispatch_and_await(request, 0, &queue, &None, &mut server_side, None);
        assert!(kept, "만료는 연결을 끊는 사건이 아니다");

        let queued = cmd_rx
            .try_recv()
            .expect("the request was queued and nobody took it");
        assert_eq!(
            queued.claim(&Arc::new(tasty_ipc::dispatch::DispatchStats::default())),
            tasty_ipc::server::Claim::Withdrawn,
            "the waiter withdrew it, so taking it now must not run it"
        );

        drop(server_side);
        let mut got = String::new();
        BufReader::new(client)
            .read_line(&mut got)
            .expect("the expiry answer");
        let resp: JsonRpcResponse = serde_json::from_str(got.trim()).expect("one JSON line");
        assert_eq!(resp.id, serde_json::json!(9));
        let err = resp.error.expect("an error response");
        assert_eq!(err.code, crate::ipc::protocol::ERR_EXPIRED_BEFORE_RUN);
        assert!(err.message.contains("did not run"), "{}", err.message);
    }

    // 장부의 몫은 **큐에서 꺼내는 순간** 돌아온다 — 꺼낸 명령을 아직 쥐고 있어도 그렇다.
    // 장부가 재는 것은 대기열이지 처리 중인 일이 아니다(ADR-0391). 지금은 소비자가 꺼낸
    // 명령을 한 회차 안에서 버리므로 표의 Drop 만으로도 곧 반납되지만, 명령을 회차 밖에
    // 보관하도록 바뀌면 그 차이가 조용한 과계수가 된다. 그래서 반납을 Drop 이 아니라
    // `try_recv` 에 묶은 것을 여기서 고정한다.
    #[test]
    fn a_dequeued_command_stops_counting_while_it_is_still_held() {
        let (tx, rx) = mpsc::channel();
        let admission = CommandAdmission::new(QueueLimits::DEFAULT);
        let server = TcpIpcServer {
            command_rx: rx,
            command_tx: tx.clone(),
            port: 0,
            shutdown: Arc::new(AtomicBool::new(false)),
            // 가짜 경로를 넣는 진짜 이유: `None` 이면 `effective_port_file_path` 가
            // `<TASTY_HOME>/tasty.port` 로 폴백하고, 이 서버의 Drop 이 **실행 중인 사용자 tasty 의
            // 포트 파일을 지운다.** `None` 으로 단순화하지 마라.
            // 이유: 프로세스당 하나여도 된다 — 이 경로는 한 번도 만들어지지 않고, Drop 이 지우려다
            // NotFound 로 끝나는 자리일 뿐이다. 재호출이 앞 호출의 파일을 지울 일이 없다.
            custom_port_file: Some(std::env::temp_dir().join(format!(
                "tasty-dequeue-release-test-{}.port",
                std::process::id()
            ))),
            admission: admission.clone(),
        };
        let (resp_tx, _resp_rx) = mpsc::sync_channel(1);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "workspace.list".to_string(),
            params: serde_json::Value::Null,
            id: Some(serde_json::json!(1)),
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let mut cmd = IpcCommand::with_wire_bytes(request, resp_tx, 100);
        cmd.admit(&admission, Origin::Socket)
            .expect("an empty queue takes one");
        tx.send(cmd).expect("queue");
        assert_eq!(admission.snapshot().queued_bytes, 100);

        let held = server.try_recv().expect("dequeue");
        let snap = admission.snapshot();
        assert_eq!(
            (snap.queued_bytes, snap.queued_commands),
            (0, 0),
            "a command taken off the queue still counts — the share is returned on drop, not on dequeue"
        );
        drop(held);
    }

    // 큐 바이트 상한을 넘기는 요청은 **큐에 안 들어가고** `ERR_COMMAND_QUEUE_FULL` 로 답을
    // 받으며 연결은 유지된다. 몫이 돌아오면 같은 요청이 다시 들어간다.
    //
    // 상한을 제품 값(수십 MiB)이 아니라 작은 값으로 세워, 이미 큐에 든 몫(`held`) 하나만으로
    // 넘치게 한다 — 다른 상한(줄 상한·연결 상한·주입 깊이)은 건드리지 않는다.
    #[test]
    fn a_request_past_the_queue_bytes_limit_is_refused_and_the_connection_kept() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = std::net::TcpStream::connect(addr).expect("connect");
        let (mut server_side, _) = listener.accept().expect("accept");
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let queue = test_queue(
            cmd_tx,
            QueueLimits {
                queued_bytes: 10,
                injected_depth: INJECTED_DEPTH_LIMIT,
            },
        );
        let request = |id: u64| JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "workspace.list".to_string(),
            params: serde_json::Value::Null,
            id: Some(serde_json::json!(id)),
            session_token: None,
            response_timeout_ms: Some(30),
            idempotency_key: None,
        };

        let held = queue
            .admission
            .admit(8, Origin::Socket)
            .expect("an empty queue takes one");
        let kept =
            TcpIpcServer::dispatch_and_await(request(3), 8, &queue, &None, &mut server_side, None);
        assert!(kept, "a queue refusal must not close the connection");
        assert!(
            cmd_rx.try_recv().is_err(),
            "the refused request reached the queue"
        );

        drop(held);
        let kept =
            TcpIpcServer::dispatch_and_await(request(4), 8, &queue, &None, &mut server_side, None);
        assert!(kept);
        assert!(
            cmd_rx.try_recv().is_ok(),
            "after the share came back the request must be queued"
        );

        drop(server_side);
        let mut lines = BufReader::new(client).lines();
        let first: JsonRpcResponse =
            serde_json::from_str(&lines.next().expect("line").expect("read")).expect("json");
        assert_eq!(first.id, serde_json::json!(3));
        let err = first.error.expect("refusal is an error response");
        assert_eq!(err.code, crate::ipc::protocol::ERR_COMMAND_QUEUE_FULL);
        assert!(err.message.contains("nothing ran"), "{}", err.message);
        let second: JsonRpcResponse =
            serde_json::from_str(&lines.next().expect("line").expect("read")).expect("json");
        assert_eq!(
            second.error.expect("nobody answers the queued one").code,
            crate::ipc::protocol::ERR_EXPIRED_BEFORE_RUN,
            "the second request was queued and never taken, so its wait expired before it ran"
        );
        let snap = queue.admission.snapshot();
        assert_eq!(snap.refused_bytes, 1);
    }

    // 상한을 안 실으면 **기다린다.** 그것이 이 필드 이전의 동작이고, 기본값이 상한이
    // 되면 사람의 결재를 기다리는 호출이 잘린다.
    //
    // "영원히" 는 시험으로 못 재므로 좌변을 좁힌다 — 짧은 시간 안에 **끝나지 않는 것**만
    // 본다. 위 시험이 같은 장치에서 50 ms 만에 끝나므로, 그 차이가 상한의 유무를 가른다.
    #[test]
    fn a_request_without_a_bound_is_still_waited_on() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let _client = std::net::TcpStream::connect(addr).expect("connect");
        let (mut server_side, _) = listener.accept().expect("accept");

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let queue = test_queue(cmd_tx, QueueLimits::DEFAULT);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "workspace.list".to_string(),
            params: serde_json::Value::Null,
            id: Some(serde_json::json!(1)),
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };

        let (done_tx, done_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let kept =
                TcpIpcServer::dispatch_and_await(request, 0, &queue, &None, &mut server_side, None);
            // 받는 쪽이 이미 실패해 사라졌을 수 있다 — 그때는 시험이 다른 실패문으로
            // 끝나므로 이 전송의 실패에 대해 할 일이 없다.
            let _ = done_tx.send(kept);
        });

        if let Ok(kept) = done_rx.recv_timeout(Duration::from_millis(300)) {
            panic!("상한을 안 실었는데 기다림이 끝났다 (반환값 {kept}) — 기본값이 상한이 됐다");
        }

        // 명령을 집어 응답 통로를 버리면 기다림이 풀린다. 스레드를 매달아 두지 않는다.
        drop(cmd_rx);
        waiter.join().expect("waiter");
    }

    // 상한을 넘긴 줄은 **EOF 가 아니라 JSON 한 줄**로 거절된다.
    //
    // 재는 자리가 헬퍼가 아니라 `run_request_response_loop` 인 것이 이 시험의 값이다 —
    // 그 루프가 예전에 `break` 로 끝나던 갈래가 바로 여기다. 소켓을 실제로 쓰므로
    // "client 가 무엇을 받는가" 가 좌변이 된다.
    //
    // 첫 줄로 빈 줄을 넘기는 이유: `process_request_line` 은 빈 줄에서 dispatch 없이
    // `true` 를 돌려주므로, 메인 루프 없이도 루프가 다음 줄을 읽는 자리까지 간다.
    #[test]
    fn an_oversized_line_is_refused_with_a_code_not_a_bare_close() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = std::net::TcpStream::connect(addr).expect("connect");
        let (server_side, _) = listener.accept().expect("accept");

        // 개행 없이 상한을 넘겨 보낸다. 8 MiB 는 소켓 버퍼에 안 들어가므로 별도
        // 스레드가 쓰고, 서버가 상한에서 읽기를 멈추면 그 쓰기는 미완으로 남는다 —
        // join 하지 않고 두며 시험 끝에 소켓이 닫히면 풀린다.
        let mut client_w = client.try_clone().expect("clone");
        std::thread::spawn(move || {
            let blob = vec![b'x'; MAX_REQUEST_LINE_BYTES + 16];
            // 실패가 정상 종료다 — 서버가 상한에서 읽기를 멈추면 남은 바이트는 갈 곳이
            // 없고, 시험이 끝나며 소켓이 닫히면 이 쓰기가 오류로 풀린다. 그 오류에
            // 대해 할 일이 없다.
            let _ = client_w.write_all(&blob);
        });

        let mut reader = BufReader::new(server_side.try_clone().expect("clone"));
        let mut writer = server_side;
        let (cmd_tx, _cmd_rx) = mpsc::channel();
        let queue = test_queue(cmd_tx, QueueLimits::DEFAULT);
        let mut first = String::new();
        TcpIpcServer::run_request_response_loop(
            &mut reader,
            &mut writer,
            &mut first,
            &queue,
            &None,
            None,
        );

        // 서버 쪽을 먼저 닫는다. 거절이 안 쓰였다면 client 의 읽기가 **막히지 않고**
        // EOF(0 바이트)로 끝나야 한다 — 그래야 회귀가 hang 이 아니라 실패로 드러난다.
        drop(reader);
        drop(writer);

        let mut got = String::new();
        BufReader::new(client)
            .read_line(&mut got)
            .expect("거절 응답을 읽어야 한다");
        assert!(
            !got.is_empty(),
            "연결이 그냥 닫혔다 — 거절이 응답으로 안 왔다"
        );
        let resp: JsonRpcResponse =
            serde_json::from_str(got.trim()).expect("JSON 한 줄이어야 한다");
        let err = resp.error.expect("에러 응답이어야 한다");
        assert_eq!(
            err.code,
            crate::ipc::protocol::ERR_REQUEST_LINE_TOO_LONG,
            "줄 상한 거절이 전송 계층 코드로 안 왔다"
        );
        assert!(
            err.message.contains(&MAX_REQUEST_LINE_BYTES.to_string()),
            "거절 문구가 상한 값을 안 싣는다: {}",
            err.message
        );
    }

    // 쓰기 상한이 **소켓에 실제로 걸린다.** 이 시험이 재는 것은 거기까지다 —
    // 타임아웃이 발화하는 것은 상대가 안 읽어 송신 버퍼가 차야 하는데, 그 버퍼 크기는
    // 커널이 자동 조정하므로 재현이 느리고 불안정하다. 그래서 "값이 걸렸다" 는 여기서
    // 재고 "발화한다" 는 안 잰다.
    //
    // 좌변을 상수와 비교하지 않고 **소켓에게 되물어** 확인한다. 값이 안 걸리는 실패와
    // 플랫폼이 값을 반올림하는 실패가 둘 다 여기서 드러난다.
    #[test]
    fn the_response_write_timeout_is_actually_on_the_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let _client = std::net::TcpStream::connect(addr).expect("connect");
        let (server_side, _) = listener.accept().expect("accept");

        assert_eq!(
            server_side.write_timeout().expect("read back"),
            None,
            "새 소켓에는 상한이 없어야 한다 — 이 시험의 전제"
        );

        TcpIpcServer::arm_response_write_timeout(&server_side);

        assert_eq!(
            server_side.write_timeout().expect("read back"),
            Some(RESPONSE_WRITE_TIMEOUT),
            "쓰기 상한이 소켓에 안 걸렸다"
        );
    }

    fn slots() -> (Arc<ConnectionStats>, AtomicBool) {
        (Arc::new(ConnectionStats::default()), AtomicBool::new(false))
    }

    /// `handle_connection` 을 실제 소켓 쌍 위에서 돌린다. 끝나면 `done` 에 신호가 온다.
    fn serve_one(
        first_line_idle: Duration,
    ) -> (
        std::net::TcpStream,
        Arc<ConnectionStats>,
        mpsc::Receiver<()>,
        mpsc::Receiver<IpcCommand>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let client =
            std::net::TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (server_side, _) = listener.accept().expect("accept");
        let (stats, sat) = slots();
        let slot = ConnectionSlot::try_acquire(&stats, &sat).expect("seat");
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let queue = test_queue(cmd_tx, QueueLimits::DEFAULT);
        let (inbound_tx, _inbound_rx) = mpsc::channel();
        let ctx = StreamContext {
            hub: tasty_ipc::stream_hub::StreamHub::new(),
            inbound_tx,
            waker: Arc::new(|| {}),
        };
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            TcpIpcServer::handle_connection(server_side, queue, None, ctx, slot, first_line_idle);
            // 받는 쪽이 이미 실패해 사라졌을 수 있다 — 그때 시험은 다른 실패문으로 끝난다.
            let _ = done_tx.send(());
        });
        (client, stats, done_rx, cmd_rx)
    }

    // 줄을 한 번도 안 보내는 연결은 기한에서 **사유를 받고** 닫히며, 자리가 돌아온다.
    // 이 상한이 없으면 연결 스레드가 첫 읽기에서 영원히 멈추고 자리를 쥔다.
    #[test]
    fn a_connection_that_never_sends_a_line_is_told_why_and_its_seat_returned() {
        let (client, stats, done, _cmd_rx) = serve_one(Duration::from_millis(100));
        assert_eq!(stats.snapshot().live, 1);
        done.recv_timeout(Duration::from_secs(5))
            .expect("the connection thread never ended — the first line has no deadline");
        assert_eq!(stats.snapshot().live, 0, "the seat was not returned");

        let mut got = String::new();
        BufReader::new(client).read_line(&mut got).expect("read");
        let resp: JsonRpcResponse =
            serde_json::from_str(got.trim()).expect("one JSON line, not a bare close");
        let err = resp.error.expect("error response");
        assert_eq!(err.code, crate::ipc::protocol::ERR_FIRST_LINE_IDLE);
        assert!(err.message.contains("nothing ran"), "{}", err.message);
    }

    // 기한은 **첫 줄에만** 걸린다. 첫 줄 뒤에는 기한보다 오래 쉬어도 연결이 살아 있어야
    // 한다 — 요청 사이에 쉬는 client 의 호환이 이 결정의 조건이다.
    #[test]
    fn a_pause_after_the_first_line_does_not_close_the_connection() {
        let (mut client, stats, done, _cmd_rx) = serve_one(Duration::from_millis(100));
        client.write_all(b"\n").expect("first line");
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            done.try_recv().is_err(),
            "the connection closed during a pause after its first line"
        );
        assert_eq!(stats.snapshot().live, 1);

        // 연결이 살아 있다는 것을 응답으로 확인한다 — 잘못된 JSON 은 parse error 로 답한다.
        client.write_all(b"{not json\n").expect("second line");
        let mut got = String::new();
        BufReader::new(client.try_clone().expect("clone"))
            .read_line(&mut got)
            .expect("read");
        let resp: JsonRpcResponse = serde_json::from_str(got.trim()).expect("json");
        assert_eq!(resp.error.expect("error").code, -32700);
        drop(client);
        done.recv_timeout(Duration::from_secs(5))
            .expect("ends on EOF");
    }

    // 자리는 잡히고, 스레드가 어떻게 끝나든 Drop 이 돌려준다.
    #[test]
    fn a_slot_is_returned_when_it_drops() {
        let (live, sat) = slots();
        {
            let _held = ConnectionSlot::try_acquire(&live, &sat).expect("첫 자리는 잡힌다");
            assert_eq!(live.snapshot().live, 1);
        }
        assert_eq!(live.snapshot().live, 0, "Drop 이 자리를 반납해야 한다");
    }

    // 상한을 넘는 요청은 거절되고, **거절이 계수를 밀어 올리지 않는다.**
    // try_open 은 먼저 fetch_add 하고 초과면 되돌리는데, 그 되돌림이 빠지면
    // 계수가 영구히 상한 위로 떠서 자리가 다시는 안 열린다.
    #[test]
    fn a_refusal_does_not_leak_the_count() {
        let (live, sat) = slots();
        let held: Vec<_> = (0..MAX_CONCURRENT_CONNECTIONS)
            .map(|_| ConnectionSlot::try_acquire(&live, &sat).expect("상한까지는 잡힌다"))
            .collect();
        assert_eq!(live.snapshot().live, MAX_CONCURRENT_CONNECTIONS as u64);

        assert!(
            ConnectionSlot::try_acquire(&live, &sat).is_none(),
            "상한을 넘는 연결은 거절된다"
        );
        assert_eq!(
            live.snapshot().live,
            MAX_CONCURRENT_CONNECTIONS as u64,
            "거절은 계수를 그대로 두어야 한다"
        );
        drop(held);
        assert_eq!(live.snapshot().live, 0);
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

    // ★ 자리 계수가 **밖에서 읽는 게이지**에 쌓인다. 이 파일의 자리 관리와
    // `system.pressure` 의 `connections` 덩어리를 잇는 것은 이 한 줄뿐이고, 이
    // 시험이 그 줄을 잰다 — accept 루프가 자기 지역 원자값으로 되돌아가면 여기서
    // 죽는다(게이지가 안 움직인다).
    #[test]
    fn the_seat_count_lands_in_the_gauge_the_diagnostic_reads() {
        let (gauge, sat) = slots();
        let held = ConnectionSlot::try_acquire(&gauge, &sat).expect("첫 자리는 잡힌다");
        let during = gauge.snapshot();
        drop(held);
        let after = gauge.snapshot();

        assert_eq!(during.live, 1, "붙어 있는 동안은 게이지가 1 이어야 한다");
        assert_eq!(after.live, 0, "반납되면 0 으로 돌아온다");
        assert_eq!(after.accepted, 1, "누계는 반납해도 안 내려간다");
        assert_eq!(after.live_max, 1, "최댓값이 그 순간을 남긴다");
        assert_eq!(after.refused_saturated, 0);
    }

    // 상한 거절이 **로그 게이트와 무관하게** 세진다. `saturated` 가 두 번째
    // 거절부터 로그를 debug 로 접는데, 그 접힘이 계수까지 접으면 운영자가 보는
    // 수가 실제 거절 수보다 작아진다.
    #[test]
    fn a_folded_log_line_does_not_fold_the_refusal_count() {
        let (gauge, sat) = slots();
        let _held: Vec<_> = (0..MAX_CONCURRENT_CONNECTIONS)
            .map(|_| ConnectionSlot::try_acquire(&gauge, &sat).expect("상한까지는 잡힌다"))
            .collect();
        for _ in 0..3 {
            assert!(ConnectionSlot::try_acquire(&gauge, &sat).is_none());
        }
        assert_eq!(
            gauge.snapshot().refused_saturated,
            3,
            "세 번 거절했으면 세 번 세야 한다 — 로그가 한 줄이어도"
        );
    }

    // 상한이 어디서 나왔는지를 값으로 고정한다. 호스트가 받아들이는 가장 큰 요청
    // payload 는 저장소 항목 하나이고, 그것이 한 줄에 실릴 때의 최악 팽창은 JSON
    // escape 의 6 배다. 상한을 그 아래로 내리면 **지금 통과하는 memory.set 이 거절된다.**
    //
    // 좌변이 `MemorySettings::default().entry_max_mb` 인 것이 이 시험의 요점이다.
    // 집행되는 cap 은 부팅이 그 설정값을 환산해 `MemoryConfig::entry_max_bytes` 에
    // 넣은 값이고(`src/boot.rs`), `tasty_memory::MAX_VALUE_BYTES` 는 그 경로에 **없다** —
    // 아무도 fallback 으로 안 읽는 상수라 그것을 재면 저장소 cap 을 64 배 올려도 이
    // 시험이 초록으로 남는다. 즉 재는 대상이 달랐다.
    #[test]
    fn the_cap_clears_the_largest_payload_the_store_accepts() {
        let entry_max_bytes = tasty_settings::MemorySettings::default()
            .entry_max_mb
            .saturating_mul(1024 * 1024) as usize;
        let worst_case_on_the_wire = entry_max_bytes * 6;
        assert!(
            MAX_REQUEST_LINE_BYTES >= worst_case_on_the_wire,
            "상한 {MAX_REQUEST_LINE_BYTES} 가 저장소 기본 cap {entry_max_bytes} 의 escape \
             최악치 {worst_case_on_the_wire} 보다 작다 — 정상 요청을 거절하게 된다"
        );
    }
}

#[cfg(test)]
mod notify_cleanup_tests {
    use super::*;

    // 기본 부팅(`--port-file` 없음)은 예전 그대로 데이터 루트의 `notify/` 를 치운다.
    #[test]
    fn without_a_port_file_override_the_data_root_notify_dir_is_cleared() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            TcpIpcServer::notify_dir_to_clear(Some(tmp.path().to_path_buf()), None),
            Some(tmp.path().join("notify"))
        );
    }

    // `--port-file` 이 데이터 루트 안(기본 자리와 같은 디렉토리)이면 그 호스트가 루트의
    // 주인이다 — 청소 대상은 그대로다.
    #[test]
    fn a_port_file_inside_the_data_root_keeps_the_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let port_file = tmp.path().join("other-name.port");
        assert_eq!(
            TcpIpcServer::notify_dir_to_clear(Some(tmp.path().to_path_buf()), Some(&port_file)),
            Some(tmp.path().join("notify"))
        );
    }

    // ADR-0344 가 실측한 사고의 회귀 시험. 같은 데이터 루트에 기본 포트 파일로 뜬 호스트 A
    // 가 있고, 호스트 B 가 `--port-file` 만 다른 디렉토리로 주고 뜬다. B 는 A 의 살아 있는
    // `notify/` 를 지우면 안 된다 — 청소 대상 자체가 없어야 한다.
    #[test]
    fn a_port_file_outside_the_data_root_leaves_the_notify_dir_alone() {
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let live = home.path().join("notify").join("9.log");
        std::fs::create_dir_all(live.parent().unwrap()).unwrap();
        std::fs::write(&live, b"surface 9 task complete (via spawn)\n").unwrap();
        let port_file = elsewhere.path().join("tasty.port");

        let target =
            TcpIpcServer::notify_dir_to_clear(Some(home.path().to_path_buf()), Some(&port_file));
        assert_eq!(
            target, None,
            "데이터 루트 밖의 포트 파일인데 청소 대상이 나왔다"
        );

        TcpIpcServer::clear_notify_then_publish_port(target, 4244, Some(&port_file))
            .expect("port file write");
        assert!(port_file.exists(), "포트 파일은 그래도 써야 한다");
        assert!(
            live.exists(),
            "다른 호스트의 살아 있는 완료 로그가 지워졌다"
        );
    }

    // 홈을 못 찾으면 포트 파일이 어디 있든 청소 대상이 없다.
    #[test]
    fn an_unresolved_data_root_has_nothing_to_clear() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            TcpIpcServer::notify_dir_to_clear(None, Some(&tmp.path().join("tasty.port"))),
            None
        );
    }

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

    // 청소가 **포트 파일보다 먼저** 끝난다. 포트 파일이 이 인스턴스의 존재를 알리는
    // 유일한 통로이므로, 그것이 나타난 시점에 notify/ 가 이미 치워져 있으면 어떤
    // writer 도 청소와 겹칠 수 없다.
    //
    // 파일을 여럿 심는 이유: 청소를 다시 join 하지 않는 스레드로 던지면 지울 것이
    // 많을수록 이 단언이 확실히 깨진다. 한 개만 심으면 분리 스레드가 이겨서 통과할
    // 수 있고, 그러면 이 시험이 순서를 재지 않는다.
    #[test]
    fn the_notify_dir_is_cleared_before_the_port_file_is_published() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        std::fs::create_dir_all(&notify).unwrap();
        for i in 0..512 {
            std::fs::write(notify.join(format!("{i}.log")), b"surface done\n").unwrap();
        }
        let port_file = tmp.path().join("tasty.port");

        TcpIpcServer::clear_notify_then_publish_port(Some(notify.clone()), 4242, Some(&port_file))
            .expect("port file write");

        assert!(port_file.exists(), "포트 파일이 쓰여야 한다");
        assert!(
            !notify.exists(),
            "포트 파일이 보이는 시점에 notify/ 는 이미 치워져 있어야 한다"
        );
    }

    // 위 시험은 **반환 뒤**의 두 사실만 본다 — 그것만으로는 순서가 안 재진다(두 단계를
    // 뒤집어도 반환 시점에는 똑같이 참이다). 순서는 발행 **시점**에 서서만 잴 수 있으므로,
    // 발행 클로저 안에서 `notify/` 의 부재를 관측해 값으로 남긴다.
    #[test]
    fn the_cleanup_has_already_finished_when_the_publish_step_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let notify = tmp.path().join("notify");
        std::fs::create_dir_all(&notify).unwrap();
        for i in 0..512 {
            std::fs::write(notify.join(format!("{i}.log")), b"surface done\n").unwrap();
        }

        let mut notify_seen_at_publish = None;
        TcpIpcServer::clear_notify_then_publish(Some(notify.clone()), || {
            notify_seen_at_publish = Some(notify.exists());
            Ok(())
        })
        .expect("publish");

        assert_eq!(
            notify_seen_at_publish,
            Some(false),
            "발행 단계가 도는 시점에 notify/ 가 아직 남아 있었다 — 청소가 뒤로 밀렸다"
        );
    }

    // 홈을 못 찾으면 청소할 대상이 없다 — 그래도 포트 파일은 써야 한다. 청소 실패가
    // 서버 기동을 막으면 안 된다.
    #[test]
    fn an_unresolved_home_skips_the_cleanup_and_still_publishes_the_port() {
        let tmp = tempfile::tempdir().unwrap();
        let port_file = tmp.path().join("tasty.port");

        TcpIpcServer::clear_notify_then_publish_port(None, 4243, Some(&port_file))
            .expect("port file write");

        assert!(port_file.exists());
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
    use tasty_ipc::stream_hub::StreamHub;

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
