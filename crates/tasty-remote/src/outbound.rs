//! App-owned outbound connection lifetime. No View/engine relationship is duplicated here.
use std::time::Instant;
use tasty_ssh::{Backoff,SshTunnel};
use super::client_session::AttachClientSession;
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
    pub attempt:AttemptToken,
    /// 자동 attach의 로컬 anchor ID. anchor 없는 수동 요청은 None이며 중복 방지 집합에 넣지 않는다.
    pub anchor_ws_id: Option<u32>,
    pub remote_ws: u32,
    pub result: anyhow::Result<(Option<SshTunnel>, u16)>,
    /// 신규 mirror 생성 대신 기존 세션 재연결과 실패 시 백오프 갱신을 수행할지 구별한다.
    pub is_reconnect: bool,
}

pub struct Remote {
    retirements:Vec<crate::transport::RetirementReceipt>,
    attempt_threads:Vec<std::thread::JoinHandle<()>>,
    retirement_failed:bool,
    shutdown_started:Option<Instant>,
    attempts:Vec<AttemptRecord>,
    pub sessions:Vec<AttachClientSession>,
    pub active:std::collections::HashSet<u32>,
    pub last_active_ws:Option<u32>,
    pub pending_reactivation:std::collections::HashSet<u32>,
    pub reconnect:std::collections::HashMap<u32,ReconnectSlot>,
    pub tx:std::sync::mpsc::Sender<AutoAttachOutcome>,
    pub rx:std::sync::mpsc::Receiver<AutoAttachOutcome>,
}
impl Remote {
    pub fn new()->Self {
        let (tx,rx)=std::sync::mpsc::channel();
        Self {retirements:Vec::new(),attempt_threads:Vec::new(),retirement_failed:false,shutdown_started:None,attempts:Vec::new(),sessions:Vec::new(),active:Default::default(),last_active_ws:None,pending_reactivation:Default::default(),reconnect:Default::default(),tx,rx}
    }
}

#[derive(Clone)]
pub struct AttemptToken(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl AttemptToken {
    pub fn is_active(&self)->bool {self.0.load(std::sync::atomic::Ordering::Acquire)}
    fn cancel(&self) {self.0.store(false,std::sync::atomic::Ordering::Release);}
    fn same(&self,other:&Self)->bool {std::sync::Arc::ptr_eq(&self.0,&other.0)}
}
pub struct AttemptRecord {
    pub token:AttemptToken,
    pub anchor:Option<u32>,
    pub mapping:Option<tasty_model::WorkspaceAttachMapping>,
}
impl Remote {
    pub fn spawn_attempt(&mut self,work:impl FnOnce()+Send+'static) {
        self.reap_attempts();
        self.attempt_threads.push(std::thread::spawn(work));
    }
    pub fn reap_attempts(&mut self) {
        let mut running=Vec::new();
        for thread in std::mem::take(&mut self.attempt_threads) {
            if thread.is_finished() {if thread.join().is_err() {self.retirement_failed=true;tracing::error!("remote endpoint worker panicked");}}
            else {running.push(thread);}
        }
        self.attempt_threads=running;
    }
    pub fn track_workers(&mut self,receipt:crate::transport::RetirementReceipt) {
        self.retirement_failed|=self.retirements.iter().any(|receipt|receipt.failed());
        self.retirements.retain(|receipt|!receipt.is_done());
        self.retirements.push(receipt);
    }
    pub fn begin_shutdown(&mut self) {
        if self.shutdown_started.is_some() {return;}
        self.shutdown_started=Some(Instant::now());
        for attempt in &self.attempts {attempt.token.cancel();}
        self.sessions.clear();
        let threads=std::mem::take(&mut self.attempt_threads);
        if !threads.is_empty() {self.track_workers(crate::transport::join_attempt_workers(threads));}
        while let Ok(outcome)=self.rx.try_recv() {drop(outcome);}
    }
    /// Joining and deadline expiry are distinct observations; callers must not call timeout success.
    pub fn shutdown_observation(&self)->ShutdownObservation {
        while let Ok(outcome)=self.rx.try_recv() {drop(outcome);}
        if self.retirements.iter().all(|receipt|receipt.is_done()) {
            if self.retirement_failed || self.retirements.iter().any(|receipt|receipt.failed()) {ShutdownObservation::WorkerFailed} else {ShutdownObservation::Joined}
        } else if self.shutdown_started.is_some_and(|start|start.elapsed()>=std::time::Duration::from_secs(5)) {
            ShutdownObservation::TimedOut
        } else {ShutdownObservation::Waiting}
    }
    pub fn begin_attempt(&mut self,anchor:Option<u32>,mapping:Option<tasty_model::WorkspaceAttachMapping>)->Result<AttemptToken,&'static str> {
        if let Some(anchor)=anchor {
            self.attempts.retain(|attempt| {
                if attempt.anchor==Some(anchor) {attempt.token.cancel();false} else {true}
            });
        }
        if self.attempts.len()>=64 {return Err("remote connection attempt queue is full");}
        let token=AttemptToken(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)));
        self.attempts.push(AttemptRecord {token:token.clone(),anchor,mapping});
        Ok(token)
    }
    pub fn finish_attempt(&mut self,token:&AttemptToken)->Option<AttemptRecord> {
        let index=self.attempts.iter().position(|attempt|attempt.token.same(token))?;
        let record=self.attempts.remove(index);
        let active=record.token.is_active();record.token.cancel();
        active.then_some(record)
    }
}
impl Drop for Remote {
    fn drop(&mut self) {self.begin_shutdown();}
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ShutdownObservation {Waiting,Joined,WorkerFailed,TimedOut}

impl ReconnectSlot {
    pub fn new()->Self {Self {backoff:Backoff::new(),next_attempt:Instant::now(),attempts:0,given_up:false}}
}
impl Default for ReconnectSlot {fn default()->Self {Self::new()}}
