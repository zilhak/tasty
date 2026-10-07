//! 거절 응답을 보낸 연결을 닫기 전에 받은 바이트를 비운다(Windows).
//!
//! Windows 는 읽지 않은 수신 데이터가 남은 소켓을 닫으면 FIN 대신 RST 를 보낸다. 상대 쪽 Windows
//! 스택은 RST 를 받으면 아직 읽지 않은 수신 데이터까지 버리고 `WSAECONNRESET` 을 돌려주므로, 상한을
//! 넘은 줄을 보내던 클라이언트는 -32060 거절을 읽지 못한다. Linux 는 RST 전에 받은 데이터를 먼저
//! 읽게 해 주므로 이 처리가 필요 없다.
//!
//! 쓰기 쪽을 먼저 닫아(FIN) 응답이 끝났음을 알리고, 상대가 이미 보낸 바이트를 짧게 읽어 버린 뒤
//! 닫는다. 끝없이 보내는 상대 때문에 연결 스레드가 묶이지 않도록 시간과 바이트에 상한을 둔다.
//! 상한을 넘으면 그대로 닫으며, 그때의 RST 는 받아들인다(최선 노력).

use std::io::Read;
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

/// 비우는 전체 시간 상한.
const DRAIN_WINDOW: Duration = Duration::from_millis(500);
/// 새 바이트가 이만큼 오지 않으면 상대가 보내기를 마친 것으로 보고 멈춘다.
const DRAIN_IDLE: Duration = Duration::from_millis(50);
/// 비우는 바이트 상한. 요청 한 줄의 상한과 같다.
const DRAIN_MAX_BYTES: usize = crate::ipc::protocol::MAX_REQUEST_LINE_BYTES;

pub(super) fn drain_before_close(stream: &mut TcpStream) {
    if let Err(e) = stream.shutdown(Shutdown::Write) {
        tracing::debug!("IPC refusal half-close failed: {e}");
        return;
    }
    if let Err(e) = stream.set_read_timeout(Some(DRAIN_IDLE)) {
        tracing::debug!("IPC refusal drain could not set a read timeout: {e}");
        return;
    }
    let deadline = Instant::now() + DRAIN_WINDOW;
    let mut drained = 0usize;
    let mut buf = [0u8; 64 * 1024];
    while Instant::now() < deadline && drained < DRAIN_MAX_BYTES {
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => drained += n,
            // 기다려도 오지 않으면(시간 초과) 상대가 보내기를 마친 것이다.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return;
            }
            Err(e) => {
                tracing::debug!("IPC refusal drain ended on a read error: {e}");
                return;
            }
        }
    }
    tracing::debug!("IPC refusal drain stopped at its limit after {drained} bytes");
}
