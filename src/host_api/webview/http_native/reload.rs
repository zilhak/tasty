//! Same native view, selected-clear barrier and one captured original Reload continuation.
use super::super::http_context::linux::{ReadyView, RetiredView};
use super::*;
use webkit2gtk::WebViewExt;
pub(super) struct Replacement {
    old: Arc<HttpSession>,
    target: String,
    retired: Option<RetiredView>,
    prepared: PendingNative,
    result: Rc<RefCell<Option<Result<ReadyView, super::super::http_context::linux::ContextError>>>>,
}
impl NativeDocument {
    pub(crate) fn session_slot(&self) -> Rc<RefCell<Arc<HttpSession>>> {
        self.session.clone()
    }
    pub(crate) fn session(&self) -> Arc<HttpSession> {
        self.session.borrow().clone()
    }
    pub(crate) fn retaining(&self) -> bool {
        self.replacement.borrow().is_some()
    }
    pub(crate) fn request_reload(&self) -> Result<(), String> {
        if self.retaining() {
            return Err("native HTTP replacement already pending".into());
        }
        let old = self.session();
        let uri = self.view.webview().uri();
        tracing::debug!(
            uri_unset = uri.is_none(),
            uri_empty = uri.as_deref() == Some(""),
            uri_blank = uri.as_deref() == Some("about:blank"),
            initial_get = old.can_restart_initial_get(),
            "native HTTP Reload capture"
        );
        let action =
            if uri.as_ref().is_none_or(|uri| uri.is_empty()) && old.can_restart_initial_get() {
                self.view
                    .initial_get_continuation(old.source.entry_url())
                    .map_err(|error| format!("initial native GET Reload: {error:?}"))?
            } else {
                self.view.reload_continuation()
            };
        let target =
            if uri.as_ref().is_none_or(|uri| uri.is_empty()) && old.can_restart_initial_get() {
                old.source.entry_url().to_owned()
            } else {
                uri.as_ref()
                    .map(|uri| uri.to_string())
                    .ok_or("native Reload target unavailable")?
            };
        old.retire_reload()?;
        let retired = self
            .store
            .retire_document(&self.view, action)
            .map_err(|error| {
                old.fail();
                format!("native HTTP retirement: {error:?}")
            })?;
        old.request_retained();
        old.close();
        *self.replacement.borrow_mut() = Some(Replacement {
            old: old.clone(),
            target,
            retired: Some(retired),
            prepared: PendingNative::new(old),
            result: Rc::default(),
        });
        Ok(())
    }
    pub(crate) fn poll_reload(
        &self,
        session: Arc<HttpSession>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Option<ReadyView>, String> {
        let mut pending = self.replacement.borrow_mut();
        let Some(replacement) = pending.as_mut() else {
            return Ok(None);
        };
        if !replacement.old.endpoint_joined()? || Arc::ptr_eq(&replacement.old, &session) {
            return Ok(None);
        }
        if !session.is_current() {
            return Err("replacement HTTP session retired".into());
        }
        if Arc::ptr_eq(&replacement.old, &replacement.prepared.session) {
            replacement.prepared = PendingNative::new(session.clone());
            replacement.prepared.authority_url = replacement.target.clone();
        }
        if replacement.retired.is_some() {
            if !replacement.prepared.poll_pin()? {
                return Ok(None);
            }
            let config = replacement.prepared.context_config()?;
            let result = replacement.result.clone();
            replacement
                .retired
                .take()
                .expect("one-shot retirement")
                .after_endpoint_join(
                    config,
                    Box::new(move |ready| {
                        *result.borrow_mut() = Some(ready);
                        wake();
                    }),
                );
        }
        let result = replacement.result.borrow_mut().take();
        let Some(result) = result else {
            return Ok(None);
        };
        let ready = result.map_err(|error| format!("retained HTTP clear: {error:?}"))?;
        session.bind_native_owner(&self.native_owner)?;
        *self.session.borrow_mut() = session;
        *self.pin.borrow_mut() = replacement.prepared.pin.take();
        pending.take();
        Ok(Some(ready))
    }
}
