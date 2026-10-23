//! File continuations share caller lifetime and idempotency with the other deferred replies.
use super::{Deferred, Wait, placeholder};
use crate::app::services::AppServices;
use serde_json::Value;
use tasty_ipc::protocol::JsonRpcResponse;

pub(super) trait FileWait: Send {
    fn try_ready(&mut self) -> bool;
    fn finish(self: Box<Self>, core: &AppServices) -> JsonRpcResponse;
}
struct Completion<T, P, F> {
    poll: P,
    finish: F,
    result: Option<T>,
}
impl<T, P, F> FileWait for Completion<T, P, F>
where
    T: Send,
    P: FnMut() -> Option<T> + Send,
    F: FnOnce(&AppServices, T) -> JsonRpcResponse + Send,
{
    fn try_ready(&mut self) -> bool {
        if self.result.is_none() {
            self.result = (self.poll)();
        }
        self.result.is_some()
    }
    fn finish(self: Box<Self>, core: &AppServices) -> JsonRpcResponse {
        (self.finish)(
            core,
            self.result.expect("file result checked before completion"),
        )
    }
}
impl AppServices {
    pub(crate) fn reply_with_file<T: Send + 'static>(
        &self,
        id: Value,
        mut poll: impl FnMut() -> Option<T> + Send + 'static,
        finish: impl FnOnce(&AppServices, T) -> JsonRpcResponse + Send + 'static,
    ) -> JsonRpcResponse {
        if let Some(result) = poll() {
            return finish(self, result);
        }
        self.memory_replies().park(Deferred {
            id: id.clone(),
            wait: Wait::File(Box::new(Completion {
                poll,
                finish,
                result: None,
            })),
            idempotency: None,
        });
        placeholder(id)
    }
}
