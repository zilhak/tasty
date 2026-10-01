//! One synchronous request's fixed presentation and outputs, independent of a View's execution APIs.
use super::window_port::{IntentOutbox, IpcWindow};
use crate::{core::CoreState, model::StructurePresentation, runtime::engine_access::EngineRef};
pub(crate) struct RequestScope<'a> {
    presentation: &'a dyn StructurePresentation,
    workspace: Option<u32>,
    surface: Option<u32>,
    recent: &'a crate::recent_files::RecentFiles,
    intents: Vec<crate::intent::DispatchedIntent>,
    #[cfg(feature = "gui")]
    approvals: Vec<tasty_approval::ApprovalRecord>,
    #[cfg(feature = "gui")]
    popup_proofs: &'a std::collections::HashMap<u64, String>,
    #[cfg(feature = "gui")]
    navigation_proofs: Option<std::sync::Arc<crate::app::html_runtime::NavigationProofs>>,
    #[cfg(feature = "gui")]
    view: std::sync::Weak<()>,
}
impl<'a> RequestScope<'a> {
    pub(crate) fn capture(
        state: &'a mut crate::state::RequestContext,
        engine: &CoreState,
        #[cfg(feature = "gui")] navigation_proofs: Option<
            std::sync::Arc<crate::app::html_runtime::NavigationProofs>,
        >,
    ) -> Self {
        Self {
            presentation: &state.navigation,
            workspace: engine
                .workspace_at(state.active_workspace_index(engine))
                .map(|workspace| workspace.id),
            surface: state.focused_surface_id(engine),
            recent: &state.recent_files,
            intents: Vec::new(),
            #[cfg(feature = "gui")]
            approvals: Vec::new(),
            #[cfg(feature = "gui")]
            popup_proofs: &state.plugin_popup_user_activated,
            #[cfg(feature = "gui")]
            navigation_proofs,
            #[cfg(feature = "gui")]
            view: state.webview_identity.clone(),
        }
    }
    pub(crate) fn finish(self) -> RequestOutputs {
        RequestOutputs {
            intents: self.intents,
            #[cfg(feature = "gui")]
            approvals: self.approvals,
        }
    }
}
pub(crate) struct RequestOutputs {
    intents: Vec<crate::intent::DispatchedIntent>,
    #[cfg(feature = "gui")]
    approvals: Vec<tasty_approval::ApprovalRecord>,
}
impl RequestOutputs {
    pub(crate) fn apply(self, state: &mut crate::state::RequestContext, engine: &CoreState) {
        state.pending_intents.extend(self.intents);
        #[cfg(feature = "gui")]
        for record in self.approvals {
            crate::adapters::ui::popup::approval::enqueue_approval(state, engine, &record);
        }
    }
}
impl IpcWindow for RequestScope<'_> {
    fn presentation(&self) -> &dyn StructurePresentation {
        self.presentation
    }
    fn active_workspace_index(&self, engine: &CoreState) -> usize {
        self.workspace
            .and_then(|id| {
                engine
                    .workspaces()
                    .into_iter()
                    .position(|workspace| workspace.id == id)
            })
            .unwrap_or(0)
    }
    fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &EngineRef<'_>,
        surface: u32,
    ) -> Option<std::path::PathBuf> {
        engine
            .runtime
            .settings
            .general
            .inherit_cwd
            .then(|| engine.local_surface_cwd(surface))
            .flatten()
    }
    fn resolve_inherit_cwd(&self, engine: &EngineRef<'_>) -> Option<std::path::PathBuf> {
        self.resolve_inherit_cwd_from_surface(engine, self.surface?)
    }
    fn recent_files(&self, kind: &str) -> Vec<String> {
        self.recent.get(kind)
    }
    fn enqueue_intents(&mut self, intents: IntentOutbox) {
        self.intents.extend(intents.into_vec());
    }
    #[cfg(feature = "gui")]
    fn enqueue_approval_popup(
        &mut self,
        _engine: &CoreState,
        record: &tasty_approval::ApprovalRecord,
    ) {
        self.approvals.push(record.clone());
    }
    #[cfg(feature = "gui")]
    fn plugin_popup_user_activated(&self, plugin: &str, instance: u64) -> bool {
        self.popup_proofs
            .get(&instance)
            .is_some_and(|owner| owner == plugin)
    }
    #[cfg(feature = "gui")]
    fn take_webview_user_navigation(
        &mut self,
        engine: &EngineRef<'_>,
        plugin: &str,
        surface: u32,
        url: &str,
    ) -> bool {
        self.navigation_proofs
            .as_ref()
            .is_some_and(|proofs| proofs.take(&self.view, engine, plugin, surface, url))
    }
}
