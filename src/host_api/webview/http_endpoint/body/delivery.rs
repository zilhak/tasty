//! One bounded, opaque handoff of the actual response lease to trusted native policy.
use super::*;
use std::sync::Weak;

pub(crate) struct NativeDelivery {
    source: Weak<BodySource>,
    response: Weak<NativeResponse>,
}
impl NativeDelivery {
    /// Only actual, unambiguous main-response policy may turn this into Reload authority.
    pub(crate) fn claim(mut self) -> Result<Arc<NativeResponse>, EndpointError> {
        let source = self.source.upgrade().ok_or(EndpointError::Retired)?;
        if !source.current() {
            return Err(EndpointError::Retired);
        }
        let response = source
            .take_delivery(&self.response)?
            .ok_or(EndpointError::Identity)?;
        self.source = Weak::new();
        if !source.current() {
            return Err(EndpointError::Retired);
        }
        Ok(response)
    }
    pub(crate) fn discard(mut self) -> bool {
        let discarded = self.release();
        self.source = Weak::new();
        discarded
    }
    fn release(&self) -> bool {
        self.source.upgrade().is_some_and(|source| {
            source
                .take_delivery(&self.response)
                .ok()
                .flatten()
                .is_some()
        })
    }
}
impl Drop for NativeDelivery {
    fn drop(&mut self) {
        let _discarded = self.release();
    }
}
struct Owner {
    response: Arc<NativeResponse>,
    source: Weak<BodySource>,
    written: AtomicBool,
}
impl ResponseOwner for Owner {
    fn body(&self) -> &Arc<VerifiedBody> {
        &self.response.body
    }
    fn request(&self) -> &Arc<NativeRequest> {
        &self.response.request
    }
    fn native_delivery(&self) -> Option<NativeDelivery> {
        Some(NativeDelivery {
            source: self.source.clone(),
            response: Arc::downgrade(&self.response),
        })
    }
    fn write_completed(&self, success: bool) {
        self.written.store(success, Ordering::Release);
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if !self.written.load(Ordering::Acquire) {
            let ticket = NativeDelivery {
                source: self.source.clone(),
                response: Arc::downgrade(&self.response),
            };
            drop(ticket);
        }
    }
}
impl BodySource {
    fn delivery_lock(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<Arc<NativeResponse>>>, EndpointError> {
        self.delivery.lock().map_err(|_| {
            self.live.store(false, Ordering::Release);
            self.stop.send_replace(true);
            tracing::error!("native response delivery state poisoned; claims refused");
            EndpointError::Worker
        })
    }
    fn take_delivery(
        &self,
        response: &Weak<NativeResponse>,
    ) -> Result<Option<Arc<NativeResponse>>, EndpointError> {
        let mut pending = self.delivery_lock()?;
        if pending
            .as_ref()
            .is_some_and(|pending| Arc::as_ptr(pending) == response.as_ptr())
        {
            Ok(pending.take())
        } else {
            Ok(None)
        }
    }
    pub(super) fn publish_delivery(
        self: &Arc<Self>,
        response: NativeResponse,
    ) -> Result<Box<dyn ResponseOwner>, EndpointError> {
        let response = Arc::new(response);
        let previous = {
            let mut pending = self.delivery_lock()?;
            if !self.current() {
                return Err(EndpointError::Retired);
            }
            pending.replace(response.clone())
        };
        // NativeResponse Drop wakes the bridge; do not invoke it under the delivery lock.
        drop(previous);
        if !self.current() {
            drop(self.take_delivery(&Arc::downgrade(&response))?);
            return Err(EndpointError::Retired);
        }
        Ok(Box::new(Owner {
            response,
            source: Arc::downgrade(self),
            written: AtomicBool::new(false),
        }))
    }
    pub(super) fn retire_delivery(&self) {
        let pending = match self.delivery_lock() {
            Ok(mut pending) => pending.take(),
            Err(_) => return,
        };
        drop(pending);
    }
}
