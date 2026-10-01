//! App adapter의 결과 처리를 View 또는 headless 명령 문맥에 연결한다.

use crate::runtime::engine_access::EngineRef;
use std::path::PathBuf;

use super::RequestContext;
use crate::app::structure_context::CascadeWindow;
use crate::core::CoreState;
#[cfg(feature = "gui")]
use crate::core::host_event::PendingHostEvent;

impl CascadeWindow for RequestContext {
    fn apply_structure_result(
        &mut self,
        engine: &CoreState,
        event: &crate::app::command::CoreEvent,
    ) {
        RequestContext::apply_structure_result(self, engine, event);
    }
    fn select_surface_result(&mut self, engine: &CoreState, surface: u32) {
        for ws in &engine.workspaces() {
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id)
                    && let Some(tab) = pane.tabs.iter().find(|t| t.contains_surface(surface))
                {
                    self.navigation.select_surface(tab, surface);
                    return;
                }
            }
        }
    }
    fn select_tab_result(&mut self, engine: &CoreState, pane: u32, tab: u32) {
        if let Some(pane) = engine.find_pane_by_id(pane) {
            self.navigation.select_tab(pane, tab);
        }
    }
    fn select_pane_result(&mut self, engine: &CoreState, pane: u32) {
        if let Some(ws) = engine
            .workspaces()
            .into_iter()
            .find(|ws| ws.pane_layout().find_pane(pane).is_some())
        {
            self.navigation.select_pane(ws, pane);
        }
    }
    fn presentation(&self) -> &dyn crate::model::StructurePresentation {
        &self.navigation
    }

    fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &EngineRef<'_>,
        surface_id: u32,
    ) -> Option<PathBuf> {
        RequestContext::resolve_inherit_cwd_from_surface(self, engine, surface_id)
    }

    fn set_surface_meta(&self, surface_id: u32, key: &str, value: &str) -> std::io::Result<()> {
        self.with_memory(|m| crate::surface_meta::SurfaceMetaStore::set(m, surface_id, key, value))
    }

    fn reconcile_presentation(&mut self, engine: &CoreState) {
        RequestContext::reconcile_presentation(self, engine);
    }

    fn set_active_workspace(&mut self, engine: &CoreState, index: usize) {
        self.set_active_workspace_index(engine, index);
    }

    #[cfg(feature = "gui")]
    fn release_surface_views(&mut self, surface_id: u32) {
        RequestContext::release_surface_views(self, surface_id);
    }

    #[cfg(feature = "gui")]
    fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: Option<&'static str>,
        is_user_close: bool,
    ) {
        RequestContext::enqueue_surface_closed(self, surface_id, kind, is_user_close);
    }

    #[cfg(feature = "gui")]
    fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        RequestContext::enqueue_host_event(self, event);
    }

    #[cfg(feature = "gui")]
    fn lifecycle_baseline_insert_tab(
        &mut self,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    ) {
        RequestContext::lifecycle_baseline_insert_tab(self, tab_id, pane_id, workspace_id, kind);
    }

    #[cfg(feature = "gui")]
    fn lifecycle_baseline_pane_of(&self, tab_id: u32) -> Option<u32> {
        self.last_tab_locations
            .as_ref()
            .and_then(|m| m.get(&tab_id))
            .map(|(p, _, _)| *p)
    }

    #[cfg(feature = "gui")]
    fn lifecycle_baseline_remove_tab(&mut self, tab_id: u32) {
        RequestContext::lifecycle_baseline_remove_tab(self, tab_id);
    }

    #[cfg(feature = "gui")]
    fn observe_tutorial_surface_split(
        &mut self,
        engine: &CoreState,
        workspace_index: usize,
        pane_id: u32,
        new_surface_id: u32,
    ) {
        RequestContext::observe_tutorial_surface_split(
            self,
            engine,
            workspace_index,
            pane_id,
            new_surface_id,
        );
    }

    #[cfg(feature = "gui")]
    fn observe_tutorial_pane_split(&mut self, workspace: u32, original: u32, new_pane: u32) {
        RequestContext::observe_tutorial_pane_split(self, workspace, original, new_pane);
    }
}
