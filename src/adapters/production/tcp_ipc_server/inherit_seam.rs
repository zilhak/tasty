//! 시험 전용(Windows): accept 루프와 연결 스레드가 소켓 상속을 다루는 자리. 서버 포트별로 걸고, 걸지
//! 않은 서버(다른 시험)에서는 아무것도 하지 않는다. 끄기는 언제나 제품 호출이 하고, 이음새는 실패만 주입한다.

use std::io;
use std::net::TcpStream;
use std::sync::{Mutex, mpsc};

/// 시험이 본 소켓의 상속 여부.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Seen {
    /// 연결 스레드가 받은 reader·writer.
    Served { reader: bool, writer: bool },
    /// 포화 거절을 쓰기 직전의 소켓.
    Refused { socket: bool },
}

struct Armed {
    port: u16,
    clear_fails: bool,
    seen: mpsc::Sender<Seen>,
}

static ARMED: Mutex<Vec<Armed>> = Mutex::new(Vec::new());

/// 놓으면 그 포트의 자리를 푼다.
pub(super) struct Disarm(u16);

impl Drop for Disarm {
    fn drop(&mut self) {
        let mut armed = ARMED.lock().expect("inherit seam");
        armed.retain(|armed| armed.port != self.0);
    }
}

/// `port` 서버에 건다. accept 직후 상속을 켜고, `clear_fails` 면 끄기를 실패로 돌려준다.
pub(super) fn arm(port: u16, clear_fails: bool) -> (Disarm, mpsc::Receiver<Seen>) {
    let (seen, receiver) = mpsc::channel();
    ARMED.lock().expect("inherit seam").push(Armed {
        port,
        clear_fails,
        seen,
    });
    (Disarm(port), receiver)
}

fn with_armed<T>(stream: &TcpStream, f: impl FnOnce(&Armed) -> T) -> Option<T> {
    let port = stream.local_addr().ok()?.port();
    let armed = ARMED.lock().expect("inherit seam");
    armed.iter().find(|armed| armed.port == port).map(f)
}

fn inheritable(stream: &TcpStream) -> bool {
    tasty_utils::socket_inherit::is_inheritable(stream).expect("handle information")
}

/// accept 직후, 끄기 전.
pub(super) fn after_accept(stream: &TcpStream) {
    with_armed(stream, |_| {
        tasty_utils::socket_inherit::make_inheritable(stream).expect("make inheritable")
    });
}

/// accept 루프의 끄기 뒤에 주입할 실패. 끄기 자체는 제품 호출이 그대로 한다.
pub(super) fn injected_failure(stream: &TcpStream) -> io::Result<()> {
    if with_armed(stream, |armed| armed.clear_fails).unwrap_or(false) {
        return Err(io::Error::other("inherit seam: clearing fails on purpose"));
    }
    Ok(())
}

/// 연결 스레드가 reader·writer를 받은 직후.
pub(super) fn served(reader: &TcpStream, writer: &TcpStream) {
    with_armed(writer, |armed| {
        // 받는 쪽은 시험이다. 시험이 먼저 끝나 수신자가 사라진 경우에만 실패하고, 그때는 볼 쪽이 없어 버린다.
        let _sent = armed.seen.send(Seen::Served {
            reader: inheritable(reader),
            writer: inheritable(writer),
        });
    });
}

/// 포화 거절을 쓰기 직전.
pub(super) fn refused(stream: &TcpStream) {
    with_armed(stream, |armed| {
        // 위 `served`와 같다: 수신자(시험)가 먼저 끝난 경우에만 실패하고 버린다.
        let _sent = armed.seen.send(Seen::Refused {
            socket: inheritable(stream),
        });
    });
}
