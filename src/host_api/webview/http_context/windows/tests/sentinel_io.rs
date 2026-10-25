//! Nonblocking socket progress shares an absolute stop deadline; partial reads cannot reset it.
use super::IO_WAIT;
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) const POLL_WAIT: Duration = Duration::from_millis(5);
pub(super) const MAX_CONNECTIONS: usize = 16;
pub(super) const MAX_BYTES: usize = 4096;
pub(super) struct Drain {
    pub(super) deadline: Mutex<Instant>,
    cleanup: AtomicBool,
    pub(super) connections: AtomicUsize,
    pub(super) bytes: AtomicUsize,
}
impl Drain {
    pub(super) fn new() -> Self {
        Self {
            deadline: Mutex::new(Instant::now() + Duration::from_secs(65)),
            cleanup: AtomicBool::new(false),
            connections: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
        }
    }
    pub(super) fn stop(&self, cleanup: bool) -> io::Result<()> {
        if cleanup {
            self.cleanup.store(true, Ordering::Release);
            return Ok(());
        }
        let mut deadline = self
            .deadline
            .lock()
            .map_err(|_| io::Error::other("sentinel deadline poisoned"))?;
        *deadline = (*deadline).min(Instant::now() + IO_WAIT);
        Ok(())
    }
    pub(super) fn cleanup(&self) -> bool {
        self.cleanup.load(Ordering::Acquire)
    }
    pub(super) fn check(&self, local: Option<Instant>) -> io::Result<()> {
        if self.cleanup() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "failure cleanup cancelled observation",
            ));
        }
        let deadline = *self
            .deadline
            .lock()
            .map_err(|_| io::Error::other("sentinel deadline poisoned"))?;
        if Instant::now() >= local.map_or(deadline, |d| d.min(deadline)) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "sentinel absolute deadline exhausted",
            ));
        }
        if self.connections.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            return Err(io::Error::other("sentinel connection budget exhausted"));
        }
        if self.bytes.load(Ordering::Relaxed) >= MAX_BYTES {
            return Err(io::Error::other("sentinel byte budget exhausted"));
        }
        Ok(())
    }
    pub(super) fn connection(&self) -> io::Result<()> {
        self.connections
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(1).filter(|n| *n <= MAX_CONNECTIONS)
            })
            .map_err(|_| io::Error::other("sentinel connection budget exhausted"))?;
        Ok(())
    }
    pub(super) fn unused_connection(&self) {
        self.connections.fetch_sub(1, Ordering::Relaxed);
    }
    fn remaining(&self, requested: usize) -> usize {
        requested.min(MAX_BYTES.saturating_sub(self.bytes.load(Ordering::Relaxed)))
    }
    fn charge(&self, actual: usize) {
        // Only the SOCKS observer reads/writes bytes; the target observer only accepts sockets.
        self.bytes.fetch_add(actual, Ordering::Relaxed);
    }
}
pub(super) fn read(
    socket: &mut TcpStream,
    bytes: &mut [u8],
    deadline: Instant,
    drain: &Drain,
) -> io::Result<usize> {
    loop {
        drain.check(Some(deadline))?;
        let count = drain.remaining(bytes.len());
        match socket.read(&mut bytes[..count]) {
            Ok(n) => {
                drain.charge(n);
                return Ok(n);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(POLL_WAIT)
            }
            Err(error) => return Err(error),
        }
    }
}
pub(super) fn read_exact(
    socket: &mut TcpStream,
    mut bytes: &mut [u8],
    deadline: Instant,
    drain: &Drain,
) -> io::Result<()> {
    while !bytes.is_empty() {
        let n = read(socket, bytes, deadline, drain)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete accepted greeting",
            ));
        }
        bytes = &mut bytes[n..];
    }
    Ok(())
}
pub(super) fn write_all(
    socket: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
    drain: &Drain,
) -> io::Result<()> {
    while !bytes.is_empty() {
        drain.check(Some(deadline))?;
        let count = drain.remaining(bytes.len());
        match socket.write(&bytes[..count]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "sentinel reply write zero",
                ));
            }
            Ok(n) => {
                drain.charge(n);
                bytes = &bytes[n..];
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                std::thread::sleep(POLL_WAIT)
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
