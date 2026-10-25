use super::*;
use authority::Authority;
use gtk::{gio::prelude::*, prelude::*};
use sha2::{Digest, Sha256};
use std::time::Duration;
use webkit2gtk::{LoadEvent, NavigationPolicyDecision, PolicyDecision, PolicyDecisionType};
pub(super) const LIMIT: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(10);
pub(super) struct Probe {
    permit: allowance::Permit,
    authority: Authority,
    expected: [u8; 32],
    decision: Option<PolicyDecision>,
    webview: WebView,
    timer: Option<glib::SourceId>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
        if let Some(decision) = self.decision.take() {
            decision.ignore();
        }
        self.webview.stop_loading();
        // SAFETY: an owned probe is never embedded; destroy it only on this GTK thread.
        unsafe {
            self.webview.destroy();
        }
    }
}
pub(super) fn install(state: &Rc<view::State>) {
    let weak = Rc::downgrade(state);
    state
        .webview
        .connect_decide_policy(move |_, decision, kind| {
            if kind != PolicyDecisionType::NavigationAction {
                return false;
            }
            let uri = decision
                .downcast_ref::<NavigationPolicyDecision>()
                .and_then(|d| d.navigation_action())
                .and_then(|a| a.request())
                .and_then(|r| r.uri());
            let Some(uri) = uri else {
                return false;
            };
            if !uri.to_ascii_lowercase().starts_with("https:") {
                return false;
            }
            let Some(state) = weak.upgrade() else {
                decision.ignore();
                return true;
            };
            navigate(&state, decision, &uri)
        });
    let weak = Rc::downgrade(state);
    state
        .webview
        .connect_load_failed_with_tls_errors(move |_, _, _, _| {
            if let Some(state) = weak.upgrade() {
                state.failure.set(Some(ContextError::Tls));
                state.webview.stop_loading();
            }
            true
        });
}
fn navigate(state: &Rc<view::State>, decision: &PolicyDecision, uri: &str) -> bool {
    let authority = authority::https(uri);
    let expected = authority
        .as_ref()
        .and_then(|a| authority::pin(&state.config, a));
    if !state.live() || expected.is_none() {
        decision.ignore();
        state.failure.set(Some(ContextError::Tls));
        return true;
    }
    let authority = authority.expect("validated authority");
    let expected = expected.expect("validated pin");
    if let Some(trusted) = state.trusted.borrow().get(&authority) {
        if *trusted == expected {
            return false;
        }
        decision.ignore();
        state.failure.set(Some(ContextError::Tls));
        return true;
    }
    if state.pending.borrow().len() + state.resuming.borrow().len() >= LIMIT
        || state.trusted.borrow().len() >= LIMIT
    {
        decision.ignore();
        state.failure.set(Some(ContextError::Capacity));
        return true;
    }
    // Duplicate authority decisions cannot race their outstanding probe.
    if state
        .pending
        .borrow()
        .values()
        .any(|p| p.authority == authority)
    {
        decision.ignore();
        state.failure.set(Some(ContextError::Busy));
        return true;
    }
    let permit = match allowance::History::reserve(&state.history, expected) {
        Ok(permit) => permit,
        Err(error) => {
            decision.ignore();
            state.failure.set(Some(error));
            return true;
        }
    };
    start(state, decision.clone(), authority, expected, uri, permit);
    true
}
fn start(
    state: &Rc<view::State>,
    decision: PolicyDecision,
    authority: Authority,
    expected: [u8; 32],
    uri: &str,
    permit: allowance::Permit,
) {
    let Some(id) = state.sequence.get().checked_add(1) else {
        decision.ignore();
        state.failure.set(Some(ContextError::Capacity));
        return;
    };
    state.sequence.set(id);
    #[cfg(test)]
    state.held_methods.borrow_mut().push(
        decision
            .downcast_ref::<NavigationPolicyDecision>()
            .and_then(|d| d.navigation_action())
            .and_then(|a| a.request())
            .and_then(|r| r.http_method())
            .map(|m| m.to_string())
            .unwrap_or_default(),
    );
    let webview = WebView::with_context(&state.context);
    store::retain_profile(&webview, &state.profile);
    if let Some(settings) = WebViewExt::settings(&webview) {
        settings.set_enable_javascript(false);
    }
    callbacks(state, &webview, id);
    let weak = Rc::downgrade(state);
    let timer = glib::timeout_add_local_once(TIMEOUT, move || {
        if let Some(state) = weak.upgrade() {
            if let Some(probe) = state.pending.borrow_mut().get_mut(&id) {
                probe.timer.take();
            }
            finish(&state, id, false);
        }
    });
    state.pending.borrow_mut().insert(
        id,
        Probe {
            permit,
            authority,
            expected,
            decision: Some(decision),
            webview: webview.clone(),
            timer: Some(timer),
        },
    );
    webview.load_uri(uri);
}
fn callbacks(state: &Rc<view::State>, webview: &WebView, id: u64) {
    let weak = Rc::downgrade(state);
    webview.connect_load_failed_with_tls_errors(move |_, uri, certificate, flags| {
        if let Some(state) = weak.upgrade() {
            #[cfg(test)]
            {
                state.observations.borrow_mut().push(flags.bits());
                let leaf = certificate.certificate().map(|der| {
                    Sha256::digest(der.as_ref())
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                });
                let host = authority::https(uri).map(|a| (a.host, a.port));
                tracing::info!(
                    "NATIVE_TLS flags={} leaf={leaf:?} authority={host:?} context={:p} current={}",
                    flags.bits(),
                    state.context.as_ptr(),
                    state.live()
                );
            }
            let valid = valid(&state, id, uri, certificate, flags);
            if valid {
                if let Some(probe) = state.pending.borrow_mut().get_mut(&id) {
                    probe.permit.commit();
                }
                let host = state
                    .pending
                    .borrow()
                    .get(&id)
                    .map(|p| p.authority.host.clone());
                if let Some(host) = host {
                    state
                        .context
                        .allow_tls_certificate_for_host(certificate, &host);
                }
            }
            finish(&state, id, valid);
        }
        true
    });
    let weak = Rc::downgrade(state);
    webview.connect_load_changed(move |_, event| {
        if event == LoadEvent::Finished
            && let Some(state) = weak.upgrade()
        {
            finish(&state, id, false);
        }
    });
}
fn valid(
    state: &view::State,
    id: u64,
    uri: &str,
    certificate: &gio::TlsCertificate,
    flags: gio::TlsCertificateFlags,
) -> bool {
    if !state.live() || flags != gio::TlsCertificateFlags::UNKNOWN_CA {
        return false;
    }
    let Some(actual) = authority::https(uri) else {
        return false;
    };
    let pending = state.pending.borrow();
    let Some(probe) = pending.get(&id) else {
        return false;
    };
    let Some(der) = certificate.certificate() else {
        return false;
    };
    actual == probe.authority
        && authority::pin(&state.config, &actual) == Some(probe.expected)
        && <[u8; 32]>::from(Sha256::digest(der.as_ref())) == probe.expected
}
fn finish(state: &Rc<view::State>, id: u64, accepted: bool) {
    let Some(mut probe) = state.pending.borrow_mut().remove(&id) else {
        return;
    };
    // Stop/destroy probe before resuming the original held decision; never retry its GET.
    probe.webview.stop_loading();
    let decision = probe.decision.take();
    if accepted && state.live() {
        state
            .trusted
            .borrow_mut()
            .insert(probe.authority.clone(), probe.expected);
        drop(probe);
        if let Some(decision) = decision {
            let weak = Rc::downgrade(state);
            let source = glib::idle_add_local_once(move || {
                if let Some(state) = weak.upgrade() {
                    let resume = state.resuming.borrow_mut().remove(&id);
                    if let Some(mut resume) = resume {
                        resume.source.take();
                        if let Some(decision) = resume.decision.take() {
                            if state.live() {
                                decision.use_();
                            } else {
                                decision.ignore();
                            }
                        }
                    }
                }
            });
            state.resuming.borrow_mut().insert(
                id,
                Resume {
                    decision: Some(decision),
                    source: Some(source),
                },
            );
        }
    } else {
        state.failure.set(Some(ContextError::Tls));
        if let Some(decision) = decision {
            decision.ignore();
        }
    }
}

pub(super) struct Resume {
    decision: Option<PolicyDecision>,
    source: Option<glib::SourceId>,
}
impl Drop for Resume {
    fn drop(&mut self) {
        if let Some(source) = self.source.take() {
            source.remove();
        }
        if let Some(decision) = self.decision.take() {
            decision.ignore();
        }
    }
}
