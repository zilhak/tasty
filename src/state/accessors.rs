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

    /// 현재 switch-number overlay 스냅샷. draw 경로(04 탭 `draw_pane_tab_bars` / 05 사이드바)가
    /// 매 프레임 읽어 숫자 키캡 오버레이를 표시할 focused pane / 대상을 판단한다.
    #[cfg(feature = "gui")]
    pub(crate) fn switch_overlay(
        &self,
    ) -> Option<crate::adapters::ui::switch_overlay::SwitchOverlayState> {
        self.switch_overlay
    }

    /// 현재 눌린 modifier 로 switch-number overlay 스냅샷을 다시 계산해 저장한다.
    /// `ModifiersChanged` 마다 호출. 스냅샷이 실제로 바뀌었으면 `true` (호출측이
    /// 그때 `mark_dirty()` 한다 — modifier press/release 시 키캡이 즉시 뜨고 사라지게).
    ///
    /// `ctrl`/`shift`/`alt` 는 플랫폼 정규화가 끝난 값을 받는다(numeric.rs 와 동일).
    #[cfg(feature = "gui")]
    pub(crate) fn update_switch_overlay(
        &mut self,
        engine: &CoreState,
        kb: &crate::settings::KeybindingSettings,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        use crate::adapters::ui::switch_overlay::{
            SwitchOverlayState, SwitchTarget, switch_target_for,
        };
        let next = switch_target_for(kb, ctrl, shift, alt).map(|target| {
            let pane_id = match target {
                // parked 상태(워크스페이스 0개)에서는 focused pane 이 없으므로 None.
                SwitchTarget::Tab if !engine.workspaces.is_empty() => {
                    Some(self.focused_pane_id(engine))
                }
                _ => None,
            };
            SwitchOverlayState { target, pane_id }
        });
        let changed = next != self.switch_overlay;
        self.switch_overlay = next;
        changed
    }

    /// switch-number overlay 스냅샷을 비운다(창 비활성/포커스 상실 시). 실제로 비워졌으면
    /// `true`.
    #[cfg(feature = "gui")]
    pub(crate) fn clear_switch_overlay(&mut self) -> bool {
        let changed = self.switch_overlay.is_some();
        self.switch_overlay = None;
        changed
    }
}
