//! Workspace / pane / surface / terminal / image panel 접근 헬퍼.
//!
//! 거의 모든 접근자가 `active_workspace` 인덱스 또는 focused pane 의 active tab 을
//! 기준으로 한다. parked 상태(워크스페이스 0개) 에서는 `Option::None` 또는 panic 직전
//! invariant 호출자가 책임.

use tasty_terminal::Terminal;

use super::AppState;
use crate::core::CoreState;

impl AppState {
    /// Invariant: caller must ensure `engine.workspaces` is non-empty.
    /// Parked states (after the last window closes) can have zero workspaces —
    /// such callers must use `engine.workspaces.is_empty()` checks instead.
    pub fn active_workspace<'a>(&self, engine: &'a CoreState) -> &'a crate::model::Workspace {
        debug_assert!(
            !engine.workspaces.is_empty(),
            "active_workspace called with empty workspaces"
        );
        let idx = self
            .active_workspace
            .min(engine.workspaces.len().saturating_sub(1));
        &engine.workspaces[idx]
    }

    pub fn active_workspace_mut<'a>(
        &self,
        engine: &'a mut CoreState,
    ) -> &'a mut crate::model::Workspace {
        debug_assert!(
            !engine.workspaces.is_empty(),
            "active_workspace_mut called with empty workspaces"
        );
        let idx = self
            .active_workspace
            .min(engine.workspaces.len().saturating_sub(1));
        &mut engine.workspaces[idx]
    }

    /// Get the focused pane in the active workspace, or the first pane as fallback.
    /// Returns `None` if no workspaces exist (parked state after last-window close).
    pub fn focused_pane<'a>(&self, engine: &'a CoreState) -> Option<&'a crate::model::Pane> {
        if engine.workspaces.is_empty() {
            return None;
        }
        let ws = self.active_workspace(engine);
        let layout = ws.pane_layout();
        layout
            .find_pane(ws.focused_pane)
            .or_else(|| layout.first_pane())
    }

    /// Get the focused pane (mutable) in the active workspace, or the first pane as fallback.
    /// Returns `None` if no workspaces exist (parked state after last-window close).
    pub fn focused_pane_mut<'a>(
        &self,
        engine: &'a mut CoreState,
    ) -> Option<&'a mut crate::model::Pane> {
        if engine.workspaces.is_empty() {
            return None;
        }
        let ws = self.active_workspace_mut(engine);
        let focused_id = ws.focused_pane;
        // If focused_id is stale, fall back to the first available pane.
        if ws.pane_layout().find_pane(focused_id).is_none() {
            let fallback_id = ws.pane_layout().first_pane().map(|p| p.id);
            if let Some(fid) = fallback_id {
                ws.focused_pane = fid;
            }
        }
        let focused_id = ws.focused_pane;
        ws.pane_layout_mut().find_pane_mut(focused_id)
    }

    /// Get the focused surface ID (the surface that currently receives input).
    pub fn focused_surface_id(&self, engine: &CoreState) -> Option<u32> {
        let pane = self.focused_pane(engine)?;
        let tab = pane.tabs.get(pane.active_tab)?;
        tab.focused_surface_id()
    }

    /// Get the ultimately focused terminal.
    pub fn focused_terminal<'a>(&self, engine: &'a CoreState) -> Option<&'a Terminal> {
        let id = self.focused_surface_id(engine)?;
        engine.terminals.get(id)
    }

    /// Get the ultimately focused terminal (mutable).
    pub fn focused_terminal_mut<'a>(&self, engine: &'a mut CoreState) -> Option<&'a mut Terminal> {
        let id = self.focused_surface_id(engine)?;
        engine.terminals.get_mut(id)
    }

    /// Get the focused image panel (mutable).
    pub fn focused_image_mut<'a>(
        &self,
        engine: &'a mut CoreState,
    ) -> Option<&'a mut crate::model::ImagePanel> {
        let pane = self.focused_pane_mut(engine)?;
        let tab = pane.tabs.get_mut(pane.active_tab)?;
        let focused = tab.focused_surface;
        tab.layout_mut()
            .find_leaf_mut(focused)?
            .as_any_mut()
            .downcast_mut::<crate::model::ImagePanel>()
    }

    /// Find an image panel by its surface ID across all workspaces (mutable).
    /// Used by IPC handlers that target a specific surface — focus-independent.
    pub fn image_panel_mut<'a>(
        &self,
        engine: &'a mut CoreState,
        surface_id: u32,
    ) -> Option<&'a mut crate::model::ImagePanel> {
        let (ws_idx, pid) = engine.find_workspace_index_for_surface(surface_id)?;
        let workspace = engine.workspaces.get_mut(ws_idx)?;
        let pane = workspace.pane_layout_mut().find_pane_mut(pid)?;
        for tab in &mut pane.tabs {
            if tab.contains_surface(surface_id) {
                return tab
                    .layout_mut()
                    .find_leaf_mut(surface_id)?
                    .as_any_mut()
                    .downcast_mut::<crate::model::ImagePanel>();
            }
        }
        None
    }

    /// Get the focused pane ID.
    pub fn focused_pane_id(&self, engine: &CoreState) -> crate::model::PaneId {
        self.active_workspace(engine).focused_pane
    }
}
