//! debug 빌드 전용 클라이언트 실행. 명령 선언은 commands/debug.rs에 있다.

#![cfg(debug_assertions)]

pub mod attach;

use crate::out::outln;

/// Run the `debug stream-echo` verification: connect, upgrade to a streaming
/// channel, send `count` data frames, and confirm each is echoed back by the
/// host's main loop. Returns an error on connect/handshake failure or mismatch.
pub fn run_stream_echo(payload: &str, count: u32, port_file: Option<&str>) -> anyhow::Result<()> {
    use std::net::TcpStream;

    use tasty_ipc::stream::{STREAM_PROTO, StreamTag};

    use tasty_ipc::client::StreamConnection;

    let port = crate::port_file::read_port(port_file)?;
    let sock = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "{}",
            tasty_i18n::t_fmt2(
                "cli.request.connect_failed",
                &port.to_string(),
                &e.to_string()
            )
        )
    })?;

    let (mut conn, client_id) = StreamConnection::open(sock, STREAM_PROTO)?;
    outln!("stream opened (client_id={client_id}, proto={STREAM_PROTO})")?;

    for i in 0..count {
        let msg = format!("{payload}#{i}");
        conn.send(StreamTag::Data, msg.as_bytes())?;
        let frame = conn.recv()?;
        if frame.tag != StreamTag::Data {
            anyhow::bail!(
                "{}",
                tasty_i18n::t_fmt2(
                    "cli.debug.frame_tag_mismatch",
                    &i.to_string(),
                    &format!("{:?}", frame.tag)
                )
            );
        }
        if frame.payload != msg.as_bytes() {
            anyhow::bail!(
                "{}",
                tasty_i18n::t_args(
                    "cli.debug.frame_echo_mismatch",
                    &[
                        &i.to_string(),
                        &format!("{msg:?}"),
                        &format!("{:?}", String::from_utf8_lossy(&frame.payload)),
                    ]
                )
            );
        }
        outln!(
            "echo {}/{} ok: {}",
            i + 1,
            count,
            String::from_utf8_lossy(&frame.payload)
        )?;
    }

    conn.detach()?;
    outln!("all {count} frame(s) echoed back; detached")?;
    Ok(())
}
