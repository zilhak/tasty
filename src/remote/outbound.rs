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
    /// 자동 attach의 로컬 anchor ID. anchor 없는 수동 요청은 None이며 중복 방지 집합에 넣지 않는다.
    pub(crate) anchor_ws_id: Option<u32>,
    pub(crate) remote_ws: u32,
    pub(crate) result: anyhow::Result<(Option<SshTunnel>, u16)>,
    /// 신규 mirror 생성 대신 기존 세션 재연결과 실패 시 백오프 갱신을 수행할지 구별한다.
    pub(crate) is_reconnect: bool,
}

pub(crate) struct Remote {
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
        Self {sessions:Vec::new(),active:Default::default(),last_active_ws:None,pending_reactivation:Default::default(),reconnect:Default::default(),tx,rx}
    }
}
