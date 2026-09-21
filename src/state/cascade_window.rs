//! [`CascadeWindow`] 의 창 쪽 구현 — 도메인 구조 실행이 부르는 창 연산을 `AppState` 의 같은
//! 이름 메서드로 넘긴다. 메서드마다 한 줄 위임이고 새 동작은 없다. 포트를 도메인이 선언하는
//! 이유는 [`crate::core::cascade_window`] 모듈 문서.

use std::path::PathBuf;

use super::AppState;
use crate::core::CoreState;
use crate::core::cascade_window::CascadeWindow;
#[cfg(feature = "gui")]
use crate::core::host_event::PendingHostEvent;

impl CascadeWindow for AppState {
    fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &CoreState,
        surface_id: u32,
    ) -> Option<PathBuf> {
        AppState::resolve_inherit_cwd_from_surface(self, engine, surface_id)
    }

    fn set_surface_meta(&self, surface_id: u32, key: &str, value: &str) -> std::io::Result<()> {
        self.with_memory(|m| crate::surface_meta::SurfaceMetaStore::set(m, surface_id, key, value))
    }

    fn cleanup_surface_traced(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
        sums: &mut crate::close_trace::CleanupSums,
    ) {
        AppState::cleanup_surface_traced(self, engine, surface_id, persist_id, sums);
    }

    fn fix_workspace_pointers_after_removal(&mut self, removed_idx: usize, remaining: usize) {
        AppState::fix_workspace_pointers_after_removal(self, removed_idx, remaining);
    }

    fn set_active_workspace(&mut self, index: usize) {
        self.active_workspace = index;
    }

    #[cfg(feature = "gui")]
    fn after_workspace_removed(&mut self, workspace_id: u32, path: &'static str) {
        AppState::after_workspace_removed(self, workspace_id, path);
    }

    #[cfg(feature = "gui")]
    fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: Option<&'static str>,
        is_user_close: bool,
    ) {
        AppState::enqueue_surface_closed(self, surface_id, kind, is_user_close);
    }

    #[cfg(feature = "gui")]
    fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        AppState::enqueue_host_event(self, event);
    }

    #[cfg(feature = "gui")]
    fn lifecycle_baseline_insert_tab(
        &mut self,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    ) {
        AppState::lifecycle_baseline_insert_tab(self, tab_id, pane_id, workspace_id, kind);
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
        AppState::lifecycle_baseline_remove_tab(self, tab_id);
    }

    #[cfg(feature = "gui")]
    fn observe_tutorial_surface_split(
        &mut self,
        engine: &CoreState,
        workspace_index: usize,
        pane_id: u32,
        new_surface_id: u32,
    ) {
        AppState::observe_tutorial_surface_split(
            self,
            engine,
            workspace_index,
            pane_id,
            new_surface_id,
        );
    }

    #[cfg(feature = "gui")]
    fn observe_tutorial_pane_split(&mut self, workspace: u32, original: u32, new_pane: u32) {
        AppState::observe_tutorial_pane_split(self, workspace, original, new_pane);
    }
}
