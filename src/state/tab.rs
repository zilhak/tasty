#[cfg(feature = "gui")]
use crate::runtime::engine_read::EngineRead;
#[cfg(feature = "gui")]
use serde_json::Value;

#[cfg(any(feature = "gui", debug_assertions))]
use tasty_model::TabSwitch;

use super::RequestContext;
#[cfg(any(feature = "gui", debug_assertions, test))]
use crate::core::CoreState;

impl RequestContext {
    /// Resolve the menu owner once; App admits and executes the explicit creation command.
    #[cfg(feature = "gui")]
    pub fn add_kind_tab_by_owner(
        &mut self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        owner_surface_id: u32,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<()> {
        let (_, pane_id) = engine
            .find_workspace_index_for_surface(owner_surface_id)
            .ok_or_else(|| anyhow::anyhow!("owner surface {owner_surface_id} not found"))?;
        let cwd = self.resolve_inherit_cwd(engine);
        self.dispatch_intent(
            crate::app::command::DomainIntent::CreateTab {
                pane_id,
                cwd,
                kind: kind.into(),
                name: None,
                surface_params: params.clone(),
                activate: true,
            }
            .from_user_context_menu(),
        );
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub fn set_explorer_cwd(
        &mut self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        sid: u32,
        folder: std::path::PathBuf,
    ) {
        let Some(target) = crate::runtime::surface_binding::SurfaceBinding::capture(engine, sid)
        else {
            return;
        };
        self.dispatch_intent(
            crate::intent::Intent::Engine(crate::app::engine_action::EngineAction::ExplorerCwd {
                target,
                folder,
            })
            .from_user_context_menu(),
        );
        #[cfg(feature = "gui")]
        if let Some(view) = self.explorer_views.get_mut(sid) {
            view.request_reload();
        }
    }

    #[cfg(feature = "gui")]
    pub fn next_tab_in_pane(&mut self, engine: &CoreState) {
        #[cfg(feature = "gui")]
        let before = self.tutorial_tab_snapshot(engine);
        if let Some(pane) = self.focused_pane(engine) {
            let index = (self.navigation.tab_index(pane) + 1) % pane.tabs.len().max(1);
            self.navigation.goto_tab(pane, index);
        }
        #[cfg(feature = "gui")]
        self.observe_tutorial_tab_switch(engine, before);
    }

    #[cfg(feature = "gui")]
    pub fn prev_tab_in_pane(&mut self, engine: &CoreState) {
        #[cfg(feature = "gui")]
        let before = self.tutorial_tab_snapshot(engine);
        if let Some(pane) = self.focused_pane(engine) {
            let len = pane.tabs.len().max(1);
            let index = (self.navigation.tab_index(pane) + len - 1) % len;
            self.navigation.goto_tab(pane, index);
        }
        #[cfg(feature = "gui")]
        self.observe_tutorial_tab_switch(engine, before);
    }

    /// 포커스된 pane에서 0-based 인덱스로 탭을 바꾼다. pane 부재와 범위 오류를 구분한다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub fn goto_tab_in_pane(&mut self, engine: &CoreState, index: usize) -> TabSwitch {
        #[cfg(feature = "gui")]
        let before = self.tutorial_tab_snapshot(engine);
        let result = if let Some(pane) = self.focused_pane(engine) {
            self.navigation.goto_tab(pane, index)
        } else {
            TabSwitch::NoPane
        };
        #[cfg(feature = "gui")]
        self.observe_tutorial_tab_switch(engine, before);
        result
    }

    /// 지정한 pane의 탭을 닫는다. 포커스와 무관하게 사본 저장·정리·dirty 갱신을 수행한다.
    #[cfg(feature = "gui")]
    pub fn close_tab(&mut self, engine: &EngineRead<'_>, pane_id: u32, tab_index: usize) -> bool {
        let mirror_op = self
            .active_workspace(engine)
            .pane_layout()
            .find_pane(pane_id)
            .and_then(|p| p.tabs.get(tab_index))
            .and_then(|t| self.navigation.surface_id(t))
            .map(|sid| crate::ipc::stream::StructuralOp::CloseTab {
                anchor_surface_id: sid,
            });
        let candidates = self
            .active_workspace(engine)
            .pane_layout()
            .find_pane(pane_id)
            .map(|pane| self.pane_sibling_tab_focus_candidates(pane, tab_index))
            .unwrap_or_default();
        if self.forward_mirror_structural(engine, mirror_op, candidates) {
            return true;
        }
        let in_tab: Vec<u32> = {
            let mut t = Vec::new();
            if let Some(pane) = self
                .active_workspace(engine)
                .pane_layout()
                .find_pane(pane_id)
                && let Some(tab) = pane.tabs.get(tab_index)
            {
                t.extend(tab.all_surface_ids());
            }
            t
        };
        if self.refuse_if_hard_occupied(engine, in_tab) {
            return false;
        }
        let Some(tab_id) = self
            .active_workspace(engine)
            .pane_layout()
            .find_pane(pane_id)
            .and_then(|pane| pane.tabs.get(tab_index))
            .map(|tab| tab.id)
        else {
            return false;
        };
        self.close_tab_through_core(engine, tab_id)
    }

    /// AppServices 탭 닫기로 트리를 바꾸고 복원 기록을 남긴 뒤 창 쪽 정리와 알림을 이어서 한다.
    #[cfg(feature = "gui")]
    fn close_tab_through_core(&mut self, engine: &EngineRead<'_>, tab_id: u32) -> bool {
        if engine.find_pane_for_tab(tab_id).is_none() {
            return false;
        }
        self.dispatch_intent(
            crate::app::command::DomainIntent::CloseTab { tab_id }.from_user_context_menu(),
        );
        true
    }

    /// 활성 탭 닫기를 처리한다. mirror 요청을 전달한 경우에도 true다.
    #[cfg(feature = "gui")]
    pub fn close_active_tab(&mut self, engine: &EngineRead<'_>) -> bool {
        let mirror_op =
            self.focused_surface_id(engine)
                .map(|sid| crate::ipc::stream::StructuralOp::CloseTab {
                    anchor_surface_id: sid,
                });
        let candidates = self
            .focused_pane(engine)
            .map(|pane| {
                self.pane_sibling_tab_focus_candidates(pane, self.navigation.tab_index(pane))
            })
            .unwrap_or_default();
        if self.forward_mirror_structural(engine, mirror_op, candidates) {
            return true;
        }
        let in_tab: Vec<u32> = {
            let mut t = Vec::new();
            if let Some(pane) = self.focused_pane(engine)
                && let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane))
            {
                t.extend(tab.all_surface_ids());
            }
            t
        };
        if self.refuse_if_hard_occupied(engine, in_tab) {
            return false;
        }
        let Some(tab_id) = self
            .focused_pane(engine)
            .and_then(|pane| pane.tabs.get(self.navigation.tab_index(pane)))
            .map(|tab| tab.id)
        else {
            return false;
        };
        self.close_tab_through_core(engine, tab_id)
    }
}

#[cfg(feature = "gui")]
impl RequestContext {
    pub(crate) fn observe_tutorial_tab_created(&mut self, engine: &CoreState, pane: u32, tab: u32) {
        if self.tutorial.active.is_none() {
            return;
        }
        if let Some(ws) = engine
            .workspaces()
            .into_iter()
            .find(|w| w.pane_layout().find_pane(pane).is_some())
        {
            self.tutorial
                .observe(crate::adapters::ui::tutorial::PracticeEvent::NewTab {
                    workspace: ws.id,
                    pane,
                    tab,
                });
        }
    }
    pub(crate) fn tutorial_tab_snapshot(&self, engine: &CoreState) -> Option<(u32, u32, u32)> {
        let ws = engine.workspace_at(self.active_workspace_index(engine))?;
        let pane = ws
            .pane_layout()
            .find_pane(self.navigation.pane_id(ws).unwrap_or(0))?;
        Some((
            ws.id,
            pane.id,
            pane.tabs.get(self.navigation.tab_index(pane))?.id,
        ))
    }
    pub(crate) fn observe_tutorial_tab_switch(
        &mut self,
        engine: &CoreState,
        before: Option<(u32, u32, u32)>,
    ) {
        if let (Some((workspace, pane, from)), Some((ws, p, to))) =
            (before, self.tutorial_tab_snapshot(engine))
        {
            if workspace == ws && pane == p {
                self.tutorial
                    .observe(crate::adapters::ui::tutorial::PracticeEvent::SwitchTab {
                        workspace,
                        pane,
                        from,
                        to,
                    });
            }
        }
    }
}
