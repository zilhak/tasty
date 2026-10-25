//! A same-view barrier; external retirement/pin work completes before entry.
use super::*;
use continuation::ReadyDone;
use gtk::gio::prelude::*;
use webkit2gtk::{NetworkProxyMode, NetworkProxySettings};

enum Phase {
    Joining,
    Clearing {
        config: ContextConfig,
        done: ReadyDone,
        cancel: gio::Cancellable,
    },
    Ready,
}
pub(super) struct Operation {
    id: u64,
    view: Rc<view::State>,
    action: NativeContinuation,
    phase: Phase,
}
impl store::State {
    pub(super) fn retire_document(
        self: &Rc<Self>,
        view: &Rc<view::State>,
        action: NativeContinuation,
    ) -> Result<RetiredView, ContextError> {
        if self.closed.get() || view.closed.get() {
            return Err(ContextError::Retired);
        }
        if self.initial_pending() || self.replacement.borrow().is_some() {
            return Err(ContextError::Busy);
        }
        if !self
            .active
            .borrow()
            .upgrade()
            .is_some_and(|v| Rc::ptr_eq(&v, view))
            || !action.belongs_to(view)
        {
            return Err(ContextError::Policy);
        }
        let id = self.next_id()?;
        view.revoke()?;
        if action.is_reload() {
            view.webview.stop_loading();
        }
        *self.replacement.borrow_mut() = Some(Operation {
            id,
            view: view.clone(),
            action,
            phase: Phase::Joining,
        });
        Ok(RetiredView {
            store: Some(self.clone()),
            id,
        })
    }
    pub(super) fn clear_retained(self: &Rc<Self>, id: u64, config: ContextConfig, done: ReadyDone) {
        let target = self
            .replacement
            .borrow()
            .as_ref()
            .filter(|op| op.id == id && matches!(op.phase, Phase::Joining))
            .map(|op| op.action.target.clone());
        let Some(target) = target else {
            done(Err(ContextError::Retired));
            return;
        };
        if let Err(error) = self.validate_replacement(&config, target.as_deref()) {
            self.abandon_retained(id);
            done(Err(error));
            return;
        }
        // The configuration callbacks can reenter/close this store. Recheck after
        // invoking them, before publishing a clear or replacing operation state.
        let cancel = gio::Cancellable::new();
        {
            let mut slot = self.replacement.borrow_mut();
            let Some(op) = slot
                .as_mut()
                .filter(|op| op.id == id && matches!(op.phase, Phase::Joining))
            else {
                drop(slot);
                done(Err(ContextError::Retired));
                return;
            };
            op.phase = Phase::Clearing {
                config,
                done,
                cancel: cancel.clone(),
            };
        }
        let weak = glib::thread_guard::ThreadGuard::new(Rc::downgrade(self));
        // The native callback pins both the profile and the retained GTK owner,
        // even if close resolves the logical callback before native completion.
        let retained = glib::thread_guard::ThreadGuard::new(
            self.replacement.borrow().as_ref().map(|op| op.view.clone()),
        );
        self.manager.clear(
            store::clear_types(),
            glib::TimeSpan::from_microseconds(0),
            Some(&cancel),
            move |result| {
                let _retained = retained.into_inner();
                if let Some(state) = weak.into_inner().upgrade() {
                    state.finish_retained(id, result.is_ok());
                }
            },
        );
    }
    pub(super) fn validate_replacement(
        &self,
        config: &ContextConfig,
        target: Option<&str>,
    ) -> Result<(), ContextError> {
        if self.closed.get() || !(config.current)() {
            return Err(ContextError::Retired);
        }
        if !authority::proxy(&config.proxy_uri) {
            return Err(ContextError::Policy);
        }
        let target = target
            .and_then(|t| url::Url::parse(t).ok())
            .ok_or(ContextError::Policy)?;
        if !matches!(target.scheme(), "http" | "https")
            || target.host().is_none()
            || !target.username().is_empty()
            || target.password().is_some()
        {
            return Err(ContextError::Policy);
        }
        if self.history.borrow().full() {
            return Err(ContextError::Capacity);
        }
        if target.scheme() == "https" {
            let authority = authority::https(target.as_str()).ok_or(ContextError::Policy)?;
            let leaf =
                (config.expected_leaf)(&authority.host, authority.port).ok_or(ContextError::Tls)?;
            self.history.borrow().check(leaf)?;
        }
        Ok(())
    }
    pub(super) fn finish_retained(self: &Rc<Self>, id: u64, success: bool) {
        let snapshot = self
            .replacement
            .borrow()
            .as_ref()
            .filter(|op| op.id == id)
            .and_then(|op| {
                if let Phase::Clearing { config, .. } = &op.phase {
                    Some((op.view.clone(), op.action.target.clone(), config.clone()))
                } else {
                    None
                }
            });
        let Some((view, target, config)) = snapshot else {
            return;
        };
        let result = if view.closed.get() {
            Err(ContextError::Retired)
        } else {
            self.validate_replacement(&config, target.as_deref())
        };
        let result = result.and({
            if success {
                Ok(())
            } else {
                Err(ContextError::Cache)
            }
        });
        let mut operation = {
            let mut slot = self.replacement.borrow_mut();
            if !slot
                .as_ref()
                .is_some_and(|op| op.id == id && matches!(op.phase, Phase::Clearing { .. }))
            {
                return;
            }
            slot.take().expect("matched replacement")
        };
        let phase = std::mem::replace(&mut operation.phase, Phase::Ready);
        let Phase::Clearing { done, .. } = phase else {
            unreachable!("matched clear phase");
        };
        if let Err(error) = result {
            drop(operation);
            done(Err(error));
            return;
        }
        *operation.view.config.borrow_mut() = config;
        *self.replacement.borrow_mut() = Some(operation);
        done(Ok(ReadyView {
            store: Some(self.clone()),
            id,
        }));
    }
    pub(super) fn resume_retained(&self, id: u64) -> Result<(), ContextError> {
        let target = self
            .replacement
            .borrow()
            .as_ref()
            .filter(|op| op.id == id && matches!(op.phase, Phase::Ready))
            .map(|op| (op.view.clone(), op.action.target.clone()));
        let Some((view, target)) = target else {
            return Err(ContextError::Retired);
        };
        let config = view.config.borrow();
        // Clone callbacks before invoking caller code; never hold a config borrow
        // through a callback that might retire/close the same view.
        let cfg = ContextConfig {
            proxy_uri: config.proxy_uri.clone(),
            current: config.current.clone(),
            expected_leaf: config.expected_leaf.clone(),
        };
        drop(config);
        if let Err(error) = self.validate_replacement(&cfg, target.as_deref()) {
            self.abandon_retained(id);
            return Err(error);
        }
        let op = {
            let mut slot = self.replacement.borrow_mut();
            if !slot
                .as_ref()
                .is_some_and(|op| op.id == id && matches!(op.phase, Phase::Ready))
            {
                return Err(ContextError::Retired);
            }
            slot.take().expect("matched ready continuation")
        };
        if view.closed.get() || self.closed.get() {
            return Err(ContextError::Retired);
        }
        if !op.action.still_matches(&view.webview) {
            return Err(ContextError::Policy);
        }
        self.configure_proxy(&cfg);
        view.blocked.set(false);
        op.action.resume(&view.webview);
        Ok(())
    }
    pub(super) fn abandon_retained(&self, id: u64) {
        let operation = {
            let mut slot = self.replacement.borrow_mut();
            if slot.as_ref().is_none_or(|op| op.id != id) {
                return;
            }
            slot.take()
        };
        if let Some(op) = operation {
            cancel(op);
        }
    }
    pub(super) fn cancel_retained(&self) {
        let op = self.replacement.borrow_mut().take();
        if let Some(op) = op {
            cancel(op);
        }
    }
    pub(super) fn cancel_view(&self, view: &view::State) {
        let id = self
            .replacement
            .borrow()
            .as_ref()
            .filter(|op| std::ptr::eq(op.view.as_ref(), view))
            .map(|op| op.id);
        if let Some(id) = id {
            self.abandon_retained(id);
        }
    }
    fn configure_proxy(&self, config: &ContextConfig) {
        let mut proxy = NetworkProxySettings::new(Some(&config.proxy_uri), &[]);
        self.manager
            .set_network_proxy_settings(NetworkProxyMode::Custom, Some(&mut proxy));
    }
}
fn cancel(op: Operation) {
    let Operation { action, phase, .. } = op;
    drop(action);
    if let Phase::Clearing { cancel, done, .. } = phase {
        cancel.cancel();
        done(Err(ContextError::Retired));
    }
}
