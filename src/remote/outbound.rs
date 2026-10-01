//! App-owned outbound connection lifetime. No View/engine relationship is duplicated here.
use std::time::Instant;
use tasty_ssh::{Backoff,SshTunnel};
use super::client_session::AttachClientSession;
/// 실패한 재연결의 다음 시도 시각과 누적 횟수. 첫 시도는 슬롯 없이 즉시 수행한다.
pub(crate) struct ReconnectSlot {
    pub(crate) backoff: Backoff,
    pub(crate) next_attempt: Instant,
    pub(crate) attempts: u32,
    /// 슬롯을 지우면 첫 시도로 취급하므로 중단 상태를 별도로 보존한다.
    /// 사용자가 해당 워크스페이스로 돌아오면 재개할 수 있다.
    pub(crate) given_up: bool,
}

/// AppEvent에 넣을 수 없는 터널 핸들을 별도 결과 채널로 전달한다.
pub(crate) struct AutoAttachOutcome {
    pub(crate) attempt:AttemptToken,
    /// 자동 attach의 로컬 anchor ID. anchor 없는 수동 요청은 None이며 중복 방지 집합에 넣지 않는다.
    pub(crate) anchor_ws_id: Option<u32>,
    pub(crate) remote_ws: u32,
    pub(crate) result: anyhow::Result<(Option<SshTunnel>, u16)>,
    /// 신규 mirror 생성 대신 기존 세션 재연결과 실패 시 백오프 갱신을 수행할지 구별한다.
    pub(crate) is_reconnect: bool,
}

pub(crate) struct Remote {
    attempts:Vec<AttemptRecord>,
    pub(crate) sessions:Vec<AttachClientSession>,
    pub(crate) active:std::collections::HashSet<u32>,
    pub(crate) last_active_ws:Option<u32>,
    pub(crate) pending_reactivation:std::collections::HashSet<u32>,
    pub(crate) reconnect:std::collections::HashMap<u32,ReconnectSlot>,
    pub(crate) tx:std::sync::mpsc::Sender<AutoAttachOutcome>,
    pub(crate) rx:std::sync::mpsc::Receiver<AutoAttachOutcome>,
}
impl Remote {
    pub(crate) fn new()->Self {
        let (tx,rx)=std::sync::mpsc::channel();
        Self {attempts:Vec::new(),sessions:Vec::new(),active:Default::default(),last_active_ws:None,pending_reactivation:Default::default(),reconnect:Default::default(),tx,rx}
    }
}

#[derive(Clone)]
pub(crate) struct AttemptToken(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl AttemptToken {
    pub(crate) fn is_active(&self)->bool {self.0.load(std::sync::atomic::Ordering::Acquire)}
    fn cancel(&self) {self.0.store(false,std::sync::atomic::Ordering::Release);}
    fn same(&self,other:&Self)->bool {std::sync::Arc::ptr_eq(&self.0,&other.0)}
}
pub(crate) struct AttemptRecord {
    pub(crate) token:AttemptToken,
    pub(crate) anchor:Option<u32>,
    pub(crate) mapping:Option<crate::model::WorkspaceAttachMapping>,
}
impl Remote {
    pub(crate) fn begin_attempt(&mut self,anchor:Option<u32>,mapping:Option<crate::model::WorkspaceAttachMapping>)->Result<AttemptToken,&'static str> {
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
    pub(crate) fn finish_attempt(&mut self,token:&AttemptToken)->Option<AttemptRecord> {
        let index=self.attempts.iter().position(|attempt|attempt.token.same(token))?;
        let record=self.attempts.remove(index);
        let active=record.token.is_active();record.token.cancel();
        active.then_some(record)
    }
}
impl Drop for Remote {
    fn drop(&mut self) {for attempt in &self.attempts {attempt.token.cancel();}}
}
