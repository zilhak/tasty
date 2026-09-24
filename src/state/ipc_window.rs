//! IPC의 IpcWindow 요청을 AppState의 창 연산에 연결한다.

use std::path::PathBuf;

use super::AppState;
use crate::adapters::ipc::window_port::{IntentOutbox, IpcWindow};
use crate::core::CoreState;

impl IpcWindow for AppState {
    fn active_workspace_index(&self) -> usize {
        self.active_workspace
    }

    fn resolve_inherit_cwd(&self, engine: &CoreState) -> Option<PathBuf> {
        AppState::resolve_inherit_cwd(self, engine)
    }

    fn cascade_workspace_created(
        &mut self,
        engine: &mut CoreState,
        origin: &crate::intent::IntentOrigin,
        window_id: u64,
        created: crate::app::dispatch_domain::WorkspaceCreatedCascade,
    ) {
        crate::app::dispatch_domain::cascade_workspace_created(
            self, engine, origin, window_id, created,
        );
    }

    fn cascade_workspace_meta_updated(
        &mut self,
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    ) {
        crate::app::dispatch_domain::cascade_workspace_meta_updated(
            self,
            workspace_id,
            name,
            subtitle,
            description,
        );
    }

    fn close_workspace_at(
        &mut self,
        engine: &mut CoreState,
        ws_idx: usize,
        origin: super::WorkspaceCloseOrigin,
    ) -> bool {
        AppState::close_workspace_at(self, engine, ws_idx, origin)
    }

    fn fix_workspace_pointers_after_move(&mut self, from: usize, to: usize) {
        crate::app::dispatch_domain::cascade_workspace_moved(self, from, to);
    }

    fn push_host_event(&mut self, event: super::PendingHostEvent) {
        AppState::enqueue_host_event(self, event);
    }

    fn recent_files(&self, kind: &str) -> Vec<String> {
        self.recent_files.get(kind)
    }

    fn apply_preset(
        &mut self,
        core: &crate::core::Core,
        engine: &mut CoreState,
        target: crate::intent::preset::PresetApplyTarget,
        options: super::preset_apply::ApplyOptions,
    ) -> Result<crate::intent::preset::ApplyOutcome, crate::intent::preset::PresetMutationError>
    {
        crate::intent::preset::apply_inner(core, self, engine, target, options)
    }

    #[cfg(feature = "gui")]
    fn enqueue_approval_popup(
        &mut self,
        engine: &mut CoreState,
        record: &tasty_approval::ApprovalRecord,
    ) {
        crate::adapters::ui::popup::approval::enqueue_approval(self, engine, record);
    }

    #[cfg(feature = "gui")]
    fn plugin_popup_user_activated(&self, plugin_id: &str, instance_id: u64) -> bool {
        self.plugin_popup_user_activated
            .get(&instance_id)
            .is_some_and(|owner| owner == plugin_id)
    }

    #[cfg(feature = "gui")]
    fn take_webview_user_navigation(
        &mut self,
        plugin_id: &str,
        surface_id: u32,
        url: &str,
    ) -> bool {
        crate::plugin_bridge::user_navigation::take(
            &mut self.webview_user_navigations,
            plugin_id,
            surface_id,
            url,
        )
    }

    fn enqueue_intents(&mut self, intents: IntentOutbox) {
        for intent in intents.into_vec() {
            self.dispatch_intent(intent);
        }
    }
}
