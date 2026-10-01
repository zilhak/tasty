//! Script authority and navigation proofs remain in App/runtime; Views receive values and bound requests.
use super::engine_action::SurfaceBinding;
use crate::plugin_bridge::remote_surface::RemoteSurface;
use crate::runtime::engine_access::{EngineMut, EngineRef};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tasty_model::html_script::{
    BannerPhase, DocumentRecord, HtmlScriptState, ScriptDetection, ScriptMarker,
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum HtmlActionKind {
    Allow,
    Dismiss,
    Reshow,
}
#[derive(Clone, Debug)]
pub(crate) struct HtmlAction {
    target: SurfaceBinding,
    surface: u32,
    owner: Weak<Mutex<HtmlScriptState>>,
    document: Option<DocumentRecord>,
    kind: HtmlActionKind,
    consumed: Arc<std::sync::atomic::AtomicBool>,
}
#[derive(Clone)]
pub(crate) struct HtmlSnapshot {
    pub(crate) phase: BannerPhase,
    pub(crate) remote_only: bool,
    pub(crate) marker: Option<ScriptMarker>,
    request: HtmlAction,
}
impl HtmlSnapshot {
    pub(crate) fn request(&self, kind: HtmlActionKind) -> HtmlAction {
        let mut request = self.request.clone();
        request.kind = kind;
        request.consumed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        request
    }
}
pub(crate) fn snapshot(remote: &RemoteSurface, target: SurfaceBinding) -> HtmlSnapshot {
    let surface = remote.id;
    remote.with_html_script(|state| HtmlSnapshot {
        phase: state.banner_phase(),
        remote_only: state.current_detection() == Some(ScriptDetection::ScriptsRemoteOnly),
        marker: state.marker(),
        request: HtmlAction {
            target,
            surface,
            owner: Arc::downgrade(&remote.html_script),
            document: state.current().cloned(),
            kind: HtmlActionKind::Dismiss,
            consumed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        },
    })
}
impl HtmlAction {
    pub(crate) fn apply(&self, engine: &mut EngineMut<'_>) {
        if self
            .consumed
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        if !self.target.current(&engine.as_ref()) {
            return;
        }
        let Some(remote) = engine
            .runtime
            .surfaces
            .get(&self.surface)
            .and_then(|surface| surface.as_any().downcast_ref::<RemoteSurface>())
        else {
            return;
        };
        if !Arc::downgrade(&remote.html_script).ptr_eq(&self.owner) {
            return;
        }
        remote.with_html_script(|state| {
            apply_decision(state, self.document.as_ref(), self.kind, self.surface)
        });
    }
}

fn apply_decision(
    state: &mut HtmlScriptState,
    document: Option<&DocumentRecord>,
    kind: HtmlActionKind,
    surface: u32,
) {
    // Record identity is the existing policy's fragment-free URL and content fingerprint.
    if state.current() != document {
        return;
    }
    match kind {
        HtmlActionKind::Allow => match state.allow_current() {
            Ok(()) => tracing::info!(surface, "html scripts allowed by the user"),
            Err(error) => tracing::warn!(surface, %error, "html script allowance refused"),
        },
        HtmlActionKind::Dismiss => state.dismiss_banner(),
        HtmlActionKind::Reshow => state.reshow_banner(),
    }
}

/// Identity only: no mutable resource or ScriptGate is exposed back to the View.
pub(crate) struct NativeWebviewBinding {
    target: SurfaceBinding,
    surface: u32,
    source: Weak<Mutex<Option<String>>>,
}
impl NativeWebviewBinding {
    pub(crate) fn capture(engine: &EngineRef<'_>, surface: u32) -> Option<Self> {
        let remote = engine
            .find_surface_by_id(surface)?
            .as_any()
            .downcast_ref::<RemoteSurface>()?;
        Some(Self {
            target: SurfaceBinding::capture(&engine.read(), surface)?,
            surface,
            source: Arc::downgrade(&remote.webview_url),
        })
    }
    pub(crate) fn current(&self, engine: &crate::runtime::engine_access::EngineRef<'_>) -> bool {
        self.target.current(engine)
            && engine
                .runtime
                .surfaces
                .get(&self.surface)
                .and_then(|surface| surface.as_any().downcast_ref::<RemoteSurface>())
                .is_some_and(|remote| Arc::downgrade(&remote.webview_url).ptr_eq(&self.source))
    }
}

struct ViewProofs {
    view: Weak<()>,
    records: crate::plugin_bridge::user_navigation::UserNavigations,
    sources: HashMap<u32, Weak<Mutex<Option<String>>>>,
}
#[derive(Default)]
pub(crate) struct NavigationProofs {
    views: Mutex<Vec<ViewProofs>>,
}
static PROOFS_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
impl NavigationProofs {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<ViewProofs>> {
        crate::poison::recover_mutex(
            self.views.lock(),
            "native navigation proofs",
            &PROOFS_POISONED,
        )
    }
    pub(crate) fn record(
        &self,
        view: &Weak<()>,
        surface: u32,
        owner: Option<&crate::plugin_bridge::user_navigation::NavigationOwner>,
        source: Option<&Arc<Mutex<Option<String>>>>,
        navigation: &crate::webview::PendingNavigation,
    ) {
        let mut views = self.lock();
        views.retain(|entry| entry.view.strong_count() != 0);
        if view.strong_count() == 0 {
            return;
        }
        let index = views
            .iter()
            .position(|entry| entry.view.ptr_eq(view))
            .unwrap_or_else(|| {
                views.push(ViewProofs {
                    view: view.clone(),
                    records: HashMap::new(),
                    sources: HashMap::new(),
                });
                views.len() - 1
            });
        let entry = &mut views[index];
        crate::plugin_bridge::user_navigation::record(
            &mut entry.records,
            surface,
            owner,
            navigation,
        );
        if entry.records.contains_key(&surface)
            && let Some(source) = source
        {
            entry.sources.insert(surface, Arc::downgrade(source));
        } else {
            entry.sources.remove(&surface);
        }
    }
    pub(crate) fn settle_frame(&self, view: &Weak<()>, live: &[u32], takeovers: &[(u32, bool)]) {
        let mut views = self.lock();
        views.retain(|entry| entry.view.strong_count() != 0);
        let Some(entry) = views.iter_mut().find(|entry| entry.view.ptr_eq(view)) else {
            return;
        };
        entry.records.retain(|surface, _| live.contains(surface));
        for &(surface, takeover) in takeovers {
            crate::plugin_bridge::user_navigation::settle_frame(
                &mut entry.records,
                surface,
                takeover,
            );
        }
        entry
            .sources
            .retain(|surface, _| entry.records.contains_key(surface));
    }
    pub(crate) fn invalidate(&self, view: &Weak<()>, surface: u32) {
        let mut views = self.lock();
        if let Some(entry) = views.iter_mut().find(|entry| entry.view.ptr_eq(view)) {
            entry.records.remove(&surface);
            entry.sources.remove(&surface);
        }
    }
    pub(crate) fn take(
        &self,
        view: &Weak<()>,
        engine: &EngineRef<'_>,
        plugin: &str,
        surface: u32,
        url: &str,
    ) -> bool {
        let source = engine
            .find_surface_by_id(surface)
            .and_then(|surface| surface.as_any().downcast_ref::<RemoteSurface>())
            .map(|remote| &remote.webview_url);
        self.take_current(view, source, plugin, surface, url)
    }
    fn take_current(
        &self,
        view: &Weak<()>,
        current: Option<&Arc<Mutex<Option<String>>>>,
        plugin: &str,
        surface: u32,
        url: &str,
    ) -> bool {
        let mut views = self.lock();
        views.retain(|entry| entry.view.strong_count() != 0);
        let Some(entry) = views.iter_mut().find(|entry| entry.view.ptr_eq(view)) else {
            return false;
        };
        if !entry.sources.get(&surface).is_some_and(|source| {
            current.is_some_and(|current| source.ptr_eq(&Arc::downgrade(current)))
        }) {
            entry.records.remove(&surface);
            entry.sources.remove(&surface);
            return false;
        }
        let taken =
            crate::plugin_bridge::user_navigation::take(&mut entry.records, plugin, surface, url);
        if taken {
            entry.sources.remove(&surface);
        }
        taken
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_model::html_script::{Fingerprint, ScriptScan};

    fn commit(state: &mut HtmlScriptState, url: &str, byte: u8) {
        state.on_load_started();
        state.on_main_response(
            url,
            Some(ScriptScan {
                fingerprint: Fingerprint([byte; 32]),
                detection: ScriptDetection::Scripts,
            }),
        );
        state.on_committed(Some(url));
        let _ = state.on_load_finished();
    }

    #[test]
    fn a_delayed_banner_decision_cannot_allow_or_dismiss_a_replaced_document() {
        let mut state = HtmlScriptState::new(true);
        commit(&mut state, "file:///document.html", 1);
        let drawn = state.current().cloned();
        commit(&mut state, "file:///document.html", 2);
        apply_decision(&mut state, drawn.as_ref(), HtmlActionKind::Allow, 1);
        assert!(state.allowance().is_none());
        assert!(!state.take_reload_request());
        apply_decision(&mut state, drawn.as_ref(), HtmlActionKind::Dismiss, 1);
        assert!(!state.banner().dismissed);
        let current = state.current().cloned();
        apply_decision(&mut state, current.as_ref(), HtmlActionKind::Allow, 1);
        assert!(state.current_is_allowed());
        assert!(state.take_reload_request());
        assert!(!state.take_reload_request());
    }

    #[test]
    fn navigation_claims_keep_the_original_view_plugin_url_and_one_shot_scope() {
        let proofs = NavigationProofs::default();
        let view = Arc::new(());
        let other_view = Arc::new(());
        let source = Arc::new(Mutex::new(Some("file:///document.html".into())));
        let owner = crate::plugin_bridge::user_navigation::NavigationOwner {
            plugin_id: "p".into(),
            wrote_page: true,
        };
        let nav = crate::webview::PendingNavigation {
            url: "file:///link.html".into(),
            user_gesture: true,
        };
        proofs.record(&Arc::downgrade(&view), 1, Some(&owner), Some(&source), &nav);
        assert!(!proofs.take_current(
            &Arc::downgrade(&other_view),
            Some(&source),
            "p",
            1,
            &nav.url
        ));
        assert!(!proofs.take_current(&Arc::downgrade(&view), Some(&source), "other", 1, &nav.url));
        assert!(proofs.take_current(&Arc::downgrade(&view), Some(&source), "p", 1, &nav.url));
        assert!(!proofs.take_current(&Arc::downgrade(&view), Some(&source), "p", 1, &nav.url));
    }

    #[test]
    fn replaced_resources_and_owner_takeover_invalidate_navigation_proof() {
        let proofs = NavigationProofs::default();
        let view = Arc::new(());
        let identity = Arc::downgrade(&view);
        let owner = crate::plugin_bridge::user_navigation::NavigationOwner {
            plugin_id: "p".into(),
            wrote_page: true,
        };
        let nav = crate::webview::PendingNavigation {
            url: "file:///link.html".into(),
            user_gesture: true,
        };
        let source = Arc::new(Mutex::new(None));
        proofs.record(&identity, 1, Some(&owner), Some(&source), &nav);
        let retired_source = source;
        let source = Arc::new(Mutex::new(None));
        assert!(!proofs.take_current(&identity, Some(&source), "p", 1, &nav.url));
        drop(retired_source);
        proofs.record(&identity, 1, Some(&owner), Some(&source), &nav);
        proofs.settle_frame(&identity, &[1], &[(1, true)]);
        assert!(!proofs.take_current(&identity, Some(&source), "p", 1, &nav.url));
        proofs.record(&identity, 1, Some(&owner), Some(&source), &nav);
        drop(view);
        assert!(!proofs.take_current(&identity, Some(&source), "p", 1, &nav.url));
    }
}
