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
use tasty_ipc::port_file as pf;
use tasty_ipc::stream::{self, STREAM_PROTO, StreamFrame, StreamTag};
use tasty_terminal::Terminal;

use crate::stream::StreamConnection;

/// `tasty attach <surface>` 진입점(mirror-dump / raw). force-detach 는 별도(JSON-RPC).
pub fn run_attach(
    surface: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    raw: bool,
    port_file: Option<&str>,
) -> Result<()> {
    let port = pf::read_port_file_from(port_file)?;
    let sock = TcpStream::connect(format!("127.0.0.1:{port}")).map_err(|e| {
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

/// mirror-dump 모드: 출력을 수집해 mirror grid 재구성 → stdout 출력.
fn run_mirror_dump(
    mut conn: StreamConnection,
    cols: usize,
    rows: usize,
    dump_after: Option<u64>,
    send: Option<&str>,
) -> Result<()> {
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
        loop {
            match conn.recv() {
                Ok(frame) => {
                    let stop = frame.tag == StreamTag::Detach;
                    if tx.send(frame).is_err() || stop {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut mirror = Terminal::new_detached(cols, rows);
    let deadline = Instant::now() + Duration::from_millis(collect_ms);
    let mut forced = false;
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
            Err(_) => break, // timeout or reader gone
        }
    }

    // mirror 화면을 stdout 으로 — 검증 핵심(GUI 없이 grid 확인).
    println!("{}", mirror.screen_text());

    // 정상 종료 시 detach 통지(force-detach 면 서버가 이미 끊음).
    let mut writer = writer;
    if !forced {
        let _ = stream::write_frame(&mut writer, StreamTag::Detach, &[]);
    } else {
        eprintln!("force-detached by server");
    }
    let _ = reader.join();
    Ok(())
}

/// raw 브리지 모드: stdin→서버 입력, 서버 출력→stdout. detach 키 `Ctrl+\`(0x1c).
/// 단계 4 옵션(완전 raw TTY 설정은 추후) — 기본 passthrough.
fn run_raw_bridge(conn: StreamConnection, send: Option<&str>) -> Result<()> {
    let mut writer = conn.try_clone_writer()?;
    if let Some(s) = send {
        stream::write_frame(&mut writer, StreamTag::Data, &decode_escapes(s))?;
    }

    // reader thread: 서버 출력 → stdout. force-detach/Detach 시 프로세스 종료.
    let mut conn = conn;
    let reader = thread::spawn(move || {
        let stdout = std::io::stdout();
        loop {
            match conn.recv() {
                Ok(frame) => match frame.tag {
                    StreamTag::Data => {
                        let mut h = stdout.lock();
                        let _ = h.write_all(&frame.payload);
                        let _ = h.flush();
                    }
                    StreamTag::Detach => break,
                    StreamTag::Control => {
                        if String::from_utf8_lossy(&frame.payload).contains("force_detached") {
                            eprintln!("\r\nforce-detached by server");
                            break;
                        }
                    }
                    StreamTag::Ping => {}
                },
                Err(_) => break,
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
                        let _ = stream::write_frame(&mut writer, StreamTag::Data, &buf[..pos]);
                    }
                    let _ = stream::write_frame(&mut writer, StreamTag::Detach, &[]);
                    break;
                }
                if stream::write_frame(&mut writer, StreamTag::Data, &buf[..n]).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = reader.join();
    Ok(())
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
