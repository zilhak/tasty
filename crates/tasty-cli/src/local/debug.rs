//! debug 빌드 전용 클라이언트 주도 실행.
//!
//! 선언(`DebugCommands` 등 clap enum)은 `commands/debug.rs` 에 남고 여기엔 실행만
//! 온다. `#![cfg(debug_assertions)]` 는 이 모듈 트리 전체에 걸린다 — 원칙 1 ②
//! (사용자 입력 재현은 release 표면 밖).

#![cfg(debug_assertions)]

pub mod attach;

/// Run the `debug stream-echo` verification: connect, upgrade to a streaming
/// channel, send `count` data frames, and confirm each is echoed back by the
/// host's main loop. Returns an error on connect/handshake failure or mismatch.
///
/// This exercises the *transport infrastructure* (server→client push), not user
/// input simulation, so it lives in the debug-isolated CLI surface per the
/// agent/user action separation policy.
pub fn run_stream_echo(payload: &str, count: u32, port_file: Option<&str>) -> anyhow::Result<()> {
    use std::net::TcpStream;

    use tasty_ipc::port_file as pf;
    use tasty_ipc::stream::{STREAM_PROTO, StreamTag};

    use tasty_ipc::client::StreamConnection;

    let port = pf::read_port_file_from(port_file)?;
    let sock = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port,
            e
        )
    })?;

    let (mut conn, client_id) = StreamConnection::open(sock, STREAM_PROTO)?;
    println!("stream opened (client_id={client_id}, proto={STREAM_PROTO})");

    for i in 0..count {
        let msg = format!("{payload}#{i}");
        conn.send(StreamTag::Data, msg.as_bytes())?;
        let frame = conn.recv()?;
        if frame.tag != StreamTag::Data {
            anyhow::bail!("frame {i}: expected Data tag, got {:?}", frame.tag);
        }
        if frame.payload != msg.as_bytes() {
            anyhow::bail!(
                "frame {i}: echo mismatch — sent {:?}, got {:?}",
                msg,
                String::from_utf8_lossy(&frame.payload)
            );
        }
        println!(
            "echo {}/{} ok: {}",
            i + 1,
            count,
            String::from_utf8_lossy(&frame.payload)
        );
    }

    conn.detach()?;
    println!("all {count} frame(s) echoed back; detached");
    Ok(())
}
