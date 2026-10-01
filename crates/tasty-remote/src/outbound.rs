//! App-owned outbound connection lifetime. No View/engine relationship is duplicated here.
use super::client_session::AttachClientSession;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tasty_ssh::{Backoff, SshTunnel};
/// 실패한 재연결의 다음 시도 시각과 누적 횟수. 첫 시도는 슬롯 없이 즉시 수행한다.
pub struct ReconnectSlot {
    pub backoff: Backoff,
    pub next_attempt: Instant,
    pub attempts: u32,
    /// 슬롯을 지우면 첫 시도로 취급하므로 중단 상태를 별도로 보존한다.
    /// 사용자가 해당 워크스페이스로 돌아오면 재개할 수 있다.
    pub given_up: bool,
}

/// AppEvent에 넣을 수 없는 터널 핸들을 별도 결과 채널로 전달한다.
pub struct AutoAttachOutcome {
    pub attempt: AttemptToken,
    /// 자동 attach의 로컬 anchor ID. anchor 없는 수동 요청은 None이며 중복 방지 집합에 넣지 않는다.
    pub anchor_ws_id: Option<u32>,
    pub remote_ws: u32,
    pub result: anyhow::Result<(Option<SshTunnel>, u16)>,
    /// 신규 mirror 생성 대신 기존 세션 재연결과 실패 시 백오프 갱신을 수행할지 구별한다.
    pub is_reconnect: bool,
}

pub struct Remote {
    pub(crate) browsers: crate::browser::Browsers,
    retirements: Vec<crate::transport::RetirementReceipt>,
    attempt_threads: Vec<std::thread::JoinHandle<()>>,
    retirement_failed: bool,
    pub(crate) shutdown_started: Option<Instant>,
    pub(crate) pending_connections: std::collections::HashMap<
        super::pending_connection::ConnectionTicket,
        super::pending_connection::PendingConnection,
    >,
    pub(crate) next_connection: u64,
    pub(crate) connection_tx:
        std::sync::mpsc::SyncSender<super::pending_connection::ConnectionOutcome>,
    pub(crate) connection_rx:
        std::sync::mpsc::Receiver<super::pending_connection::ConnectionOutcome>,
    attempts: Vec<AttemptRecord>,
    pub sessions: Vec<AttachClientSession>,
    pub active: std::collections::HashSet<u32>,
    pub last_active_ws: Option<u32>,
    pub pending_reactivation: std::collections::HashSet<u32>,
    pub reconnect: std::collections::HashMap<u32, ReconnectSlot>,
    pub tx: std::sync::mpsc::Sender<AutoAttachOutcome>,
    pub rx: std::sync::mpsc::Receiver<AutoAttachOutcome>,
}
impl Remote {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (connection_tx, connection_rx) = std::sync::mpsc::sync_channel(8);
        Self {
            browsers: Default::default(),
            pending_connections: Default::default(),
            next_connection: 1,
            connection_tx,
            connection_rx,
            retirements: Vec::new(),
            attempt_threads: Vec::new(),
            retirement_failed: false,
            shutdown_started: None,
            attempts: Vec::new(),
            sessions: Vec::new(),
            active: Default::default(),
            last_active_ws: None,
            pending_reactivation: Default::default(),
            reconnect: Default::default(),
            tx,
            rx,
        }
    }
}

#[derive(Clone)]
pub struct AttemptToken(std::sync::Arc<AttemptState>);
struct AttemptState {
    ssh: std::sync::Mutex<Vec<tasty_ssh::SshCancel>>,
    active: std::sync::atomic::AtomicBool,
    sockets: std::sync::Mutex<Vec<std::net::TcpStream>>,
}
impl PartialEq for AttemptToken {
    fn eq(&self, other: &Self) -> bool {
        self.same(other)
    }
}
impl Eq for AttemptToken {}
impl std::hash::Hash for AttemptToken {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&(std::sync::Arc::as_ptr(&self.0) as usize), state);
    }
}
/// Cancellation can race EOF or a previous shutdown of another clone of this socket.
/// Failure does not establish a join: callers still retain and poll the worker receipt.
fn shutdown_cancelled_socket(socket: &std::net::TcpStream) {
    match socket.shutdown(std::net::Shutdown::Both) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {
            tracing::debug!("remote cancellation socket already disconnected: {error}");
        }
        Err(error) => {
            tracing::warn!(
                "remote cancellation socket shutdown failed; waiting for worker exit: {error}"
            );
        }
    }
}

// These lists own cancellation controls, not a stream writer or protocol frame state.
static ATTEMPT_SSH_POISON_REPORTED: AtomicBool = AtomicBool::new(false);
static ATTEMPT_SOCKETS_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

impl AttemptToken {
    pub fn is_active(&self) -> bool {
        self.0.active.load(std::sync::atomic::Ordering::Acquire)
    }
    pub(crate) fn cancel(&self) {
        self.0
            .active
            .store(false, std::sync::atomic::Ordering::Release);
        {
            let mut ssh = tasty_utils::poison::recover_mutex(
                self.0.ssh.lock(),
                "remote attempt SSH cancellation controls",
                &ATTEMPT_SSH_POISON_REPORTED,
            );
            for cancel in ssh.drain(..) {
                cancel.request_cancel();
            }
        }
        {
            let mut sockets = tasty_utils::poison::recover_mutex(
                self.0.sockets.lock(),
                "remote attempt socket cancellation controls",
                &ATTEMPT_SOCKETS_POISON_REPORTED,
            );
            for socket in sockets.drain(..) {
                shutdown_cancelled_socket(&socket);
            }
        }
    }
    pub fn register_ssh(&self, cancel: tasty_ssh::SshCancel) -> Result<(), String> {
        let mut handles = self
            .0
            .ssh
            .lock()
            .map_err(|_| "SSH cancellation state poisoned".to_owned())?;
        if !self.is_active() {
            cancel.request_cancel();
            return Err("remote attempt retired".into());
        }
        handles.push(cancel);
        Ok(())
    }
    fn complete(&self) {
        self.0
            .active
            .store(false, std::sync::atomic::Ordering::Release);
        let mut sockets = tasty_utils::poison::recover_mutex(
            self.0.sockets.lock(),
            "remote attempt socket cancellation controls",
            &ATTEMPT_SOCKETS_POISON_REPORTED,
        );
        sockets.clear();
    }
    /// Register the exact connecting socket before the first blocking handshake read/write.
    pub fn register_socket(&self, socket: &std::net::TcpStream) -> std::io::Result<()> {
        let control = socket.try_clone()?;
        let mut sockets = self
            .0
            .sockets
            .lock()
            .map_err(|_| std::io::Error::other("connection cancellation state poisoned"))?;
        if !self.is_active() {
            shutdown_cancelled_socket(&control);
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "connection attempt retired",
            ));
        }
        sockets.push(control);
        Ok(())
    }
    fn same(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}
pub struct AttemptRecord {
    pub token: AttemptToken,
    pub anchor: Option<u32>,
    pub mapping: Option<tasty_model::WorkspaceAttachMapping>,
}
impl Remote {
    pub fn spawn_attempt(
        &mut self,
        token: AttemptToken,
        work: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        self.reap_attempts();
        let failure = if self.shutdown_started.is_some() {
            Some("remote is shutting down")
        } else if !token.is_active() {
            Some("remote attempt is retired")
        } else if self.attempt_threads.len() >= 64 {
            Some("remote endpoint worker capacity exhausted")
        } else {
            None
        };
        if let Some(reason) = failure {
            token.cancel();
            self.attempts.retain(|attempt| !attempt.token.same(&token));
            return Err(reason.into());
        }
        match std::thread::Builder::new()
            .name("remote-endpoint".into())
            .spawn(work)
        {
            Ok(thread) => {
                self.attempt_threads.push(thread);
                Ok(())
            }
            Err(error) => {
                token.cancel();
                self.attempts.retain(|attempt| !attempt.token.same(&token));
                Err(error.to_string())
            }
        }
    }
    pub fn reap_attempts(&mut self) {
        let mut running = Vec::new();
        for thread in std::mem::take(&mut self.attempt_threads) {
            if thread.is_finished() {
                if thread.join().is_err() {
                    self.retirement_failed = true;
                    tracing::error!("remote endpoint worker panicked");
                }
            } else {
                running.push(thread);
            }
        }
        self.attempt_threads = running;
    }
    pub fn track_workers(&mut self, receipt: crate::transport::RetirementReceipt) {
        self.retirement_failed |= self.retirements.iter().any(|receipt| receipt.failed());
        self.retirements.retain(|receipt| !receipt.is_done());
        self.retirements.push(receipt);
    }
    pub fn retire_tunnel(&mut self, tunnel: Option<SshTunnel>) {
        if let Some(tunnel) = tunnel {
            self.track_workers(crate::transport::retire_tunnel(tunnel));
        }
    }
    pub fn discard_endpoint_outcome(&mut self, outcome: AutoAttachOutcome) {
        if let Ok((tunnel, _)) = outcome.result {
            self.retire_tunnel(tunnel);
        }
    }
    pub fn begin_shutdown(&mut self) {
        if self.shutdown_started.is_some() {
            return;
        }
        self.shutdown_started = Some(Instant::now());
        for attempt in &self.attempts {
            attempt.token.cancel();
        }
        self.shutdown_browsers();
        self.sessions.clear();
        self.pending_connections.clear();
        self.collect_connections();
        let threads = std::mem::take(&mut self.attempt_threads);
        if !threads.is_empty() {
            self.track_workers(crate::transport::join_attempt_workers(threads));
        }
        while let Ok(outcome) = self.rx.try_recv() {
            self.discard_endpoint_outcome(outcome);
        }
    }
    /// Joining and deadline expiry are distinct observations; callers must not call timeout success.
    pub fn shutdown_observation(&mut self) -> ShutdownObservation {
        self.collect_connections();
        self.shutdown_browsers();
        while let Ok(outcome) = self.rx.try_recv() {
            self.discard_endpoint_outcome(outcome);
        }
        if self.retirements.iter().all(|receipt| receipt.is_done()) {
            // Acquire of every joined receipt follows the worker's final send. Drain once more
            // after that observation so Joined also releases the final owned SSH tunnel outcome.
            while let Ok(outcome) = self.rx.try_recv() {
                self.discard_endpoint_outcome(outcome);
            }
            self.collect_connections();
            self.shutdown_browsers();
            if self.retirements.iter().any(|receipt| !receipt.is_done()) {
                return ShutdownObservation::Waiting;
            }
            if self.retirement_failed || self.retirements.iter().any(|receipt| receipt.failed()) {
                ShutdownObservation::WorkerFailed
            } else {
                ShutdownObservation::Joined
            }
        } else if self
            .shutdown_started
            .is_some_and(|start| start.elapsed() >= std::time::Duration::from_secs(5))
        {
            ShutdownObservation::TimedOut
        } else {
            ShutdownObservation::Waiting
        }
    }
    pub fn begin_attempt(
        &mut self,
        anchor: Option<u32>,
        mapping: Option<tasty_model::WorkspaceAttachMapping>,
    ) -> Result<AttemptToken, &'static str> {
        if self.shutdown_started.is_some() {
            return Err("remote is shutting down");
        }
        self.retirement_failed |= self.retirements.iter().any(|receipt| receipt.failed());
        self.retirements.retain(|receipt| !receipt.is_done());
        if self.retirements.len() >= 128 {
            return Err("remote resource retirement capacity exhausted");
        }
        if let Some(anchor) = anchor {
            self.attempts.retain(|attempt| {
                if attempt.anchor == Some(anchor) {
                    attempt.token.cancel();
                    false
                } else {
                    true
                }
            });
        }
        if self.attempts.len() >= 64 {
            return Err("remote connection attempt queue is full");
        }
        let token = AttemptToken(std::sync::Arc::new(AttemptState {
            ssh: std::sync::Mutex::new(Vec::new()),
            active: std::sync::atomic::AtomicBool::new(true),
            sockets: std::sync::Mutex::new(Vec::new()),
        }));
        self.attempts.push(AttemptRecord {
            token: token.clone(),
            anchor,
            mapping,
        });
        Ok(token)
    }
    pub fn cancel_attempt(&mut self, token: &AttemptToken) {
        token.cancel();
        self.attempts.retain(|attempt| !attempt.token.same(token));
    }
    pub fn finish_attempt(&mut self, token: &AttemptToken) -> Option<AttemptRecord> {
        let index = self
            .attempts
            .iter()
            .position(|attempt| attempt.token.same(token))?;
        let record = self.attempts.remove(index);
        let active = record.token.is_active();
        record.token.complete();
        active.then_some(record)
    }
}
impl Drop for Remote {
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownObservation {
    Waiting,
    Joined,
    WorkerFailed,
    TimedOut,
}

impl ReconnectSlot {
    pub fn new() -> Self {
        Self {
            backoff: Backoff::new(),
            next_attempt: Instant::now(),
            attempts: 0,
            given_up: false,
        }
    }
}
impl Default for ReconnectSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// A full result queue retains its owned value only while the original attempt is live.
/// Cancellation releases a blocked producer so shutdown can observe its actual join.
pub(crate) fn send_attempt_result<T>(
    sender: &std::sync::mpsc::SyncSender<T>,
    token: &AttemptToken,
    mut value: T,
) {
    loop {
        if !token.is_active() {
            return;
        }
        match sender.try_send(value) {
            Ok(()) => return,
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return,
            Err(std::sync::mpsc::TrySendError::Full(pending)) => value = pending,
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
