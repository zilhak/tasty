//! One main-response receipt follows the actual retained native allocation across documents.
use super::*;
use std::sync::Weak;
use tasty_remote::html_http_bridge::{NativeResponse, ReloadAuthority};

pub(crate) struct ReloadSetup {
    pub authority: Current,
    pub negotiated: bool,
    pub predecessor: Option<Successor>,
}
pub(crate) struct Successor {
    receipt: ReloadAuthority,
    native: Weak<()>,
}
pub(super) struct ReloadState {
    authority: Current,
    negotiated: bool,
    native: Mutex<Weak<()>>,
    captured: Mutex<Option<ReloadAuthority>>,
    successor: Mutex<Option<ReloadAuthority>>,
}
impl ReloadSetup {
    pub(super) fn build(
        self,
        config: BridgeConfig,
        sender: Arc<ConnectionSender>,
        current: Current,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<(Arc<HttpBridge>, ReloadState), String> {
        let (bridge, native) = if let Some(previous) = self.predecessor {
            let native = previous
                .native
                .upgrade()
                .ok_or("retained native owner retired")?;
            let bridge = HttpBridge::new_successor(
                config,
                sender,
                current,
                wake,
                previous.receipt,
                &native,
                self.negotiated,
            );
            (bridge, Arc::downgrade(&native))
        } else {
            (HttpBridge::new(config, sender, current, wake), Weak::new())
        };
        let bridge = bridge.map_err(|error| format!("native HTTP bridge: {error:?}"))?;
        Ok((
            bridge,
            ReloadState {
                authority: self.authority,
                negotiated: self.negotiated,
                native: Mutex::new(native),
                captured: Mutex::default(),
                successor: Mutex::default(),
            },
        ))
    }
}
impl HttpSession {
    pub(crate) fn bind_native_owner(&self, native: &Arc<()>) -> Result<(), String> {
        let mut owner = self
            .reload
            .native
            .lock()
            .map_err(|_| "native lifetime poisoned")?;
        if owner.strong_count() != 0 && !owner.ptr_eq(&Arc::downgrade(native)) {
            return Err("native lifetime replacement refused".into());
        }
        *owner = Arc::downgrade(native);
        Ok(())
    }
    pub(super) fn capture_reload(&self, response: &NativeResponse) -> Result<(), String> {
        let native = self
            .reload
            .native
            .lock()
            .map_err(|_| "native lifetime poisoned")?
            .upgrade()
            .ok_or("actual native owner retired")?;
        if !self.is_current() || !(self.reload.authority)() {
            return Err("native main source authority retired".into());
        }
        if !self.reload.negotiated {
            return Ok(());
        }
        // Release the previous unretired reservation before capturing this actual main response.
        let old = self
            .reload
            .captured
            .lock()
            .map_err(|_| "native receipt poisoned")?
            .take();
        drop(old);
        let receipt = self
            .bridge
            .capture_reload(response, &native, self.reload.authority.clone())
            .map_err(|error| format!("native main Reload authority: {error:?}"))?;
        *self
            .reload
            .captured
            .lock()
            .map_err(|_| "native receipt poisoned")? = Some(receipt);
        Ok(())
    }
    pub(crate) fn retire_reload(&self) -> Result<(), String> {
        let receipt = self
            .reload
            .captured
            .lock()
            .map_err(|_| "native receipt poisoned")?
            .take();
        if let Some(receipt) = receipt {
            let retired = receipt
                .retire()
                .map_err(|error| format!("native Reload retirement: {error:?}"))?;
            *self
                .reload
                .successor
                .lock()
                .map_err(|_| "native successor poisoned")? = Some(retired);
        } else if self.reload.negotiated && self.deliveries.main().is_some() {
            return Err("actual main Reload receipt unavailable".into());
        }
        Ok(())
    }
    pub(super) fn close_reload(&self) {
        // reason: never recover a partially mutated receipt; closed source fences revoke it,
        // and any inaccessible bounded reservation stays owned until this session is dropped.
        let captured = self
            .reload
            .captured
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        drop(captured);
        if !self.retaining() {
            // reason: a poisoned successor cannot be reused; final session Drop releases its owner.
            let successor = self
                .reload
                .successor
                .lock()
                .ok()
                .and_then(|mut slot| slot.take());
            drop(successor);
        }
    }
    pub(crate) fn take_successor(&self) -> Result<Option<Successor>, String> {
        let receipt = self
            .reload
            .successor
            .lock()
            .map_err(|_| "native successor poisoned")?
            .take();
        let native = self
            .reload
            .native
            .lock()
            .map_err(|_| "native lifetime poisoned")?
            .clone();
        Ok(receipt.map(|receipt| Successor { receipt, native }))
    }
}
