//! One completed native request authorizes one fresh document, never a page-supplied URL.
use super::*;
use crate::{client_session::OutFrame, connection::ConnectionEpoch};
use std::sync::Weak;
use tasty_ipc::html_http::{HttpRequest, ResponseId};

/// Not Clone/Deserialize: minted from a live delivered response before native/session retirement.
/// The host supplies the retained native view allocation and a source/grant predicate independent
/// of document generations. Neither may be synthesized from page messages.
pub struct ReloadAuthority {
    pub(super) previous: ResponseId,
    pub(super) request: Arc<NativeRequest>,
    scope: BridgeScope,
    native: Weak<()>,
    authority: Current,
    sender: Arc<ConnectionSender>,
    epoch: ConnectionEpoch,
    entry: TransferId,
    armed: AtomicBool,
    detached: bool,
    original: Weak<HttpBridge>,
    reserved: Arc<AtomicBool>,
}
impl ReloadAuthority {
    /// Invoke at the user Reload boundary, before normal old-session close or native clear.
    /// Merely capturing the receipt during response delivery does not interrupt that delivery.
    pub fn retire(mut self) -> Result<Self, BridgeError> {
        let bridge = self.original.upgrade().ok_or(BridgeError::Stale)?;
        if !self.valid() || !bridge.current() {
            return Err(BridgeError::Stale);
        }
        let body = bridge
            .document()
            .and_then(|d| {
                d.with_current(|m| m.verified(self.previous.request))
                    .flatten()
            })
            .ok_or(BridgeError::Stale)?;
        if !body.matches_native(self.previous, &self.request.method, self.request.identity)
            || body.head().logical_url != self.request.logical_url
        {
            return Err(BridgeError::Stale);
        }
        if bridge.closing.swap(true, Ordering::AcqRel) {
            return Err(BridgeError::Stale);
        }
        self.detached = true;
        self.armed.store(true, Ordering::Release);
        bridge.retire_contents(false);
        Ok(self)
    }

    pub(super) fn valid(&self) -> bool {
        self.native.upgrade().is_some() && self.epoch.is_active() && (self.authority)()
    }
    pub(super) fn consumed(&self) {
        self.armed.store(false, Ordering::Release);
    }
    pub(super) fn abandon(&self) {
        if !self.armed.swap(false, Ordering::AcqRel) {
            return;
        }
        let command = HttpRequest::Close {
            entry: self.entry,
            source_generation: self.scope.source_generation,
        };
        let epoch = self.epoch.clone();
        if let Ok(frame) = command.frame()
            && self
                .sender
                .send_guarded(
                    OutFrame {
                        tag: frame.tag,
                        payload: frame.payload,
                    },
                    Arc::new(move || epoch.is_active()),
                )
                .is_err()
        {
            tracing::debug!("unused HTTP successor cleanup not enqueued");
        }
    }
}
impl Drop for ReloadAuthority {
    fn drop(&mut self) {
        self.abandon();
        self.reserved.store(false, Ordering::Release);
    }
}
impl HttpBridge {
    /// Call only for the actual current main response, before closing its session/clear barrier.
    /// Its native allocation must survive same-view Reload; source/grant checks must not depend
    /// on the old document generation that this call intentionally retires.
    pub fn capture_reload(
        self: &Arc<Self>,
        response: &NativeResponse,
        native: &Arc<()>,
        authority: Current,
    ) -> Result<ReloadAuthority, BridgeError> {
        if !self.current() || !authority() {
            return Err(BridgeError::Stale);
        }
        let document = self.document().ok_or(BridgeError::Stale)?;
        let verified = document
            .with_current(|m| m.verified(response.body.head().id.request))
            .flatten()
            .ok_or(BridgeError::Invalid)?;
        if !Arc::ptr_eq(&verified, &response.body)
            || !response.body.matches_native(
                response.body.head().id,
                &response.request.method,
                response.request.identity,
            )
            || response.body.head().logical_url != response.request.logical_url
        {
            return Err(BridgeError::Invalid);
        }
        let inner = self.lock()?;
        if !response.belongs_to(&inner.slots) || !self.current() {
            return Err(BridgeError::Stale);
        }
        // At most one retained main receipt, replaceable by dropping the previous receipt.
        if self.reload_reserved.swap(true, Ordering::AcqRel) {
            return Err(BridgeError::Stale);
        }
        drop(inner);
        let receipt = ReloadAuthority {
            previous: response.body.head().id,
            request: response.request.clone(),
            scope: self.config.scope.clone(),
            native: Arc::downgrade(native),
            authority,
            sender: self.sender.clone(),
            epoch: self.sender.epoch(),
            entry: document.entry(),
            armed: AtomicBool::new(false),
            detached: false,
            original: Arc::downgrade(self),
            reserved: self.reload_reserved.clone(),
        };
        Ok(receipt)
    }
    /// Requires negotiated html-http-reload-v1 on the actual connection, then the same retained
    /// native owner and original source scope with a strictly larger document generation.
    pub fn new_successor(
        config: BridgeConfig,
        sender: Arc<ConnectionSender>,
        current: Current,
        wake: Arc<dyn Fn() + Send + Sync>,
        receipt: ReloadAuthority,
        native: &Arc<()>,
        negotiated: bool,
    ) -> Result<Arc<Self>, BridgeError> {
        let a = &receipt.scope;
        let b = &config.scope;
        if !negotiated
            || !receipt.detached
            || !receipt.valid()
            || !receipt.epoch.same(&sender.epoch())
            || !receipt.native.ptr_eq(&Arc::downgrade(native))
            || (
                a.epoch,
                a.surface_id,
                a.binding,
                a.source_generation,
                a.allow_remote_content,
                &a.entry_url,
            ) != (
                b.epoch,
                b.surface_id,
                b.binding,
                b.source_generation,
                b.allow_remote_content,
                &b.entry_url,
            )
            || b.document <= a.document
        {
            return Err(BridgeError::Stale);
        }
        let mut bridge = Self::new(config, sender, current, wake)?;
        Arc::get_mut(&mut bridge).ok_or(BridgeError::Failed)?.reload = Some(receipt);
        Ok(bridge)
    }
}
