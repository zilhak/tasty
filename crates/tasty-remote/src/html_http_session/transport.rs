//! Model transitions and ordered enqueue happen under one captured document access.
use super::*;
use crate::{client_session::OutFrame, connection::ConnectionSender};
use tasty_ipc::html_http::{HttpRequest, ResponseId};
/// Client byte-consent/lifecycle actions; none carry JavaScript permissions or a new Begin.
#[derive(Clone, Copy, Debug)]
pub enum HttpAction {
    Allow(TransferId),
    Continue,
    Decline(TransferId),
    Stop,
    Reuse(ResponseId),
    Release(ResponseId),
}
fn enqueue(
    model: &mut ClientDocument,
    sender: &ConnectionSender,
    request: HttpRequest,
    current: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<(), ClientError> {
    let frame = match request.frame() {
        Ok(frame) => frame,
        Err(error) => {
            tracing::error!(%error, "HTTP command encoding failed; document retired");
            model.send_failed();
            return Err(ClientError::Invalid);
        }
    };
    if sender
        .send_guarded(
            OutFrame {
                tag: frame.tag,
                payload: frame.payload,
            },
            current,
        )
        .is_err()
    {
        model.send_failed();
        return Err(ClientError::Closed);
    }
    Ok(())
}
impl CapturedDocument {
    fn publication_guard(self: &Arc<Self>) -> Arc<dyn Fn() -> bool + Send + Sync> {
        let owner = Arc::downgrade(self);
        Arc::new(move || owner.upgrade().is_some_and(|owner| owner.is_current()))
    }
    /// Enqueue the admitted Begin immediately, before the caller allocates another shared ID.
    /// This method never retries a command after model advancement or send failure.
    pub fn admit_and_send(
        self: &Arc<Self>,
        sender: &ConnectionSender,
        begin: HttpRequest,
        upload: Arc<Vec<u8>>,
    ) -> Result<(), ClientError> {
        if !self.connection.same(&sender.epoch()) {
            return Err(ClientError::Stale);
        }
        let current = self.publication_guard();
        self.with_current(|model| {
            let begin = model.admit(begin, upload)?;
            enqueue(model, sender, begin, current)
        })
        .unwrap_or_else(|| {
            Err(if sender.epoch().is_active() {
                ClientError::Stale
            } else {
                ClientError::Closed
            })
        })
    }
    /// Mint at most one exact Close before retirement. Only this terminal cleanup may outlive
    /// the native document: the server still checks its full identity/source and connection.
    pub fn close_and_send(self: &Arc<Self>, sender: &ConnectionSender) -> Result<(), ClientError> {
        if !self.connection.same(&sender.epoch()) {
            return Err(ClientError::Stale);
        }
        let connection = self.connection.clone();
        let current = Arc::new(move || connection.is_active());
        let result = self
            .with_current(|model| {
                let Some(command) = model.close() else {
                    return Ok(());
                };
                if !matches!(command, HttpRequest::Close { .. }) {
                    return Err(ClientError::Invalid);
                }
                enqueue(model, sender, command, current)
            })
            .unwrap_or(Err(ClientError::Stale));
        self.retire();
        result
    }
    /// Apply one model-authorized action and enqueue its resulting command under the same fence.
    pub fn apply(
        self: &Arc<Self>,
        sender: &ConnectionSender,
        action: HttpAction,
    ) -> Result<bool, ClientError> {
        if !self.connection.same(&sender.epoch()) {
            return Err(ClientError::Stale);
        }
        let current = self.publication_guard();
        self.with_current(|model| {
            let command = match action {
                HttpAction::Allow(id) => return Ok(model.allow(id)),
                HttpAction::Continue => return Ok(model.continue_receiving()),
                HttpAction::Decline(id) => model.decline(id),
                HttpAction::Stop => model.stop(),
                HttpAction::Reuse(response) => model.reuse(response),
                HttpAction::Release(response) => model.release(response),
            };
            let Some(command) = command else {
                return Ok(false);
            };
            enqueue(model, sender, command, current)?;
            Ok(true)
        })
        .unwrap_or_else(|| {
            Err(if sender.epoch().is_active() {
                ClientError::Stale
            } else {
                ClientError::Closed
            })
        })
    }
    /// Bound work per engine pass; a full batch wakes the owner for the next pass.
    pub fn pump_commands(self: &Arc<Self>, sender: &ConnectionSender) -> bool {
        if !self.connection.same(&sender.epoch()) {
            return false;
        }
        let current = self.publication_guard();
        let mut full = false;
        let changed = self
            .with_current(|model| {
                let mut changed = model.poll_verification();
                for _ in 0..32 {
                    let Some(command) = model.next_command() else {
                        return changed;
                    };
                    changed = true;
                    if enqueue(model, sender, command, current.clone()).is_err() {
                        return changed;
                    }
                }
                full = true;
                changed
            })
            .unwrap_or(false);
        if full {
            (self.wake)();
        }
        changed
    }
}
impl HttpDocuments {
    pub fn close_all(&mut self, sender: &ConnectionSender) {
        self.retain_with_sender(sender, |_| false);
    }
    pub fn retain_with_sender(
        &mut self,
        sender: &ConnectionSender,
        mut keep: impl FnMut(u32) -> bool,
    ) {
        self.0.retain(|remote, document| {
            if keep(*remote) && document.is_current() {
                return true;
            }
            if let Err(error) = document.close_and_send(sender) {
                tracing::debug!(?error, "retired HTTP document cleanup not enqueued");
            }
            document.retire();
            false
        });
    }
    pub fn pump_commands(&self, sender: &ConnectionSender) -> bool {
        let mut changed = false;
        for document in self.0.values() {
            changed |= document.pump_commands(sender);
        }
        changed
    }
}
