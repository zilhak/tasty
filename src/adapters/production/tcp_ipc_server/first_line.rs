//! 첫 요청 줄의 시간 상한 — 줄을 한 번도 안 보내는 연결이 자리를 영원히 쥐지 못하게 한다.
//!
//! 연결 상한([`super::MAX_CONCURRENT_CONNECTIONS`])이 센 자리는 연결 스레드가 끝나야 돌아온다.
//! 요청-응답 연결의 읽기에는 시간 상한이 없으므로, **연결만 열고 아무것도 안 보내는** peer 는
//! 그 자리를 영원히 쥔다. 그런 peer 가 상한만큼이면 정상 client 가 못 붙는다 — 쓰기 쪽에
//! [`super::RESPONSE_WRITE_TIMEOUT`] 이 막은 것과 같은 모양이 읽기 쪽에 남아 있었다.
//!
//! 상한은 **첫 줄에만** 건다. 한 번이라도 요청을 보낸 연결은 요청 사이에 얼마나 쉬어도 닫지
//! 않는다 — 요청 사이에 오래 쉬는 client(오래 붙어 있는 제3자 client, 사람이 치는 대화형
//! 도구)를 끊지 않으려는 호환 결정이다(ADR-0392). 그 대가로 "첫 줄을 보낸 뒤 쉬는" 연결은
//! 여전히 자리를 쥘 수 있고, 그것은 ADR 의 재검토 조건이다.
//!
//! 상한은 **읽기 한 번이 아니라 줄 전체**에 걸린다. 소켓의 read timeout 은 읽기 한 번마다
//! 새로 시작하므로, 그것만 걸면 한 바이트씩 상한보다 조금 빨리 흘려 보내는 peer 가 개행 없이
//! 영원히 버틴다. [`DeadlineReader`] 가 읽기마다 **남은 시간**으로 timeout 을 다시 건다.

use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::ipc::stream;

/// 연결이 열린 뒤 첫 요청 줄이 끝나야 하는 시간.
///
/// **값을 고르지 않고 [`stream::HEARTBEAT_TIMEOUT`] 에서 파생한다.** 그 상수가 답하는 물음이
/// 같다 — "이 loopback 소켓의 상대가 얼마나 진척을 안 내면 죽은 것으로 보는가". 같은 소켓의
/// 쓰기 상한([`super::RESPONSE_WRITE_TIMEOUT`])과 attach 의 읽기 상한도 같은 값을 쓴다. 정상
/// client(CLI · attach · 원격 browse)는 연결 직후 첫 줄을 보내므로 이 값에 닿지 않는다.
pub(super) const FIRST_LINE_IDLE_TIMEOUT: Duration = stream::HEARTBEAT_TIMEOUT;

/// 첫 줄 상한. 제품 값은 [`FIRST_LINE_IDLE_TIMEOUT`] 이고, debug 빌드에서만 환경변수로 바꿀
/// 수 있다 — 격리 인스턴스 시험이 20 초를 기다리지 않고 거절 갈래를 재게 하려는 것이다.
#[cfg(debug_assertions)]
pub(super) fn first_line_idle_timeout() -> Duration {
    super::debug_env_usize("TASTY_DEBUG_IPC_FIRST_LINE_IDLE_MS")
        .map_or(FIRST_LINE_IDLE_TIMEOUT, |ms| {
            Duration::from_millis(ms as u64)
        })
}

#[cfg(not(debug_assertions))]
pub(super) fn first_line_idle_timeout() -> Duration {
    FIRST_LINE_IDLE_TIMEOUT
}

/// 기한까지만 읽는 `BufRead`. 소켓에서 새로 읽어야 할 때마다 남은 시간을 read timeout 으로
/// 다시 걸고, 기한이 지났으면 읽지 않고 `TimedOut` 을 낸다. 이미 버퍼에 든 바이트는 시간과
/// 무관하게 내준다.
///
/// 읽기가 끝나면 호출자가 [`clear_read_timeout`] 으로 timeout 을 걷어야 한다 — 소켓 옵션은
/// 이 래퍼가 사라져도 남는다.
pub(super) struct DeadlineReader<'a> {
    inner: &'a mut BufReader<TcpStream>,
    deadline: Instant,
}

impl<'a> DeadlineReader<'a> {
    pub(super) fn new(inner: &'a mut BufReader<TcpStream>, within: Duration) -> Self {
        Self {
            inner,
            deadline: Instant::now() + within,
        }
    }

    fn arm_remaining(&self) -> std::io::Result<()> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        // 0 은 `set_read_timeout` 이 거절하는 값이라 여기서 기한 만료로 가른다.
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.inner.get_ref().set_read_timeout(Some(remaining))
    }
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.inner.buffer().is_empty() {
            self.arm_remaining()?;
        }
        self.inner.read(buf)
    }
}

impl BufRead for DeadlineReader<'_> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.inner.buffer().is_empty() {
            self.arm_remaining()?;
        }
        self.inner.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.inner.consume(amt);
    }
}

/// 이 오류가 읽기 기한 만료인가. 플랫폼마다 `WouldBlock`(Unix) 또는 `TimedOut`(Windows)으로 온다.
pub(super) fn is_idle_expiry(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// 첫 줄 뒤에 read timeout 을 걷는다. 걷지 못하면 요청 사이의 쉼에 상한이 생겨 오래 붙어 있는
/// client 가 끊기므로, 그때는 연결을 닫는 쪽을 고른다(`false`) — 조용히 다른 계약으로 도는
/// 것보다 첫 요청에서 드러나는 편이 낫다.
pub(super) fn clear_read_timeout(reader: &BufReader<TcpStream>) -> bool {
    match reader.get_ref().set_read_timeout(None) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("IPC could not clear the first-line read timeout, closing: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    fn pair() -> (TcpStream, BufReader<TcpStream>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let client = TcpStream::connect(listener.local_addr().expect("addr")).expect("connect");
        let (server, _) = listener.accept().expect("accept");
        (client, BufReader::new(server))
    }

    fn read_line_within(
        reader: &mut BufReader<TcpStream>,
        within: Duration,
    ) -> std::io::Result<usize> {
        let mut line = String::new();
        DeadlineReader::new(reader, within).read_line(&mut line)
    }

    // 아무것도 안 보내는 peer — 기한에서 만료로 끝난다(무기한 블록이 아니다).
    #[test]
    fn a_silent_peer_expires_at_the_deadline() {
        let (_client, mut reader) = pair();
        let started = Instant::now();
        let err = read_line_within(&mut reader, Duration::from_millis(100)).expect_err("expires");
        assert!(is_idle_expiry(&err), "{err:?}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the deadline did not bound the read"
        );
    }

    // 한 바이트씩 기한보다 짧은 간격으로 흘리는 peer — 읽기 한 번의 timeout 만 걸었다면 영원히
    // 버티는 갈래다. 기한은 줄 전체에 걸리므로 여기서도 만료로 끝나야 한다.
    #[test]
    fn a_trickling_peer_cannot_stretch_the_deadline() {
        let (mut client, mut reader) = pair();
        let feeder = std::thread::spawn(move || {
            for _ in 0..40 {
                if client.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        let started = Instant::now();
        let err = read_line_within(&mut reader, Duration::from_millis(200)).expect_err("expires");
        assert!(is_idle_expiry(&err), "{err:?}");
        assert!(
            started.elapsed() < Duration::from_millis(900),
            "each byte restarted the timeout — the deadline is per read, not per line ({:?})",
            started.elapsed()
        );
        drop(reader);
        feeder.join().expect("feeder");
    }

    // 기한 안에 온 줄은 그대로 읽히고, 걷은 뒤에는 기한이 남지 않는다.
    #[test]
    fn a_line_inside_the_deadline_is_read_and_the_timeout_can_be_cleared() {
        let (mut client, mut reader) = pair();
        client.write_all(b"{\"a\":1}\n").expect("write");
        let mut line = String::new();
        DeadlineReader::new(&mut reader, Duration::from_secs(5))
            .read_line(&mut line)
            .expect("read");
        assert_eq!(line, "{\"a\":1}\n");
        assert!(clear_read_timeout(&reader));
        assert_eq!(reader.get_ref().read_timeout().expect("opt"), None);
    }
}
