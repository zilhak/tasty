//! Joined, bounded observer ownership for the native diagnostic; no native API substitutes.
#[path = "sentinel_io.rs"]
mod sentinel_io;
#[cfg(test)]
#[path = "sentinel_stop_tests.rs"]
mod stop_tests;
#[cfg(test)]
#[path = "sentinel_tests.rs"]
mod tests;
use sentinel_io::{Drain, POLL_WAIT, read, read_exact, write_all};
use std::{
    fmt, io,
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
    drain: Arc<Drain>,
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
            drain: Arc::new(Drain::new()),
        };
        for (listener, is_socks) in [(socks, true), (target, false)] {
            let stop = peers.stop.clone();
            let observed = peers.observed.clone();
            let drain = peers.drain.clone();
            peers.threads.push(std::thread::spawn(move || {
                observe(listener, is_socks, stop, observed, record, drain)
            }));
        }
        Ok(peers)
    }
    fn stop_and_join(&mut self, cleanup: bool) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Err(error) = self.drain.stop(cleanup) {
            errors.push(error.to_string());
        }
        self.stop.store(true, Ordering::Release);
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
        self.stop_and_join(false)?;
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
        if let Err(error) = self.stop_and_join(true) {
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
    drain: Arc<Drain>,
) -> Result<(), String> {
    loop {
        if drain.cleanup() {
            record(format_args!("EXPECTED_FAILURE_CLEANUP_STOP"));
            return Ok(());
        }
        drain.check(None).map_err(|e| e.to_string())?;
        // Reserve before accept: two observers cannot exceed the shared accept bound.
        drain.connection().map_err(|e| e.to_string())?;
        let accepted = listener.accept();
        if accepted.is_err() {
            drain.unused_connection();
        }
        match accepted {
            Ok((mut socket, _)) => {
                if socks {
                    if let Err(error) = greeting(&mut socket, &drain, &observed, record) {
                        if drain.cleanup() {
                            record(format_args!("EXPECTED_FAILURE_CLEANUP_IO {error}"));
                            return Ok(());
                        }
                        return Err(error);
                    }
                } else {
                    observed.lock().map_err(|_| "sentinel poisoned")?.direct = true;
                    record(format_args!("DIRECT_TARGET_CONNECTION"));
                }
                // Successful finalization drains until WouldBlock, never just one socket.
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    record(format_args!("FINAL_DRAIN_QUEUE_EMPTY"));
                    return Ok(());
                }
                std::thread::sleep(POLL_WAIT);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("sentinel accept failed: {error}")),
        }
    }
}

fn greeting(
    socket: &mut TcpStream,
    drain: &Drain,
    observed: &Mutex<Observed>,
    record: Record,
) -> Result<(), String> {
    let deadline = Instant::now() + IO_WAIT;
    socket
        .set_nonblocking(true)
        .map_err(|e| format!("sentinel nonblocking socket: {e}"))?;
    let mut head = [0; 2];
    read_exact(socket, &mut head, deadline, drain)
        .map_err(|e| format!("sentinel SOCKS header: {e}"))?;
    let mut methods = vec![0; head[1] as usize];
    read_exact(socket, &mut methods, deadline, drain)
        .map_err(|e| format!("sentinel SOCKS methods: {e}"))?;
    record(format_args!(
        "SOCKS_GREETING version={} methods={methods:?}",
        head[0]
    ));
    let valid = head[0] == 5 && methods == [0];
    observed
        .lock()
        .map_err(|_| "sentinel poisoned")?
        .greeting(head[0], methods);
    write_all(socket, &[5, 2], deadline, drain)
        .map_err(|e| format!("sentinel SOCKS rejection: {e}"))?;
    let mut auth = [0; 2];
    match read(socket, &mut auth, deadline, drain) {
        // Only a complete valid greeting can establish expected unsupported-method refusal.
        Ok(0) if valid => {
            record(format_args!("EXPECTED_AUTH_REJECTION_CLOSED"));
            Ok(())
        }
        Ok(0) => Err("native SOCKS method set changed; re-evaluate compatibility".into()),
        Err(error)
            if valid
                && matches!(
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
        Err(error) => Err(format!("sentinel SOCKS auth refusal: {error}")),
    }
}
