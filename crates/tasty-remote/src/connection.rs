//! A bounded queued frame belongs to exactly one connection, never a replaceable sender slot.
use super::client_session::OutFrame;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
// Only the observer value is replaced/cloned under this lock; callbacks run after unlocking.
static FAILURE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
pub struct ConnectionEpoch(Arc<AtomicBool>);
impl ConnectionEpoch {
    fn new() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
    pub fn is_active(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn retire(&self) {
        self.0.store(false, Ordering::Release);
    }
    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
pub struct QueuedFrame {
    pub epoch: ConnectionEpoch,
    pub frame: OutFrame,
    _bytes: QueuedBytes,
}
struct QueuedBytes {
    counter: Arc<AtomicUsize>,
    bytes: usize,
}
impl Drop for QueuedBytes {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
/// 연결 실패를 알릴 상대 — 끊김 플래그와 깨우기 콜백. `bind_failure` 가 채우고
/// `fail` 이 읽는다.
type FailureObserver = (Arc<AtomicBool>, Arc<dyn Fn() + Send + Sync>);
pub struct ConnectionSender {
    epoch: ConnectionEpoch,
    sender: mpsc::SyncSender<QueuedFrame>,
    bytes: Arc<AtomicUsize>,
    failed: AtomicBool,
    failure: std::sync::Mutex<Option<FailureObserver>>,
}
impl ConnectionSender {
    pub fn bind_failure(&self, disconnected: Arc<AtomicBool>, wake: Arc<dyn Fn() + Send + Sync>) {
        let mut observer = self.failure.lock().unwrap_or_else(|poison| {
            tasty_utils::poison::recover_poisoned(
                poison,
                "remote connection failure observer",
                &FAILURE_POISON_REPORTED,
            )
        });
        *observer = Some((disconnected.clone(), wake.clone()));
        drop(observer);
        if self.failed.load(Ordering::Acquire) {
            disconnected.store(true, Ordering::Release);
            wake();
        }
    }
    fn fail(&self, reason: &str) {
        self.epoch.retire();
        if !self.failed.swap(true, Ordering::AcqRel) {
            tracing::warn!(
                reason,
                "remote output rejected; connection retired and queued input will not replay"
            );
        }
        let observer = self
            .failure
            .lock()
            .unwrap_or_else(|poison| {
                tasty_utils::poison::recover_poisoned(
                    poison,
                    "remote connection failure observer",
                    &FAILURE_POISON_REPORTED,
                )
            })
            .clone();
        if let Some((disconnected, wake)) = observer {
            disconnected.store(true, Ordering::Release);
            wake();
        }
    }

    pub fn epoch(&self) -> ConnectionEpoch {
        self.epoch.clone()
    }
    pub fn retire(&self) {
        self.epoch.retire();
    }
    /// Failure means the frame was not accepted. Callers never retry old user input on a new epoch.
    pub fn send(&self, frame: OutFrame) -> Result<(), mpsc::SendError<OutFrame>> {
        if !self.epoch.is_active() {
            return Err(mpsc::SendError(frame));
        }
        let weight = frame
            .payload
            .capacity()
            .saturating_add(std::mem::size_of::<QueuedFrame>());
        if self
            .bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                held.checked_add(weight)
                    .filter(|next| *next <= tasty_ipc::admission::QUEUED_BYTES_LIMIT)
            })
            .is_err()
        {
            self.fail("outgoing byte capacity");
            return Err(mpsc::SendError(frame));
        }
        let queued = QueuedFrame {
            epoch: self.epoch.clone(),
            frame,
            _bytes: QueuedBytes {
                counter: self.bytes.clone(),
                bytes: weight,
            },
        };
        self.sender.try_send(queued).map_err(|error| {
            let queued = match error {
                mpsc::TrySendError::Full(queued) => {
                    self.fail("outgoing frame capacity");
                    queued
                }
                mpsc::TrySendError::Disconnected(queued) => {
                    self.fail("writer disconnected");
                    queued
                }
            };
            mpsc::SendError(queued.frame)
        })
    }
}
pub fn channel() -> (Arc<ConnectionSender>, mpsc::Receiver<QueuedFrame>) {
    let (sender, receiver) = mpsc::sync_channel(256);
    (
        Arc::new(ConnectionSender {
            epoch: ConnectionEpoch::new(),
            sender,
            bytes: Arc::new(AtomicUsize::new(0)),
            failed: AtomicBool::new(false),
            failure: std::sync::Mutex::new(None),
        }),
        receiver,
    )
}
