//! Native futures and returned responses retain one slot until actual socket/body ownership ends.
use super::*;
use std::{
    future::Future,
    pin::Pin,
    sync::Weak,
    task::{Context, Poll, Waker},
};
struct State {
    pub id: Option<TransferId>,
    pub started: bool,
    result: Option<Result<Arc<VerifiedBody>, BridgeError>>,
    delivered: bool,
    waker: Option<Waker>,
}
pub(super) struct Slot {
    pub request: Arc<NativeRequest>,
    pub reuse: Option<ResponseId>,
    pub abandoned: AtomicBool,
    state: Mutex<State>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Slot {
    pub(super) fn new(
        request: Arc<NativeRequest>,
        reuse: Option<ResponseId>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            request,
            reuse,
            abandoned: AtomicBool::new(false),
            state: Mutex::new(State {
                id: None,
                started: false,
                result: None,
                delivered: false,
                waker: None,
            }),
            wake,
        }
    }
    fn lock(&self) -> Option<std::sync::MutexGuard<'_, State>> {
        match self.state.lock() {
            Ok(state) => Some(state),
            Err(_) => {
                tracing::error!("native HTTP waiter poisoned; publication refused");
                None
            }
        }
    }
    pub(super) fn assignment(&self) -> Option<(Option<TransferId>, bool)> {
        self.lock().map(|state| (state.id, state.started))
    }
    pub(super) fn assign(&self, id: Option<TransferId>) -> bool {
        let Some(mut state) = self.lock() else {
            return false;
        };
        state.id = id;
        state.started = true;
        true
    }
    pub(super) fn finish(&self, result: Result<Arc<VerifiedBody>, BridgeError>) {
        let wake = {
            let Some(mut state) = self.lock() else {
                tracing::error!("native HTTP waiter poisoned; publication refused");
                return;
            };
            if state.delivered || state.result.is_some() {
                return;
            }
            state.result = Some(result);
            state.waker.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}
struct Lease(Arc<Slot>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.abandoned.store(true, Ordering::Release);
        (self.0.wake)();
    }
}
pub struct NativeWait {
    bridge: Weak<HttpBridge>,
    lease: Option<Lease>,
}
pub struct NativeResponse {
    pub body: Arc<VerifiedBody>,
    pub request: Arc<NativeRequest>,
    _lease: Lease,
}
impl NativeWait {
    pub(super) fn new(bridge: &Arc<HttpBridge>, slot: Arc<Slot>) -> Self {
        Self {
            bridge: Arc::downgrade(bridge),
            lease: Some(Lease(slot)),
        }
    }
}
impl Future for NativeWait {
    type Output = Result<NativeResponse, BridgeError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Some(lease) = &this.lease else {
            return Poll::Ready(Err(BridgeError::Closed));
        };
        let Some(bridge) = this.bridge.upgrade().filter(|bridge| bridge.current()) else {
            return Poll::Ready(Err(BridgeError::Stale));
        };
        let slot = lease.0.clone();
        let waker = cx.waker().clone();
        let (result, old_waker, finished_waker) = {
            let Some(mut state) = slot.lock() else {
                return Poll::Ready(Err(BridgeError::Failed));
            };
            let old = state.waker.replace(waker);
            let result = state.result.take();
            let finished = if result.is_some() {
                state.delivered = true;
                state.waker.take()
            } else {
                None
            };
            (result, old, finished)
        };
        drop(old_waker);
        drop(finished_waker);
        let Some(result) = result else {
            return Poll::Pending;
        };
        if !bridge.current() {
            return Poll::Ready(Err(BridgeError::Stale));
        }
        Poll::Ready(result.map(|body| NativeResponse {
            body,
            request: slot.request.clone(),
            _lease: this.lease.take().expect("one native response lease"),
        }))
    }
}
