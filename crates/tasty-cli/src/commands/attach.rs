//! `tasty attach <surface>` — surface 단위 attach client (attach/detach 단계 4).
//!
//! 두 모드:
//! - **mirror-dump**(기본, `--dump-after`): attach 후 일정 시간 출력을 수집해 mirror
//!   Terminal 로 grid 를 재구성하고 `screen_text` 를 stdout 으로 출력한다. GUI 없이
//!   로컬 loopback e2e 를 자동 검증하는 핵심 경로(초기 스냅샷 + 출력 delta 확인).
//! - **raw 브리지**(`--raw`): stdin↔stdout passthrough. detach 전용 키 `Ctrl+\`
//!   (decisions #8). 완전한 raw TTY 모드는 단계 4 옵션(여기선 기본 passthrough).
//!
//! `--send` 로 attach 직후 1 회 비대화형 입력을 보낼 수 있다(escape 디코딩) —
//! raw TTY 없이 입력 라우팅을 검증하기 위함.
//!
//! 핸드셰이크(`stream.open{target}`) 직후 서버는 attach 결과를 Control 프레임으로
//! 통지한다(`attached{cols,rows}` 또는 `attach_error{reason}`). 그 다음 Data 프레임이
//! 초기 스냅샷 + 이후 출력 delta. force-detach 는 `Control{force_detached}`+`Detach`.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tasty_ipc::stream::{self, STREAM_PROTO, StreamFrame, StreamTag};
use tasty_terminal::Terminal;

use crate::ssh::{self, Backoff, PortMode, SshTarget, SshTunnel};
use crate::stream::StreamConnection;

/// 한 attach 세션이 끝난 사유(백오프 재연결 판단용 — 단계 5).
pub(crate) enum AttachExit {
    /// 정상 종료(mirror-dump 1회성 완료, raw 의 사용자 detach/EOF, force-detach).
    Completed,
    /// 연결이 예기치 않게 끊김(터널/서버 단절) — 재연결 대상.
    Disconnected,
}

/// 단일 attach 세션 1 회: `127.0.0.1:port` 접속 → 핸드셰이크 → mirror/raw.
/// 로컬(loopback)과 SSH(터널 localport) 양쪽이 공유한다 — SSH 경로는 이 함수에
/// **터널의 localport** 를 넘기기만 한다(O7: `--port` 공개 플래그 불필요).
pub(crate) fn run_attach_on_port(
    port: u16,
    surface: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    raw: bool,
) -> Result<AttachExit> {
    let sock = TcpStream::connect(("127.0.0.1", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {port}: {e}. Is tasty running?"
        )
    })?;
    let (mut conn, client_id) = StreamConnection::open_attach(sock, STREAM_PROTO, surface)?;

    // 핸드셰이크 ack 다음의 attach 결과 Control 프레임.
    let first = conn.recv()?;
    if first.tag != StreamTag::Control {
        bail!("expected attach Control frame, got {:?}", first.tag);
    }
    let ctrl: serde_json::Value = serde_json::from_slice(&first.payload)?;
    match ctrl.get("event").and_then(|v| v.as_str()) {
        Some("attached") => {}
        Some("attach_error") => {
            let reason = ctrl
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            bail!("attach rejected: {reason}");
        }
        other => bail!("unexpected attach control event: {other:?}"),
    }
    let cols = ctrl.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
    let rows = ctrl.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
    eprintln!("attached surface {surface} (client_id={client_id}, {cols}x{rows})");

    if raw {
        run_raw_bridge(conn, send)
    } else {
        run_mirror_dump(conn, cols, rows, dump_after, send)
    }
}

/// `tasty attach --ssh user@host <surface>` (1회성 SSH 터널 attach — 단계 5).
///
/// ① 원격 포트 발견(auto fallback 체인) → ② `ssh -L` 터널 수립 → ③ 터널 localport 로
/// 단계 4 attach. SSH 끊김 시 백오프 재연결(decisions 7, `--no-reconnect` 로 off).
/// 세션은 서버에 상주하므로 재연결은 터널 재수립 + 재attach 만 하면 복구된다.
#[allow(clippy::too_many_arguments)]
pub fn run_attach_ssh(
    target: SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
    surface: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    raw: bool,
    reconnect: bool,
) -> Result<()> {
    let ssh = ssh::resolve_ssh_path();
    let dest = target.destination.clone();
    let mode = PortMode::parse(port_mode)?;
    // 자동 검증(Claude Bash) 한정 host key accept-new. 평상시는 기본 strict 유지(보안).
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);

    let mut backoff = Backoff::new();
    loop {
        // ① 원격 포트 발견.
        let remote_port = match ssh::discover_remote_port(
            &ssh,
            &target,
            remote_tasty,
            mode,
            verify,
            debug,
            port_file,
        ) {
            Ok(p) => p,
            Err(e) if reconnect => {
                eprintln!("원격 포트 발견 실패: {e} — 백오프 재시도");
                backoff.sleep();
                continue;
            }
            Err(e) => return Err(e),
        };

        // ② ssh -L 터널 (Drop 시 자식 ssh 자동 kill — 원격 데몬은 생존).
        let tunnel = match SshTunnel::establish(&ssh, &target, remote_port, verify) {
            Ok(t) => t,
            Err(e) if reconnect => {
                eprintln!("ssh 터널 수립 실패: {e} — 백오프 재시도");
                backoff.sleep();
                continue;
            }
            Err(e) => return Err(e),
        };
        eprintln!(
            "ssh 터널 수립: 127.0.0.1:{} → {dest}:{remote_port}",
            tunnel.local_port
        );
        backoff.reset();

        // ③ 단계 4 attach (터널 localport 로).
        match run_attach_on_port(tunnel.local_port, surface, dump_after, send, raw)? {
            AttachExit::Completed => return Ok(()),
            AttachExit::Disconnected if reconnect => {
                eprintln!("연결 끊김 — 백오프 재연결(세션은 서버 상주)");
                drop(tunnel); // 자식 ssh kill 후 재수립.
                backoff.sleep();
                continue;
            }
            AttachExit::Disconnected => return Ok(()),
        }
    }
}

/// workspace attach 1 회(로컬/SSH 공용). 핸드셰이크 → `attached_workspace` 디스크립터
/// 파싱 → 터미널마다 mirror 생성 + 비-터미널 placeholder 기록 → demux-dump.
pub(crate) fn run_attach_workspace_on_port(
    port: u16,
    workspace: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    send_to: Option<u32>,
) -> Result<AttachExit> {
    let sock = TcpStream::connect(("127.0.0.1", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {port}: {e}. Is tasty running?"
        )
    })?;
    let (mut conn, client_id) =
        StreamConnection::open_attach_workspace(sock, STREAM_PROTO, workspace)?;

    let first = conn.recv()?;
    if first.tag != StreamTag::Control {
        bail!("expected attach Control frame, got {:?}", first.tag);
    }
    let ctrl: serde_json::Value = serde_json::from_slice(&first.payload)?;
    match ctrl.get("event").and_then(|v| v.as_str()) {
        Some("attached_workspace") => {}
        Some("attach_error") => {
            let reason = ctrl
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            bail!("workspace attach rejected: {reason}");
        }
        other => bail!("unexpected attach control event: {other:?}"),
    }

    // surfaces 디스크립터 → 터미널 mirror + 비-터미널 placeholder. 트리 순서 보존.
    let surfaces = ctrl
        .get("surfaces")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut mirrors: Vec<(u32, Terminal)> = Vec::new();
    let mut placeholders: Vec<(u32, String)> = Vec::new();
    for s in &surfaces {
        let remote_id = s.get("remote_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        match s.get("role").and_then(|v| v.as_str()) {
            Some("terminal") => {
                let cols = s.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
                let rows = s.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
                mirrors.push((remote_id, Terminal::new_detached(cols, rows)));
            }
            _ => {
                let kind = s
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                placeholders.push((remote_id, kind));
            }
        }
    }
    eprintln!(
        "attached workspace {workspace} (client_id={client_id}, {} terminals, {} placeholders)",
        mirrors.len(),
        placeholders.len()
    );

    run_workspace_mirror_dump(conn, mirrors, placeholders, dump_after, send, send_to)
}

/// `tasty attach --ssh user@host --workspace <id>` (1회성 SSH 터널 workspace attach).
/// 단계 5 의 터널/포트발견/백오프를 그대로 재사용 — surface 단위와 동일한 SSH 경로.
#[allow(clippy::too_many_arguments)]
pub fn run_attach_workspace_ssh(
    target: SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
    workspace: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    send_to: Option<u32>,
    reconnect: bool,
) -> Result<()> {
    let ssh = ssh::resolve_ssh_path();
    let dest = target.destination.clone();
    let mode = PortMode::parse(port_mode)?;
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);

    let mut backoff = Backoff::new();
    loop {
        let remote_port = match ssh::discover_remote_port(
            &ssh,
            &target,
            remote_tasty,
            mode,
            verify,
            debug,
            port_file,
        ) {
            Ok(p) => p,
            Err(e) if reconnect => {
                eprintln!("원격 포트 발견 실패: {e} — 백오프 재시도");
                backoff.sleep();
                continue;
            }
            Err(e) => return Err(e),
        };

        let tunnel = match SshTunnel::establish(&ssh, &target, remote_port, verify) {
            Ok(t) => t,
            Err(e) if reconnect => {
                eprintln!("ssh 터널 수립 실패: {e} — 백오프 재시도");
                backoff.sleep();
                continue;
            }
            Err(e) => return Err(e),
        };
        eprintln!(
            "ssh 터널 수립: 127.0.0.1:{} → {dest}:{remote_port}",
            tunnel.local_port
        );
        backoff.reset();

        match run_attach_workspace_on_port(tunnel.local_port, workspace, dump_after, send, send_to)?
        {
            AttachExit::Completed => return Ok(()),
            AttachExit::Disconnected if reconnect => {
                eprintln!("연결 끊김 — 백오프 재연결(세션은 서버 상주)");
                drop(tunnel);
                backoff.sleep();
                continue;
            }
            AttachExit::Disconnected => return Ok(()),
        }
    }
}

/// workspace demux-dump: surface-prefixed Data 를 demux 해 각 mirror 에 feed,
/// deadline 후 surface 별 화면을 섹션으로 stdout 출력. 검증 핵심(GUI 없이 N grid 확인).
fn run_workspace_mirror_dump(
    mut conn: StreamConnection,
    mut mirrors: Vec<(u32, Terminal)>,
    placeholders: Vec<(u32, String)>,
    dump_after: Option<u64>,
    send: Option<&str>,
    send_to: Option<u32>,
) -> Result<AttachExit> {
    let collect_ms = dump_after.unwrap_or(500);

    // 초기 입력 1 회(지정 surface 로 surface-prefixed).
    if let Some(s) = send {
        match send_to {
            Some(sid) => conn.send(
                StreamTag::Data,
                &stream::encode_mux(sid, &decode_escapes(s)),
            )?,
            None => {
                eprintln!(
                    "workspace 모드의 --send 는 --send-to <surface_id> 와 함께 써야 합니다 (무시)."
                )
            }
        }
    }

    let writer = conn.try_clone_writer()?;
    let (tx, rx) = mpsc::channel::<StreamFrame>();
    let reader = thread::spawn(move || {
        while let Ok(frame) = conn.recv() {
            let stop = frame.tag == StreamTag::Detach;
            if tx.send(frame).is_err() || stop {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_millis(collect_ms);
    let mut forced = false;
    let mut disconnected = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(frame) => match frame.tag {
                StreamTag::Data => {
                    if let Some((sid, payload)) = stream::decode_mux(&frame.payload)
                        && let Some((_, m)) = mirrors.iter_mut().find(|(id, _)| *id == sid)
                    {
                        m.feed_bytes(payload);
                    }
                }
                StreamTag::Control => {
                    if String::from_utf8_lossy(&frame.payload).contains("force_detached") {
                        forced = true;
                        break;
                    }
                }
                StreamTag::Detach => {
                    forced = true;
                    break;
                }
                // heartbeat — read 자체가 이미 소켓 read timeout 을 리셋하므로 별도
                // 처리 불필요. 이 dump 는 기본 500ms 로 짧게 끝나 client 발 Ping 송신은
                // 두지 않았다(HEARTBEAT_TIMEOUT 20s 이내).
                StreamTag::Ping => {}
            },
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                disconnected = true;
                break;
            }
        }
    }

    // 각 surface 화면을 섹션 헤더와 함께 stdout 으로 — 검증 grep 용.
    for (sid, m) in &mirrors {
        println!("=== surface {sid} ===");
        println!("{}", m.screen_text(true));
    }
    for (sid, kind) in &placeholders {
        println!("=== surface {sid} (placeholder: {kind}) ===");
    }

    let mut writer = writer;
    if !forced && !disconnected {
        let _ = stream::write_frame(&mut writer, StreamTag::Detach, &[]); // best-effort detach 통지 — 무시
    } else if forced {
        eprintln!("force-detached by server");
    }
    let _ = reader.join(); // reader 스레드 join 실패(패닉) 무시 — 종료 경로
    Ok(if disconnected {
        AttachExit::Disconnected
    } else {
        AttachExit::Completed
    })
}

/// mirror-dump 모드: 출력을 수집해 mirror grid 재구성 → stdout 출력.
/// 1회성이라 항상 `Completed` 를 반환하지만, deadline 전에 reader 가 끊기면
/// `Disconnected`(터널/서버 단절)로 보고해 SSH 재연결이 가능하게 한다.
fn run_mirror_dump(
    mut conn: StreamConnection,
    cols: usize,
    rows: usize,
    dump_after: Option<u64>,
    send: Option<&str>,
) -> Result<AttachExit> {
    let collect_ms = dump_after.unwrap_or(500);

    // 초기 입력 1 회(비대화형 검증용).
    if let Some(s) = send {
        conn.send(StreamTag::Data, &decode_escapes(s))?;
    }

    // reader thread → channel; 메인은 deadline 까지 수집해 mirror 에 feed.
    // (소켓 read timeout 은 프레임 중간에 잘릴 수 있어 thread+channel 로 분리.)
    let writer = conn.try_clone_writer()?;
    let (tx, rx) = mpsc::channel::<StreamFrame>();
    let reader = thread::spawn(move || {
        while let Ok(frame) = conn.recv() {
            let stop = frame.tag == StreamTag::Detach;
            if tx.send(frame).is_err() || stop {
                break;
            }
        }
    });

    let mut mirror = Terminal::new_detached(cols, rows);
    let deadline = Instant::now() + Duration::from_millis(collect_ms);
    let mut forced = false;
    let mut disconnected = false;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok(frame) => match frame.tag {
                StreamTag::Data => {
                    mirror.feed_bytes(&frame.payload);
                }
                StreamTag::Control => {
                    if String::from_utf8_lossy(&frame.payload).contains("force_detached") {
                        forced = true;
                        break;
                    }
                }
                StreamTag::Detach => {
                    forced = true;
                    break;
                }
                // heartbeat — read 자체가 이미 소켓 read timeout 을 리셋하므로 별도
                // 처리 불필요. 이 dump 도 기본 500ms 로 짧게 끝나 client 발 Ping 송신은
                // 두지 않았다(HEARTBEAT_TIMEOUT 20s 이내).
                StreamTag::Ping => {}
            },
            Err(mpsc::RecvTimeoutError::Timeout) => break, // 정상: deadline 도달.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // reader 스레드 종료 = 소켓 끊김(터널/서버 단절) → 재연결 대상.
                disconnected = true;
                break;
            }
        }
    }

    // mirror 화면을 stdout 으로 — 검증 핵심(GUI 없이 grid 확인).
    println!("{}", mirror.screen_text(true));

    // 정상 종료 시 detach 통지(force-detach/단절이면 서버가 이미 끊음).
    let mut writer = writer;
    if !forced && !disconnected {
        let _ = stream::write_frame(&mut writer, StreamTag::Detach, &[]); // best-effort detach 통지 — 무시
    } else if forced {
        eprintln!("force-detached by server");
    }
    let _ = reader.join(); // reader 스레드 join 실패(패닉) 무시 — 종료 경로
    Ok(if disconnected {
        AttachExit::Disconnected
    } else {
        AttachExit::Completed
    })
}

/// raw 브리지 내부 이벤트 — stdin 스레드와 server reader 스레드가 하나의 채널에
/// merge 해 보낸다. main 은 이 채널에만 블록하므로 더 이상 stdin syscall 에 직접
/// 갇히지 않고, 서버 쪽 단절을 즉시 감지해 `AttachExit::Disconnected` 를 반환할 수
/// 있다(mirror-dump 의 `rx.recv_timeout` 패턴과 동일한 아이디어 — 여기선 deadline
/// 이 없으므로 `recv()`).
enum RawEvent {
    Stdin(Vec<u8>),
    StdinEof,
    Server(StreamFrame),
    /// server reader 의 `conn.recv()` 가 `Err` — 조용한 끊김(재연결 대상).
    ServerRecvErr,
}

/// raw 브리지 모드: stdin→서버 입력, 서버 출력→stdout. detach 키 `Ctrl+\`(0x1c).
/// 단계 4 옵션(완전 raw TTY 설정은 추후) — 기본 passthrough.
///
/// stdin 과 server 출력을 각각 별도 스레드로 분리해 하나의 `mpsc` 채널에 merge 한다
/// (mirror-dump 와 동일 패턴). main 은 채널 `recv()` 에만 블록하므로 서버 단절을
/// `RawEvent::ServerRecvErr` 로 즉시 감지해 `AttachExit::Disconnected` 를 정상
/// 반환할 수 있다 — `process::exit` 는 쓰지 않는다.
///
/// stdin 스레드는 blocking `read()` 를 깨울 방법이 없어 종료 신호를 못 받는다 —
/// 재연결 루프에서 이 함수가 재호출될 때마다 이전 stdin 스레드는 버려진 채 계속
/// blocking read 에 갇혀 남는다(join 하지 않음). 재연결 횟수만큼 좀비 스레드가
/// 쌓이지만 CPU 를 쓰지 않고 스택 메모리만 소모한다 — 완전 non-blocking stdin(플랫폼별
/// poll/self-pipe/WaitForMultipleObjects)은 크로스플랫폼 복잡도가 커 이번 스코프에서
/// 배제했다.
fn run_raw_bridge(conn: StreamConnection, send: Option<&str>) -> Result<AttachExit> {
    // 입력/Detach/heartbeat 송신용 단일 writer(여러 스레드가 공유 — 프레임 인터리브 방지).
    let writer = Arc::new(Mutex::new(conn.try_clone_writer()?));
    if let Some(s) = send {
        let mut w = writer.lock().unwrap();
        stream::write_frame(&mut *w, StreamTag::Data, &decode_escapes(s))?;
        drop(w);
    }

    // heartbeat thread: 이 세션은 raw 브리지라 stdin 이 조용하면(사용자가 그냥 보기만
    // 하는 동안) 오래 idle 할 수 있다 — 주기적으로 Ping 을 보내 서버측 read timeout 을
    // 갱신한다(반대 방향은 서버 write thread 의 동일 로직이 처리). 별도 종료 신호 없이
    // 프로세스 종료(main 함수 반환)에 맡긴다 — CLI 프로세스라 스레드 정리를 기다릴
    // 이유가 없다.
    {
        let writer = writer.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(stream::HEARTBEAT_INTERVAL);
                let sent = match writer.lock() {
                    Ok(mut w) => stream::write_frame(&mut *w, StreamTag::Ping, &[]).is_ok(),
                    Err(_) => false,
                };
                if !sent {
                    break;
                }
            }
        });
    }

    let (tx, rx) = mpsc::channel::<RawEvent>();

    // stdin thread: blocking read → 채널. EOF/에러 시 1 회 통지 후 스레드 종료.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut stdin = std::io::stdin();
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(RawEvent::StdinEof);
                        break;
                    }
                    Ok(n) => {
                        if tx.send(RawEvent::Stdin(buf[..n].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(RawEvent::StdinEof);
                        break;
                    }
                }
            }
        });
    }

    // server reader thread: 서버 출력 → 채널. `conn.recv()` 가 `Err` 면 명시적으로
    // `ServerRecvErr` 를 보낸다(채널 drop 감지에 기대지 않음 — tx clone 이 stdin
    // 스레드에도 남아있어 drop 만으로는 신호가 안 된다).
    {
        let mut conn = conn;
        let tx = tx.clone();
        thread::spawn(move || {
            loop {
                match conn.recv() {
                    Ok(frame) => {
                        let is_detach = frame.tag == StreamTag::Detach;
                        if tx.send(RawEvent::Server(frame)).is_err() || is_detach {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(RawEvent::ServerRecvErr);
                        break;
                    }
                }
            }
        });
    }
    drop(tx); // 남은 건 두 스레드가 쥔 clone 뿐 — 원본은 더 필요 없음.

    // main: 채널 이벤트를 기다린다(더 이상 stdin syscall 에 직접 블록하지 않음).
    raw_bridge_main_loop(rx, writer)
}

/// `run_raw_bridge` 의 이벤트 디스패치 루프 — stdin/server 스레드가 실제 OS 자원에
/// 블록하는 부분과 분리해뒀다. 채널 로직만 이 함수가 담당하므로, 유닛 테스트가
/// 실제 stdin/소켓 없이 `RawEvent` 를 직접 채널에 흘려 종료 사유 판정을 검증할 수
/// 있다(아래 `raw_bridge_tests`).
fn raw_bridge_main_loop(
    rx: mpsc::Receiver<RawEvent>,
    writer: Arc<Mutex<TcpStream>>,
) -> Result<AttachExit> {
    let stdout = std::io::stdout();
    loop {
        match rx.recv() {
            Ok(RawEvent::Stdin(data)) => {
                if let Some(pos) = data.iter().position(|&b| b == 0x1c) {
                    // Ctrl+\ 이전 바이트만 보내고 detach.
                    if pos > 0
                        && let Ok(mut w) = writer.lock()
                    {
                        let _ = stream::write_frame(&mut *w, StreamTag::Data, &data[..pos]); // 종료 경로 best-effort 송신 — 무시
                    }
                    if let Ok(mut w) = writer.lock() {
                        let _ = stream::write_frame(&mut *w, StreamTag::Detach, &[]); // best-effort detach 통지 — 무시
                    }
                    return Ok(AttachExit::Completed);
                }
                let write_ok = match writer.lock() {
                    Ok(mut w) => stream::write_frame(&mut *w, StreamTag::Data, &data).is_ok(),
                    Err(_) => false,
                };
                if !write_ok {
                    return Ok(AttachExit::Completed);
                }
            }
            Ok(RawEvent::StdinEof) => return Ok(AttachExit::Completed),
            Ok(RawEvent::Server(frame)) => match frame.tag {
                StreamTag::Data => {
                    let mut h = stdout.lock();
                    let _ = h.write_all(&frame.payload); // best-effort stdout 미러 — 무시
                    let _ = h.flush(); // best-effort flush — 무시
                }
                StreamTag::Detach => return Ok(AttachExit::Completed),
                StreamTag::Control => {
                    if String::from_utf8_lossy(&frame.payload).contains("force_detached") {
                        eprintln!("\r\nforce-detached by server");
                        return Ok(AttachExit::Completed);
                    }
                }
                // heartbeat — read 자체가 이미 소켓 read timeout 을 리셋하므로 별도
                // 처리 불필요.
                StreamTag::Ping => {}
            },
            Ok(RawEvent::ServerRecvErr) => return Ok(AttachExit::Disconnected),
            // 두 송신 스레드가 모두 죽어야만 발생 — 사실상 도달 불가(stdin 스레드는
            // 항상 종료 전 StdinEof 를 보내고, 있는대로 서버 스레드도 ServerRecvErr
            // 를 보낸다).
            Err(_) => return Ok(AttachExit::Completed),
        }
    }
}

/// 입력 문자열의 escape 를 raw 바이트로 디코딩: `\r \n \t \0 \\ \xNN`.
/// (요청-응답 경로의 `unescape` 와 달리 `\xNN` 제어바이트를 지원 — raw 입력 주입용.)
fn decode_escapes(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'r' => {
                    out.push(b'\r');
                    i += 2;
                }
                b'n' => {
                    out.push(b'\n');
                    i += 2;
                }
                b't' => {
                    out.push(b'\t');
                    i += 2;
                }
                b'0' => {
                    out.push(0);
                    i += 2;
                }
                b'\\' => {
                    out.push(b'\\');
                    i += 2;
                }
                b'x' if i + 3 < bytes.len() => {
                    let hi = (bytes[i + 2] as char).to_digit(16);
                    let lo = (bytes[i + 3] as char).to_digit(16);
                    if let (Some(h), Some(l)) = (hi, lo) {
                        out.push((h * 16 + l) as u8);
                        i += 4;
                    } else {
                        out.push(b'\\');
                        i += 1;
                    }
                }
                _ => {
                    out.push(b'\\');
                    i += 1;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::decode_escapes;

    #[test]
    fn decode_basic_escapes() {
        assert_eq!(decode_escapes("echo hi\\r"), b"echo hi\r".to_vec());
        assert_eq!(decode_escapes("a\\tb\\n"), b"a\tb\n".to_vec());
    }

    #[test]
    fn decode_hex_control_bytes() {
        // \x1b = ESC, \x1c = Ctrl+\
        assert_eq!(decode_escapes("\\x1b[A"), vec![0x1b, b'[', b'A']);
        assert_eq!(decode_escapes("x\\x1cy"), vec![b'x', 0x1c, b'y']);
    }

    #[test]
    fn decode_passthrough_unknown() {
        assert_eq!(decode_escapes("a\\qb"), b"a\\qb".to_vec());
        assert_eq!(decode_escapes("plain"), b"plain".to_vec());
    }
}

/// `raw_bridge_main_loop` 채널 로직 단위 테스트 — 실제 stdin/소켓 대신 `RawEvent`
/// 를 채널에 직접 흘려 종료 사유별 `AttachExit` 판정을 검증한다. 이 항목이 고치는
/// 결함은 "서버 쪽 단절을 감지해도 raw 브리지가 `AttachExit::Disconnected` 를 반환하지
/// 못하고(과거엔 `process::exit` 로 프로세스 자체가 죽어 반환 지점에 도달 못함)
/// 백오프 재연결이 발동하지 않는" 것이었다 — 아래 `server_recv_err_reports_disconnected`
/// 가 바로 그 회귀를 잡는다.
#[cfg(test)]
mod raw_bridge_tests {
    use std::io::Read;
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex, mpsc};
    use std::thread;

    use tasty_ipc::stream::{StreamFrame, StreamTag};

    use super::{AttachExit, RawEvent, raw_bridge_main_loop};

    /// writer.lock() 이 실제로 잠글 대상이 필요할 뿐 내용은 검사하지 않으므로,
    /// loopback 소켓 한쪽을 열어 반대쪽에서 계속 읽어 버림으로써 send-buffer 가
    /// 차 write 가 막히는 일이 없게 한다.
    fn dummy_writer() -> Arc<Mutex<TcpStream>> {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let client = TcpStream::connect(addr).expect("connect");
        let (mut server_side, _) = listener.accept().expect("accept");
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = server_side.read(&mut buf) {
                if n == 0 {
                    break;
                }
            }
        });
        Arc::new(Mutex::new(client))
    }

    #[test]
    fn server_recv_err_reports_disconnected() {
        let (tx, rx) = mpsc::channel::<RawEvent>();
        tx.send(RawEvent::ServerRecvErr).unwrap();
        drop(tx);

        let exit = raw_bridge_main_loop(rx, dummy_writer()).unwrap();
        assert!(matches!(exit, AttachExit::Disconnected));
    }

    #[test]
    fn server_detach_frame_reports_completed() {
        let (tx, rx) = mpsc::channel::<RawEvent>();
        tx.send(RawEvent::Server(StreamFrame::new(
            StreamTag::Detach,
            vec![],
        )))
        .unwrap();
        drop(tx);

        let exit = raw_bridge_main_loop(rx, dummy_writer()).unwrap();
        assert!(matches!(exit, AttachExit::Completed));
    }

    #[test]
    fn server_force_detached_control_reports_completed() {
        let (tx, rx) = mpsc::channel::<RawEvent>();
        let payload = br#"{"event":"force_detached"}"#.to_vec();
        tx.send(RawEvent::Server(StreamFrame::new(
            StreamTag::Control,
            payload,
        )))
        .unwrap();
        drop(tx);

        let exit = raw_bridge_main_loop(rx, dummy_writer()).unwrap();
        assert!(matches!(exit, AttachExit::Completed));
    }

    #[test]
    fn stdin_eof_reports_completed() {
        let (tx, rx) = mpsc::channel::<RawEvent>();
        tx.send(RawEvent::StdinEof).unwrap();
        drop(tx);

        let exit = raw_bridge_main_loop(rx, dummy_writer()).unwrap();
        assert!(matches!(exit, AttachExit::Completed));
    }

    #[test]
    fn stdin_detach_key_reports_completed() {
        let (tx, rx) = mpsc::channel::<RawEvent>();
        tx.send(RawEvent::Stdin(vec![b'a', 0x1c, b'b'])).unwrap(); // Ctrl+\ 포함
        drop(tx);

        let exit = raw_bridge_main_loop(rx, dummy_writer()).unwrap();
        assert!(matches!(exit, AttachExit::Completed));
    }

    /// 서버 프레임(Data)이 먼저 여러 번 오고, 그 다음 단절되는 순서도 정상 처리되는지
    /// — 재연결 전까지 정상 출력이 이어지다 끊김만 감지하는 실사용 패턴.
    #[test]
    fn data_frames_then_server_recv_err_reports_disconnected() {
        let (tx, rx) = mpsc::channel::<RawEvent>();
        tx.send(RawEvent::Server(StreamFrame::new(
            StreamTag::Data,
            b"hello".to_vec(),
        )))
        .unwrap();
        tx.send(RawEvent::Server(StreamFrame::new(StreamTag::Ping, vec![])))
            .unwrap();
        tx.send(RawEvent::ServerRecvErr).unwrap();
        drop(tx);

        let exit = raw_bridge_main_loop(rx, dummy_writer()).unwrap();
        assert!(matches!(exit, AttachExit::Disconnected));
    }
}
