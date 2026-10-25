use super::{RequestOwner, budget::Lease};
use crate::{
    connection::ConnectionEpoch,
    html_http_body::{self, BodyBudget, BodyError, VerifiedBody},
};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use tasty_ipc::html_http::{BodyIdentity, ResponseHead};

pub(super) struct ResultBody {
    pub value: Result<Arc<VerifiedBody>, BodyError>,
    pub owner: Weak<()>,
    pub _slot: Arc<Lease>,
}
pub(super) struct Work {
    pub request: Arc<RequestOwner>,
    pub head: ResponseHead,
    pub encoded: Arc<Vec<u8>>,
    pub identities: (BodyIdentity, BodyIdentity),
    pub budget: BodyBudget,
    pub decoded_limit: usize,
    pub _pending: Lease,
    pub _head: Lease,
}
/// Move into the caller's bounded worker pool. No runtime, thread or join is created here.
/// Dropping an unrun job resolves its mailbox as cancelled. Running jobs keep their provisional
/// buffers and worker slot until run returns, even after the document closes.
pub struct VerifyJob {
    pub(super) work: Option<Work>,
    pub(super) sender: Option<mpsc::SyncSender<ResultBody>>,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) epoch: ConnectionEpoch,
    pub(super) owner: Weak<()>,
    pub(super) slot: Option<Arc<Lease>>,
    pub(super) wake: Arc<dyn Fn() + Send + Sync>,
}
impl VerifyJob {
    /// Read-only correlation for retiring an unstarted job in the caller's bounded pool queue.
    /// This is the captured Begin identity, not authority supplied by a worker completion.
    pub fn request_id(&self) -> tasty_ipc::html_transfer::TransferId {
        self.work
            .as_ref()
            .expect("unconsumed verify job")
            .request
            .begin
            .id()
    }
    pub fn run(mut self) {
        let work = self.work.take().expect("unconsumed verify job");
        if !self.epoch.is_active() || self.owner.upgrade().is_none() {
            self.cancel.store(true, Ordering::Release);
        }
        let value = html_http_body::verify(
            &work.request.begin,
            work.head,
            work.encoded,
            work.identities,
            &work.budget,
            work.decoded_limit,
            &self.cancel,
        );
        self.post(value);
    }
    fn post(&mut self, value: Result<Arc<VerifiedBody>, BodyError>) {
        if let (Some(sender), Some(slot)) = (self.sender.take(), self.slot.as_ref()) {
            // The receiver may have retired. Dropping the returned result releases its body/slot.
            if sender
                .try_send(ResultBody {
                    value,
                    owner: self.owner.clone(),
                    _slot: slot.clone(),
                })
                .is_err()
            {
                tracing::debug!("retired HTTP verification result discarded");
            }
            (self.wake)();
        }
    }
}
impl Drop for VerifyJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.post(Err(BodyError::Cancelled));
    }
}
