//! 활성 워크스페이스·pane·surface 접근. 워크스페이스가 없을 수 있는 호출자는 Option 또는 빈 목록 검사를 사용한다.

#[cfg(feature = "gui")]
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

    #[cfg(any(feature = "gui", debug_assertions, test))]
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
    #[cfg(any(feature = "gui", debug_assertions, test))]
    pub fn focused_pane_mut<'a>(
        &self,
        engine: &'a mut CoreState,
    ) -> Option<&'a mut crate::model::Pane> {
        if engine.workspaces.is_empty() {
            return None;
        }
        let ws = self.active_workspace_mut(engine);
        let focused_id = ws.focused_pane;
        if ws.pane_layout().find_pane(focused_id).is_none() {
            let fallback_id = ws.pane_layout().first_pane().map(|p| p.id);
            if let Some(fid) = fallback_id {
                ws.focused_pane = fid;
            }
        }
        let focused_id = ws.focused_pane;
        ws.pane_layout_mut().find_pane_mut(focused_id)
    }

    pub fn focused_surface_id(&self, engine: &CoreState) -> Option<u32> {
        let pane = self.focused_pane(engine)?;
        let tab = pane.tabs.get(pane.active_tab)?;
        tab.focused_surface_id()
    }

    #[cfg(feature = "gui")]
    pub fn focused_terminal<'a>(&self, engine: &'a CoreState) -> Option<&'a Terminal> {
        let id = self.focused_surface_id(engine)?;
        engine.terminals.get(id)
    }

    #[cfg(feature = "gui")]
    pub fn focused_terminal_mut<'a>(&self, engine: &'a mut CoreState) -> Option<&'a mut Terminal> {
        let id = self.focused_surface_id(engine)?;
        engine.terminals.get_mut(id)
    }

    #[cfg(feature = "gui")]
    pub fn focused_pane_id(&self, engine: &CoreState) -> crate::model::PaneId {
        self.active_workspace(engine).focused_pane
    }

    #[cfg(feature = "gui")]
    pub(crate) fn switch_overlay(
        &self,
    ) -> Option<crate::adapters::ui::switch_overlay::SwitchOverlayState> {
        self.switch_overlay
    }

    /// 수식키에 맞는 숫자 전환 안내를 갱신하고 변경 여부를 반환한다.
    /// ctrl/shift/alt는 플랫폼별 정규화를 마친 값이어야 한다.
    #[cfg(feature = "gui")]
    pub(crate) fn update_switch_overlay(
        &mut self,
        engine: &CoreState,
        kb: &crate::settings::KeybindingSettings,
        ctrl: bool,
        shift: bool,
        alt: bool,
        option: bool,
    ) -> bool {
        use crate::adapters::ui::switch_overlay::{
            SwitchOverlayState, SwitchTarget, switch_target_for,
        };
        let next = switch_target_for(kb, ctrl, shift, alt, option)
            .filter(|t| {
                *t != SwitchTarget::Category || engine.settings.general.workspace_categories_enabled
            })
            .map(|target| {
                let pane_id = match target {
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

    /// 숫자 전환 안내를 지우고 변경 여부를 반환한다.
    #[cfg(feature = "gui")]
    pub(crate) fn clear_switch_overlay(&mut self) -> bool {
        let changed = self.switch_overlay.is_some();
        self.switch_overlay = None;
        changed
    }
}
