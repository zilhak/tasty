//! 활성 워크스페이스·pane·surface 접근. 워크스페이스가 없을 수 있는 호출자는 Option 또는 빈 목록 검사를 사용한다.

#[cfg(feature = "gui")]
use crate::core::engine_access::EngineRef;
#[cfg(feature = "gui")]
use tasty_terminal::Terminal;

use super::RequestContext;
use crate::core::CoreState;

impl RequestContext {
    pub(crate) fn active_workspace_index(&self, engine: &CoreState) -> usize {
        self.navigation.workspace_index(&engine.workspaces())
    }

    pub(crate) fn set_active_workspace_index(&mut self, engine: &CoreState, index: usize) {
        if let Some(ws) = engine.workspace_at(index) {
            self.navigation
                .select_workspace(&engine.workspaces(), ws.id);
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn select_pane(&mut self, engine: &CoreState, pane_id: u32) {
        if let Some(ws) = engine
            .workspaces()
            .into_iter()
            .find(|ws| ws.pane_layout().find_pane(pane_id).is_some())
        {
            self.navigation.select_pane(ws, pane_id);
        }
    }

    /// Retire presentation entries with their structure. Unlike the old fields
    /// on domain objects, these maps do not disappear when the objects drop.
    pub(crate) fn reconcile_presentation(&mut self, engine: &CoreState) {
        self.navigation.reconcile(&engine.workspaces());
        #[cfg(feature = "gui")]
        self.terminal_views.retain(engine);
        self.navigation
            .collapsed_categories
            .retain(|id| engine.categories().iter().any(|c| c.id == *id));
        #[cfg(any(feature = "gui", debug_assertions, test))]
        self.category_last_active.retain(|category, workspace| {
            engine
                .workspaces()
                .into_iter()
                .any(|ws| ws.id == *workspace)
                && engine.categories().iter().any(|c| c.id == *category)
        });
        #[cfg(feature = "gui")]
        self.tab_bar_scroll
            .retain(|id, _| engine.find_pane_by_id(*id).is_some());
    }

    pub(crate) fn apply_structure_result(
        &mut self,
        engine: &CoreState,
        event: &crate::core::intent::CoreEvent,
    ) {
        self.navigation.apply_result(&engine.workspaces(), event);
        self.reconcile_presentation(engine);
    }

    /// Invariant: caller must ensure `engine.workspaces()` is non-empty.
    /// Parked states (after the last window closes) can have zero workspaces —
    /// such callers must use `engine.workspaces().is_empty()` checks instead.
    pub fn active_workspace<'a>(&self, engine: &'a CoreState) -> &'a crate::model::Workspace {
        debug_assert!(
            !engine.workspaces().is_empty(),
            "active_workspace called with empty workspaces"
        );
        let idx = self
            .active_workspace_index(engine)
            .min(engine.workspaces().len().saturating_sub(1));
        engine.workspace_at(idx).expect("workspace index is valid")
    }

    #[cfg(any(feature = "gui", test))]
    pub fn active_workspace_mut<'a>(
        &self,
        engine: &'a mut CoreState,
    ) -> &'a mut crate::model::Workspace {
        debug_assert!(
            !engine.workspaces().is_empty(),
            "active_workspace_mut called with empty workspaces"
        );
        let idx = self
            .active_workspace_index(engine)
            .min(engine.workspaces().len().saturating_sub(1));
        engine
            .workspace_at_mut(idx)
            .expect("workspace index is valid")
    }

    /// Get the focused pane in the active workspace, or the first pane as fallback.
    /// Returns `None` if no workspaces exist (parked state after last-window close).
    pub fn focused_pane<'a>(&self, engine: &'a CoreState) -> Option<&'a crate::model::Pane> {
        if engine.workspaces().is_empty() {
            return None;
        }
        let ws = self.active_workspace(engine);
        let layout = ws.pane_layout();
        layout
            .find_pane(self.navigation.pane_id(ws)?)
            .or_else(|| layout.first_pane())
    }

    /// Get the focused pane (mutable) in the active workspace, or the first pane as fallback.
    /// Returns `None` if no workspaces exist (parked state after last-window close).
    #[cfg(any(feature = "gui", debug_assertions, test))]
    pub fn focused_pane_mut<'a>(
        &self,
        engine: &'a mut CoreState,
    ) -> Option<&'a mut crate::model::Pane> {
        if engine.workspaces().is_empty() {
            return None;
        }
        let ws_id = self.active_workspace_index(engine);
        let pane_id = self.navigation.pane_id(engine.workspace_at_mut(ws_id)?)?;
        engine
            .workspace_at_mut(ws_id)?
            .pane_layout_mut()
            .find_pane_mut(pane_id)
    }

    pub fn focused_surface_id(&self, engine: &CoreState) -> Option<u32> {
        let pane = self.focused_pane(engine)?;
        let tab = pane.tabs.get(self.navigation.tab_index(pane))?;
        self.navigation.surface_id(tab)
    }

    #[cfg(feature = "gui")]
    pub fn focused_terminal<'a>(&self, engine: &EngineRef<'a>) -> Option<&'a Terminal> {
        let id = self.focused_surface_id(engine)?;
        engine.runtime.terminals.get(id)
    }

    pub fn focused_pane_id(&self, engine: &CoreState) -> crate::model::PaneId {
        self.navigation
            .pane_id(self.active_workspace(engine))
            .unwrap_or(0)
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
                    SwitchTarget::Tab if !engine.workspaces().is_empty() => {
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
