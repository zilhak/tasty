#[cfg(any(feature = "gui", test))]
use serde_json::Value;
#[cfg(all(test, feature = "gui"))]
use serde_json::json;

#[cfg(any(feature = "gui", debug_assertions))]
use tasty_model::TabSwitch;

use super::AppState;
#[cfg(any(feature = "gui", debug_assertions, test))]
use crate::core::CoreState;

impl AppState {
    #[cfg(any(feature = "gui", test))]
    pub fn add_tab(&mut self, engine: &mut CoreState) -> anyhow::Result<()> {
        // mirror에서는 원격 요청만 큐에 넣으며 로컬 탭을 만들지 않는다.
        let mirror_op =
            self.focused_surface_id(engine)
                .map(|sid| crate::ipc::stream::StructuralOp::NewTab {
                    anchor_surface_id: sid,
                    surface_kind: "terminal".to_string(),
                    params: serde_json::Value::Null,
                });
        if self.forward_mirror_structural(engine, mirror_op, Vec::new()) {
            return Ok(());
        }
        let cwd = self.resolve_inherit_cwd(engine);
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);
        let terminal = crate::model::Pane::spawn_terminal(
            surface_id,
            crate::model::ShellSpawnOpts {
                cols,
                rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                extra_env: &sh.envs_ref(),
                waker,
                working_dir: cwd.as_deref(),
            },
        )?;
        engine.terminals.insert(surface_id, terminal);
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.add_terminal_marker_tab(tab_id, surface_id);
        }
        #[cfg(feature = "gui")]
        if let Some(pane) = self.focused_pane(engine) {
            self.observe_tutorial_tab_created(engine, pane.id, tab_id);
        }
        engine.send_fast_init(surface_id);
        engine.mark_layout_dirty();
        Ok(())
    }

    /// kind로 만든 surface를 포커스된 pane에 붙이고 (tab_id, surface_id)를 반환한다.
    #[cfg(any(feature = "gui", test))]
    pub fn add_kind_tab(
        &mut self,
        engine: &mut CoreState,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<(u32, u32)> {
        // 원격 요청을 큐에 넣은 mirror 경로도 여기서는 오류로 반환한다.
        let mirror_op =
            self.focused_surface_id(engine)
                .map(|sid| crate::ipc::stream::StructuralOp::NewTab {
                    anchor_surface_id: sid,
                    surface_kind: kind.to_string(),
                    params: params.clone(),
                });
        if self.forward_mirror_structural(engine, mirror_op, Vec::new()) {
            anyhow::bail!("mirror workspace: structural change forwarded to remote");
        }
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let cwd = self.resolve_inherit_cwd(engine);
        let surface =
            engine.create_surface_via_registry(kind, surface_id, cwd.as_deref(), params)?;
        let name = crate::core::surface_registry::default_tab_name_for_kind(
            kind,
            params,
            engine.surface_registry.get(kind).as_deref(),
        );
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.add_surface_tab(tab_id, name, None, surface);
            engine.mark_layout_dirty();
            Ok((tab_id, surface_id))
        } else {
            anyhow::bail!("no focused pane to add tab to")
        }
    }

    /// 활성 워크스페이스에서 owner_surface_id가 속한 pane에 탭을 추가한다.
    /// 우클릭한 pane이 포커스와 달라도 그 대상을 사용한다.
    #[cfg(any(feature = "gui", test))]
    pub fn add_kind_tab_by_owner(
        &mut self,
        engine: &mut CoreState,
        owner_surface_id: u32,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<(u32, u32)> {
        let ws = self.active_workspace(engine);
        let mut target_pane = None;
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid)
                && pane
                    .tabs
                    .iter()
                    .any(|t| t.contains_surface(owner_surface_id))
            {
                target_pane = Some(pid);
                break;
            }
        }
        let Some(pane_id) = target_pane else {
            anyhow::bail!("owner surface {owner_surface_id} not found in active workspace");
        };
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let cwd = self.resolve_inherit_cwd(engine);
        let surface =
            engine.create_surface_via_registry(kind, surface_id, cwd.as_deref(), params)?;
        let name = crate::core::surface_registry::default_tab_name_for_kind(
            kind,
            params,
            engine.surface_registry.get(kind).as_deref(),
        );
        let ws = self.active_workspace_mut(engine);
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            pane.add_surface_tab(tab_id, name, None, surface);
            engine.mark_layout_dirty();
            Ok((tab_id, surface_id))
        } else {
            anyhow::bail!("pane {pane_id} vanished before add_surface_tab");
        }
    }

    /// 지정 explorer의 root와 현재 폴더를 바꾸고 히스토리·뷰 캐시를 초기화한다.
    #[cfg(any(feature = "gui", test))]
    pub fn set_explorer_cwd(
        &mut self,
        engine: &mut CoreState,
        sid: u32,
        folder: std::path::PathBuf,
    ) {
        let ws = self.active_workspace_mut(engine);
        let pane_ids = ws.pane_layout().all_pane_ids();
        let mut done = false;
        for pid in pane_ids {
            let Some(pane) = ws.pane_layout_mut().find_pane_mut(pid) else {
                continue;
            };
            for tab in pane.tabs.iter_mut() {
                if !tab.contains_surface(sid) {
                    continue;
                }
                if let Some(leaf) = tab.layout_mut().find_leaf_mut(sid)
                    && let Some(ex) = leaf
                        .as_any_mut()
                        .downcast_mut::<crate::model::ExplorerPanel>()
                {
                    ex.active_tab_mut().set_cwd(folder.clone());
                    done = true;
                }
            }
            if done {
                break;
            }
        }
        // engine 차용이 끝난 뒤 GUI 뷰 캐시를 다시 읽도록 한다.
        #[cfg(feature = "gui")]
        if done && let Some(v) = self.explorer_views.get_mut(sid) {
            v.request_reload();
        }
    }

    #[cfg(feature = "gui")]
    pub fn add_empty_tab(&mut self, engine: &mut CoreState) -> Option<(u32, u32)> {
        self.add_kind_tab(engine, "empty", &Value::Null).ok()
    }

    #[cfg(any(feature = "gui", test))]
    pub fn next_tab_in_pane(&mut self, engine: &mut CoreState) {
        #[cfg(feature = "gui")]
        let before = self.tutorial_tab_snapshot(engine);
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.next_tab();
        }
        #[cfg(feature = "gui")]
        self.observe_tutorial_tab_switch(engine, before);
    }

    #[cfg(feature = "gui")]
    pub fn prev_tab_in_pane(&mut self, engine: &mut CoreState) {
        #[cfg(feature = "gui")]
        let before = self.tutorial_tab_snapshot(engine);
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.prev_tab();
        }
        #[cfg(feature = "gui")]
        self.observe_tutorial_tab_switch(engine, before);
    }

    /// 포커스된 pane에서 0-based 인덱스로 탭을 바꾼다. pane 부재와 범위 오류를 구분한다.
    #[cfg(any(feature = "gui", debug_assertions))]
    pub fn goto_tab_in_pane(&mut self, engine: &mut CoreState, index: usize) -> TabSwitch {
        #[cfg(feature = "gui")]
        let before = self.tutorial_tab_snapshot(engine);
        let result = if let Some(pane) = self.focused_pane_mut(engine) {
            pane.goto_tab(index)
        } else {
            TabSwitch::NoPane
        };
        #[cfg(feature = "gui")]
        self.observe_tutorial_tab_switch(engine, before);
        result
    }

    /// 지정한 pane의 탭을 닫는다. 포커스와 무관하게 사본 저장·정리·dirty 갱신을 수행한다.
    #[cfg(feature = "gui")]
    pub fn close_tab(&mut self, engine: &mut CoreState, pane_id: u32, tab_index: usize) -> bool {
        let mirror_op = self
            .active_workspace(engine)
            .pane_layout()
            .find_pane(pane_id)
            .and_then(|p| p.tabs.get(tab_index))
            .and_then(|t| t.focused_surface_id())
            .map(|sid| crate::ipc::stream::StructuralOp::CloseTab {
                anchor_surface_id: sid,
            });
        let candidates = self
            .active_workspace(engine)
            .pane_layout()
            .find_pane(pane_id)
            .map(|pane| AppState::pane_sibling_tab_focus_candidates(pane, tab_index))
            .unwrap_or_default();
        if self.forward_mirror_structural(engine, mirror_op, candidates) {
            return true;
        }
        let in_tab: Vec<u32> = {
            let mut t: Vec<(u32, Option<String>)> = Vec::new();
            if let Some(pane) = self
                .active_workspace(engine)
                .pane_layout()
                .find_pane(pane_id)
                && let Some(tab) = pane.tabs.get(tab_index)
            {
                crate::core::impl_close::collect_close_targets(tab, engine, &mut t);
            }
            t.into_iter().map(|(sid, _)| sid).collect()
        };
        if self.refuse_if_hard_occupied(engine, in_tab) {
            return false;
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        if let Some(pane) = self
            .active_workspace(engine)
            .pane_layout()
            .find_pane(pane_id)
            && let Some(tab) = pane.tabs.get(tab_index)
        {
            crate::core::impl_close::collect_close_targets(tab, engine, &mut targets);
        }
        if let Some(snapshot) = engine.capture_closed_tab(pane_id, tab_index) {
            engine.push_closed_item(snapshot);
        }
        let closed = if let Some(pane) = self
            .active_workspace_mut(engine)
            .pane_layout_mut()
            .find_pane_mut(pane_id)
        {
            pane.close_tab(tab_index)
        } else {
            false
        };
        if closed {
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, true);
            }
            engine.mark_layout_dirty();
        }
        closed
    }

    /// 활성 탭 닫기를 처리한다. mirror 요청을 전달한 경우에도 true다.
    #[cfg(any(feature = "gui", test))]
    pub fn close_active_tab(&mut self, engine: &mut CoreState) -> bool {
        let mirror_op =
            self.focused_surface_id(engine)
                .map(|sid| crate::ipc::stream::StructuralOp::CloseTab {
                    anchor_surface_id: sid,
                });
        let candidates = self
            .focused_pane(engine)
            .map(|pane| AppState::pane_sibling_tab_focus_candidates(pane, pane.active_tab))
            .unwrap_or_default();
        if self.forward_mirror_structural(engine, mirror_op, candidates) {
            return true;
        }
        let in_tab: Vec<u32> = {
            let mut t: Vec<(u32, Option<String>)> = Vec::new();
            if let Some(pane) = self.focused_pane(engine)
                && let Some(tab) = pane.tabs.get(pane.active_tab)
            {
                crate::core::impl_close::collect_close_targets(tab, engine, &mut t);
            }
            t.into_iter().map(|(sid, _)| sid).collect()
        };
        if self.refuse_if_hard_occupied(engine, in_tab) {
            return false;
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let active_slot = self.focused_pane(engine).map(|p| (p.id, p.active_tab));
        if let Some((pane_id, active)) = active_slot
            && let Some(pane) = self.focused_pane(engine)
            && let Some(tab) = pane.tabs.get(active)
        {
            crate::core::impl_close::collect_close_targets(tab, engine, &mut targets);
            if let Some(snapshot) = engine.capture_closed_tab(pane_id, active) {
                engine.push_closed_item(snapshot);
            }
        }
        let closed = if let Some(pane) = self.focused_pane_mut(engine) {
            pane.close_active_tab()
        } else {
            false
        };
        if closed {
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, true);
            }
            engine.mark_layout_dirty();
        }
        closed
    }
}

#[cfg(all(test, feature = "gui"))]
impl AppState {
    /// 시험 준비용 Markdown 탭 생성. 제품 경로는 Intent/Core를 사용한다.
    pub(crate) fn test_add_markdown_tab(
        &mut self,
        engine: &mut CoreState,
        file_path: String,
    ) -> anyhow::Result<()> {
        self.add_kind_tab(engine, "markdown", &json!({"file": file_path}))
            .map(|_| ())
    }

    /// 시험 준비용 surface 교체. 제품 경로는 Core의 ConvertSurface를 사용한다.
    pub(crate) fn test_convert_surface_to_kind(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        kind: &str,
        params: &Value,
    ) -> bool {
        let new_surface = match engine.create_surface_via_registry(kind, surface_id, None, params) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("test_convert_surface_to_kind('{}') failed: {}", kind, e);
                return false;
            }
        };

        let mut location: Option<(usize, u32, usize)> = None;
        'outer: for (ws_idx, workspace) in engine.workspaces.iter().enumerate() {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        if tab.contains_surface(surface_id) {
                            location = Some((ws_idx, pid, tab_idx));
                            break 'outer;
                        }
                    }
                }
            }
        }
        let (ws_idx, pane_id, tab_idx) = match location {
            Some(loc) => loc,
            None => return false,
        };

        let ws = &mut engine.workspaces[ws_idx];
        let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
            Some(p) => p,
            None => return false,
        };
        let tab = &mut pane.tabs[tab_idx];

        if tab.is_split() {
            let replaced = tab.layout_mut().replace_surface(surface_id, new_surface);
            if replaced {
                engine.mark_layout_dirty();
            }
            return replaced;
        }
        tab.put_surface(new_surface);
        tab.explicit_name = None;
        engine.mark_layout_dirty();
        true
    }
}

#[cfg(feature = "gui")]
impl AppState {
    pub(crate) fn observe_tutorial_tab_created(&mut self, engine: &CoreState, pane: u32, tab: u32) {
        if self.tutorial.active.is_none() {
            return;
        }
        if let Some(ws) = engine
            .workspaces
            .iter()
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
        let ws = engine.workspaces.get(self.active_workspace)?;
        let pane = ws.pane_layout().find_pane(ws.focused_pane)?;
        Some((ws.id, pane.id, pane.tabs.get(pane.active_tab)?.id))
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
