//! One owned native response completion. Dropping a waiter revokes its publication target.
use crate::html_body::NativeBodyResponse;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll, Waker},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceFailure {
    Denied,
    Cancelled,
    Declined,
    Stopped,
    Disconnected,
    Failed,
}
type Completion = Result<Arc<NativeBodyResponse>, ResourceFailure>;
#[derive(Default)]
struct State {
    value: Option<Completion>,
    waker: Option<Waker>,
}
struct Mailbox(Mutex<State>);
/// Owned only by the native task; producer ownership is weak, so cancellation is observable.
pub struct ResponseWait(Arc<Mailbox>);
pub struct ResponseTarget(Option<Weak<Mailbox>>);
impl ResponseWait {
    pub fn pair() -> (Self, ResponseTarget) {
        let mailbox = Arc::new(Mailbox(Mutex::new(State::default())));
        (
            Self(Arc::clone(&mailbox)),
            ResponseTarget(Some(Arc::downgrade(&mailbox))),
        )
    }
}
impl ResponseTarget {
    pub fn live(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|mailbox| mailbox.strong_count() > 0)
    }
    pub fn finish(mut self, value: Completion) {
        if let Some(target) = self.0.take() {
            complete(target, value);
        }
    }
}
impl Drop for ResponseTarget {
    fn drop(&mut self) {
        if let Some(target) = self.0.take() {
            complete(target, Err(ResourceFailure::Cancelled));
        }
    }
}
fn complete(target: Weak<Mailbox>, value: Completion) {
    let Some(mailbox) = target.upgrade() else {
        return;
    };
    let waker = match mailbox.0.lock() {
        Ok(mut state) => {
            state.value = Some(value);
            state.waker.take()
        }
        Err(poison) => {
            tracing::error!(%poison, "native resource completion poisoned; denying response");
            let mut state = poison.into_inner();
            state.value = Some(Err(ResourceFailure::Failed));
            state.waker.take()
        }
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}
impl Future for ResponseWait {
    type Output = Completion;
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.0.lock() {
            Ok(mut state) => match state.value.take() {
                Some(value) => Poll::Ready(value),
                None => {
                    state.waker = Some(context.waker().clone());
                    Poll::Pending
                }
            },
            Err(error) => {
                tracing::error!(%error, "native resource waiter poisoned; denying response");
                Poll::Ready(Err(ResourceFailure::Failed))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }
    #[test]
    fn completion_wakes_native_task_and_preserves_the_exact_allocation() {
        let (wait, target) = ResponseWait::pair();
        let mut wait = Box::pin(wait);
        let counter = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&counter));
        let mut context = Context::from_waker(&waker);
        assert!(wait.as_mut().poll(&mut context).is_pending());
        let id = tasty_ipc::html_transfer::TransferId {
            epoch: 1,
            surface_id: 2,
            binding: 3,
            document: 4,
            request: 5,
            transfer: 6,
        };
        let body = Arc::new(
            NativeBodyResponse::new(
                id,
                7,
                "text/css",
                "file:///remote/a.css",
                Arc::new(b"body{}".to_vec()),
            )
            .unwrap(),
        );
        target.finish(Ok(Arc::clone(&body)));
        assert_eq!(counter.0.load(Ordering::Acquire), 1);
        let Poll::Ready(Ok(response)) = wait.as_mut().poll(&mut context) else {
            panic!("not completed");
        };
        assert!(Arc::ptr_eq(&body, &response));
    }
    #[test]
    fn dropped_producer_cancels_and_dropped_native_task_revokes_target() {
        let (wait, target) = ResponseWait::pair();
        let mut wait = Box::pin(wait);
        let counter = Arc::new(WakeCount(AtomicUsize::new(0)));
        let waker = Waker::from(counter);
        let mut context = Context::from_waker(&waker);
        drop(target);
        assert!(matches!(
            wait.as_mut().poll(&mut context),
            Poll::Ready(Err(ResourceFailure::Cancelled))
        ));
        let (other, target) = ResponseWait::pair();
        assert!(target.live());
        drop(other);
        assert!(!target.live());
        target.finish(Err(ResourceFailure::Failed));
    }
}
