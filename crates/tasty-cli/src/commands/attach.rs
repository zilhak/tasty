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
use std::sync::mpsc;
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
        let remote_port =
            match ssh::discover_remote_port(&ssh, &target, remote_tasty, mode, verify, debug) {
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
        let remote_port =
            match ssh::discover_remote_port(&ssh, &target, remote_tasty, mode, verify, debug) {
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
        println!("{}", m.screen_text());
    }
    for (sid, kind) in &placeholders {
        println!("=== surface {sid} (placeholder: {kind}) ===");
    }

    let mut writer = writer;
    if !forced && !disconnected {
        let _ = stream::write_frame(&mut writer, StreamTag::Detach, &[]);  // best-effort detach 통지 — 무시
    } else if forced {
        eprintln!("force-detached by server");
    }
    let _ = reader.join();  // reader 스레드 join 실패(패닉) 무시 — 종료 경로
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
    println!("{}", mirror.screen_text());

    // 정상 종료 시 detach 통지(force-detach/단절이면 서버가 이미 끊음).
    let mut writer = writer;
    if !forced && !disconnected {
        let _ = stream::write_frame(&mut writer, StreamTag::Detach, &[]);  // best-effort detach 통지 — 무시
    } else if forced {
        eprintln!("force-detached by server");
    }
    let _ = reader.join();  // reader 스레드 join 실패(패닉) 무시 — 종료 경로
    Ok(if disconnected {
        AttachExit::Disconnected
    } else {
        AttachExit::Completed
    })
}

/// raw 브리지 모드: stdin→서버 입력, 서버 출력→stdout. detach 키 `Ctrl+\`(0x1c).
/// 단계 4 옵션(완전 raw TTY 설정은 추후) — 기본 passthrough.
///
/// 항상 `Completed` 를 반환한다(사용자 detach/EOF). 서버/터널이 먼저 닫으면 reader
/// 스레드가 프로세스를 종료하므로 raw 모드는 attach 후 자동 재연결을 하지 않는다
/// (블로킹 stdin 을 깰 수 없음 — 완전 raw TTY + 재연결 UX 는 후속, plan §6.4 R6).
fn run_raw_bridge(conn: StreamConnection, send: Option<&str>) -> Result<AttachExit> {
    let mut writer = conn.try_clone_writer()?;
    if let Some(s) = send {
        stream::write_frame(&mut writer, StreamTag::Data, &decode_escapes(s))?;
    }

    // reader thread: 서버 출력 → stdout. force-detach/Detach 시 프로세스 종료.
    let mut conn = conn;
    let reader = thread::spawn(move || {
        let stdout = std::io::stdout();
        while let Ok(frame) = conn.recv() {
            match frame.tag {
                StreamTag::Data => {
                    let mut h = stdout.lock();
                    let _ = h.write_all(&frame.payload);  // best-effort stdout 미러 — 무시
                    let _ = h.flush();  // best-effort flush — 무시
                }
                StreamTag::Detach => break,
                StreamTag::Control => {
                    if String::from_utf8_lossy(&frame.payload).contains("force_detached") {
                        eprintln!("\r\nforce-detached by server");
                        break;
                    }
                }
                StreamTag::Ping => {}
            }
        }
        std::process::exit(0);
    });

    // main: stdin → 서버. detach 전용 키 Ctrl+\ (0x1c) 감지 시 종료.
    let mut buf = [0u8; 4096];
    let mut stdin = std::io::stdin();
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => break, // stdin EOF
            Ok(n) => {
                if let Some(pos) = buf[..n].iter().position(|&b| b == 0x1c) {
                    // Ctrl+\ 이전 바이트만 보내고 detach.
                    if pos > 0 {
                        let _ = stream::write_frame(&mut writer, StreamTag::Data, &buf[..pos]);  // 종료 경로 best-effort 송신 — 무시
                    }
                    let _ = stream::write_frame(&mut writer, StreamTag::Detach, &[]);  // best-effort detach 통지 — 무시
                    break;
                }
                if stream::write_frame(&mut writer, StreamTag::Data, &buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = reader.join();  // reader 스레드 join 실패(패닉) 무시 — 종료 경로
    Ok(AttachExit::Completed)
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
