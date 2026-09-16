//! Absolute I/O budgets checked below WebSocket fragmentation and TLS buffering.
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct Deadline(Arc<Mutex<Instant>>);
impl Deadline {
    pub fn new(at: Instant) -> Self {
        Self(Arc::new(Mutex::new(at)))
    }
    pub fn reset(&self, at: Instant) -> io::Result<()> {
        *self
            .0
            .lock()
            .map_err(|_| io::Error::other("deadline lock poisoned"))? = at;
        Ok(())
    }
    fn remaining(&self) -> io::Result<Duration> {
        let at = *self
            .0
            .lock()
            .map_err(|_| io::Error::other("deadline lock poisoned"))?;
        let remaining = at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "app_server_io_deadline_exceeded",
            ))
        } else {
            Ok(remaining)
        }
    }
}
pub(super) trait SocketTimeouts {
    fn read_timeout(&self, timeout: Duration) -> io::Result<()>;
    fn write_timeout(&self, timeout: Duration) -> io::Result<()>;
}
impl SocketTimeouts for std::net::TcpStream {
    fn read_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(timeout))
    }
    fn write_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.set_write_timeout(Some(timeout))
    }
}
#[cfg(unix)]
impl SocketTimeouts for std::os::unix::net::UnixStream {
    fn read_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(timeout))
    }
    fn write_timeout(&self, timeout: Duration) -> io::Result<()> {
        self.set_write_timeout(Some(timeout))
    }
}
pub(super) struct BudgetStream<S> {
    pub socket: S,
    pub deadline: Deadline,
}
impl<S: Read + SocketTimeouts> Read for BudgetStream<S> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.socket.read_timeout(self.deadline.remaining()?)?;
        self.socket.read(bytes)
    }
}
impl<S: Write + SocketTimeouts> Write for BudgetStream<S> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.write_timeout(self.deadline.remaining()?)?;
        self.socket.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.write_timeout(self.deadline.remaining()?)?;
        self.socket.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_drip_bounded<S: Read + Write + SocketTimeouts, W: Write + Send + 'static>(
        socket: S,
        mut writer: W,
    ) {
        let thread = std::thread::spawn(move || {
            for _ in 0..100 {
                if writer.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let start = Instant::now();
        let mut stream = BudgetStream {
            socket,
            deadline: Deadline::new(start + Duration::from_millis(100)),
        };
        let error = stream.read_exact(&mut [0; 100]).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
        assert!(
            start.elapsed() < Duration::from_millis(700),
            "raw drip extended the absolute budget"
        );
        drop(stream);
        thread.join().unwrap();
    }
    #[test]
    fn tcp_partial_reads_share_one_absolute_budget() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let writer = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        assert_drip_bounded(listener.accept().unwrap().0, writer);
    }
    #[cfg(unix)]
    #[test]
    fn unix_partial_reads_share_one_absolute_budget() {
        let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
        assert_drip_bounded(reader, writer);
    }
}
