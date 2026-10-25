//! Native policy consumes the exact verified response delivered for the actual method and upload.
use super::*;
use crate::webview::http_endpoint::listener::{EndpointError, Handler};
use tasty_remote::{html_http_body::VerifiedBody, html_http_native::NativeRequest};
#[derive(Default)]
pub(super) struct Deliveries {
    pending: Mutex<Vec<Delivery>>,
    main: Mutex<Option<Arc<VerifiedBody>>>,
    first: Mutex<Option<InitialRequest>>,
}
struct InitialRequest {
    method: String,
    url: String,
    upload: tasty_ipc::html_http::BodyIdentity,
}
struct Delivery {
    ticket: Option<crate::webview::http_endpoint::body::NativeDelivery>,
    body: Arc<VerifiedBody>,
    method: String,
    upload: tasty_ipc::html_http::BodyIdentity,
}
impl Deliveries {
    pub(super) fn handler(self: &Arc<Self>, inner: Handler, current: Current) -> Handler {
        let deliveries = self.clone();
        Arc::new(move |request: Arc<NativeRequest>| {
            let future = inner(request.clone());
            let deliveries = deliveries.clone();
            let current = current.clone();
            Box::pin(async move {
                if !current() {
                    return Err(EndpointError::Retired);
                }
                {
                    let mut first = deliveries.first.lock().map_err(|_| EndpointError::Body)?;
                    first.get_or_insert_with(|| InitialRequest {
                        method: request.method.clone(),
                        url: request.logical_url.clone(),
                        upload: request.identity,
                    });
                }
                let response = future.await?;
                if !current() {
                    return Err(EndpointError::Retired);
                }
                let mut pending = deliveries.pending.lock().map_err(|_| EndpointError::Body)?;
                if pending.len() >= 256 {
                    return Err(EndpointError::Capacity);
                }
                pending.push(Delivery {
                    ticket: response.native_delivery(),
                    body: response.body().clone(),
                    method: request.method.clone(),
                    upload: request.identity,
                });
                Ok(response)
            })
        })
    }
    pub(super) fn initial_get(&self, url: &str) -> bool {
        self.main().is_none()
            && self.first.lock().ok().is_some_and(|first| {
                first.as_ref().is_some_and(|initial| {
                    initial.method == "GET" && initial.url == url && initial.upload.bytes == 0
                })
            })
    }
    pub(super) fn main(&self) -> Option<Arc<VerifiedBody>> {
        self.main.lock().ok()?.clone()
    }
    fn select(&self, url: &str, http_method: &str) -> Result<Delivery, String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "native response publication poisoned")?;
        let mut matching = pending.iter().enumerate().filter(|(_, delivery)| {
            delivery.body.head().logical_url == url && delivery.method == http_method
        });
        let index = matching
            .next()
            .map(|(index, _)| index)
            .ok_or("native main response has no verified delivery")?;
        if matching.next().is_some() {
            return Err("native main response delivery is ambiguous".into());
        }
        let delivery = pending.remove(index);
        if !delivery
            .body
            .matches_native(delivery.body.head().id, &delivery.method, delivery.upload)
        {
            return Err("native main response identity mismatch".into());
        }
        Ok(delivery)
    }
}
impl HttpSession {
    pub(crate) fn can_restart_initial_get(&self) -> bool {
        self.is_current() && self.deliveries.initial_get(self.source.entry_url())
    }
    pub(crate) fn discard_nonmain_response(&self, url: &str, http_method: &str) {
        let discarded = {
            let Ok(mut pending) = self.deliveries.pending.lock() else {
                return;
            };
            let mut discarded = Vec::new();
            let mut index = 0;
            while index < pending.len() {
                if pending[index].body.head().logical_url == url
                    && pending[index].method == http_method
                {
                    discarded.push(pending.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            discarded
        };
        for delivery in discarded {
            if let Some(ticket) = delivery.ticket {
                let _discarded = ticket.discard();
            }
        }
    }
    pub(crate) fn claim_main_response(
        &self,
        url: &str,
        http_method: &str,
    ) -> Result<Arc<VerifiedBody>, String> {
        if !self.is_current() {
            return Err("native main response source retired".into());
        }
        let url = url.split('#').next().unwrap_or(url);
        let mut delivery = match self.deliveries.select(url, http_method) {
            Ok(delivery) => delivery,
            Err(error) => {
                self.discard_nonmain_response(url, http_method);
                return Err(error);
            }
        };
        let response = delivery
            .ticket
            .take()
            .ok_or("actual native delivery ticket unavailable")?
            .claim()
            .map_err(|error| format!("actual native response claim: {error:?}"))?;
        if !Arc::ptr_eq(&response.body, &delivery.body)
            || response.request.method != delivery.method
            || response.request.identity != delivery.upload
        {
            return Err("actual native response owner mismatch".into());
        }
        self.capture_reload(&response)?;
        // Capture authority while this exact response still owns its delivered slot.
        drop(response);
        *self
            .deliveries
            .main
            .lock()
            .map_err(|_| "native main response state poisoned")? = Some(delivery.body.clone());
        Ok(delivery.body)
    }
}

#[cfg(test)]
mod tests;
