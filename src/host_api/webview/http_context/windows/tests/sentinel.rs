//! Joined, bounded observer ownership for the native diagnostic; no native API substitutes.
#[cfg(test)]
#[path = "sentinel_tests.rs"]
mod tests;
use std::{
    fmt,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

type Record = fn(fmt::Arguments<'_>);
const IO_WAIT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Observed {
    greetings: Vec<(u8, Vec<u8>)>,
    direct: bool,
    overflow: bool,
}
impl Observed {
    fn greeting(&mut self, version: u8, methods: Vec<u8>) {
        if self.greetings.len() < 8 {
            self.greetings.push((version, methods));
        } else {
            self.overflow = true;
        }
    }
    fn verify(&self) -> Result<(), String> {
        if self.direct || self.overflow || self.greetings.is_empty() {
            return Err("missing bounded proxy observation or direct target connection".into());
        }
        if self
            .greetings
            .iter()
            .any(|(v, methods)| *v != 5 || methods != &[0])
        {
            return Err("native SOCKS method set changed; re-evaluate compatibility".into());
        }
        Ok(())
    }
}

pub(super) struct Peers {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<Result<(), String>>>,
    observed: Arc<Mutex<Observed>>,
    record: Record,
}
impl Peers {
    pub(super) fn start(
        socks: TcpListener,
        target: TcpListener,
        record: Record,
    ) -> Result<Self, String> {
        socks.set_nonblocking(true).map_err(|e| e.to_string())?;
        target.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut peers = Self {
            stop: Arc::new(AtomicBool::new(false)),
            threads: Vec::new(),
            observed: Arc::new(Mutex::new(Observed::default())),
            record,
        };
        for (listener, is_socks) in [(socks, true), (target, false)] {
            let stop = peers.stop.clone();
            let observed = peers.observed.clone();
            peers.threads.push(std::thread::spawn(move || {
                observe(listener, is_socks, stop, observed, record)
            }));
        }
        Ok(peers)
    }
    fn stop_and_join(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        let mut errors = Vec::new();
        // Never return early: a failed observer cannot leave its sibling running.
        for thread in self.threads.drain(..) {
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => errors.push(error),
                Err(_) => errors.push("sentinel observer panicked".into()),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
    /// Consume the owner, so no successful receipt can precede observer termination.
    pub(super) fn finish(mut self) -> Result<(), String> {
        self.stop_and_join()?;
        self.observed
            .lock()
            .map_err(|_| "sentinel poisoned")?
            .verify()?;
        (self.record)(format_args!("CONFIRMED_NO_SOCKS_AUTH_AND_NO_DIRECT_TARGET"));
        Ok(())
    }
}
impl Drop for Peers {
    fn drop(&mut self) {
        if let Err(error) = self.stop_and_join() {
            (self.record)(format_args!("SENTINEL_CLEANUP_FAILED {error}"));
        }
    }
}

fn observe(
    listener: TcpListener,
    socks: bool,
    stop: Arc<AtomicBool>,
    observed: Arc<Mutex<Observed>>,
    record: Record,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(65);
    loop {
        // Accept once even after stop, so an already queued direct connection is recorded.
        match listener.accept() {
            Ok((mut socket, _)) => {
                if socks {
                    greeting(&mut socket, &stop, &observed, record)?;
                } else {
                    observed.lock().map_err(|_| "sentinel poisoned")?.direct = true;
                    record(format_args!("DIRECT_TARGET_CONNECTION"));
                }
                if stop.load(Ordering::Acquire) {
                    return Ok(());
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    record(format_args!("EXPECTED_IDLE_LISTENER_SHUTDOWN"));
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            // An interrupted nonblocking accept is retried within the same finite deadline.
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("sentinel accept failed: {error}")),
        }
        if Instant::now() >= deadline {
            return Err("sentinel observer deadline elapsed".into());
        }
    }
}

fn greeting(
    socket: &mut TcpStream,
    stop: &AtomicBool,
    observed: &Mutex<Observed>,
    record: Record,
) -> Result<(), String> {
    socket
        .set_nonblocking(false)
        .map_err(|e| format!("sentinel blocking socket: {e}"))?;
    socket
        .set_read_timeout(Some(IO_WAIT))
        .map_err(|e| format!("sentinel read timeout: {e}"))?;
    socket
        .set_write_timeout(Some(IO_WAIT))
        .map_err(|e| format!("sentinel write timeout: {e}"))?;
    let mut head = [0; 2];
    if let Err(error) = socket.read_exact(&mut head) {
        return shutdown_or_error(error, stop, record, "SOCKS header");
    }
    let mut methods = vec![0; head[1] as usize];
    if let Err(error) = socket.read_exact(&mut methods) {
        return shutdown_or_error(error, stop, record, "SOCKS methods");
    }
    record(format_args!(
        "SOCKS_GREETING version={} methods={methods:?}",
        head[0]
    ));
    observed
        .lock()
        .map_err(|_| "sentinel poisoned")?
        .greeting(head[0], methods);
    if let Err(error) = socket.write_all(&[5, 2]) {
        return shutdown_or_error(error, stop, record, "SOCKS rejection");
    }
    let mut auth = [0; 2];
    match socket.read(&mut auth) {
        // Selecting a method absent from the greeting must make the client close/refuse.
        Ok(0) => {
            record(format_args!("EXPECTED_AUTH_REJECTION_CLOSED"));
            Ok(())
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
            ) =>
        {
            record(format_args!(
                "EXPECTED_AUTH_REJECTION_RESET {:?}",
                error.kind()
            ));
            Ok(())
        }
        Ok(_) => Err("native replied to unsupported SOCKS authentication selection".into()),
        Err(error) => shutdown_or_error(error, stop, record, "SOCKS auth refusal"),
    }
}

fn shutdown_or_error(
    error: io::Error,
    stop: &AtomicBool,
    record: Record,
    stage: &str,
) -> Result<(), String> {
    if stop.load(Ordering::Acquire)
        && matches!(
            error.kind(),
            io::ErrorKind::UnexpectedEof
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::TimedOut
                | io::ErrorKind::WouldBlock
        )
    {
        record(format_args!(
            "EXPECTED_OBSERVER_SHUTDOWN {stage}: {:?}",
            error.kind()
        ));
        Ok(())
    } else {
        Err(format!("sentinel {stage} failed: {error}"))
    }
}
