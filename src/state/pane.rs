use crate::core::CoreState;
#[cfg(test)]
use crate::model::SplitDirection;

use super::AppState;

fn terminal_surface_in_tab(
    tab: &crate::model::Tab,
    surface_id: u32,
) -> Option<&crate::model::TerminalSurface> {
    tab.layout_opt
        .as_ref()?
        .find_surface(surface_id)?
        .as_any()
        .downcast_ref::<crate::model::TerminalSurface>()
}

impl AppState {
    /// 닫을 대상 중 hard 점유된 surface가 있으면 true를 반환해 요청을 거절한다.
    /// 종료된 PTY의 사후 정리에는 적용하지 않는다. 그 경로까지 막으면 surface가 남는다.
    /// 로컬 사용자는 점유 해제 버튼으로 먼저 연결을 끊을 수 있다.
    pub(crate) fn refuse_if_hard_occupied(
        &mut self,
        engine: &CoreState,
        targets: impl IntoIterator<Item = u32>,
    ) -> bool {
        let Some(_occupied) = targets
            .into_iter()
            .find(|sid| engine.attach.is_hard_occupied(*sid))
        else {
            return false;
        };
        #[cfg(feature = "gui")]
        self.toasts.push(
            crate::i18n::t("attach.toast.close_blocked_hard_occupied"),
            crate::model::toast_kind::ToastKind::Warning,
            crate::model::toast_kind::ToastScope::Window,
        );
        true
    }

    /// mirror의 구조 변경을 원격 큐에 넣고 true를 반환해 로컬 실행을 멈춘다.
    /// op를 만들 수 없으면 전달하지 않고 차단 토스트를 표시한다.
    /// mirror 워크스페이스 자체를 닫는 동작은 이 함수를 거치지 않는다.
    ///
    /// GUI 사용자 요청으로 표시하며, 닫기 요청은 변경 전 트리에서 구한 포커스 후보를 받는다.
    /// 새 탭·분할·이동처럼 닫기가 아닌 요청에는 빈 후보 목록을 넘긴다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn forward_mirror_structural(
        &mut self,
        engine: &mut CoreState,
        op: Option<crate::ipc::stream::StructuralOp>,
        close_focus_candidates: Vec<u32>,
    ) -> bool {
        if !self.active_workspace(engine).mirror {
            return false;
        }
        match op {
            Some(op) => {
                engine
                    .pending_structural_forward
                    .push(crate::core::PendingStructuralForward {
                        op,
                        user_triggered: true,
                        close_focus_candidates,
                        silent_failure: false,
                    });
            }
            None => {
                #[cfg(feature = "gui")]
                self.toasts.push(
                    crate::i18n::t("attach.toast.mirror_structural_blocked"),
                    crate::model::toast_kind::ToastKind::Warning,
                    crate::model::toast_kind::ToastScope::Window,
                );
            }
        }
        true
    }

    /// 닫을 탭의 다음 탭, 마지막이면 이전 탭을 우선하는 로컬 포커스 후보다.
    /// 나머지 탭도 순서대로 포함하며 하나뿐인 탭에는 후보가 없다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn pane_sibling_tab_focus_candidates(
        pane: &crate::model::Pane,
        closing_tab_index: usize,
    ) -> Vec<u32> {
        let n = pane.tabs.len();
        if n <= 1 {
            return Vec::new();
        }
        let primary = if closing_tab_index + 1 < n {
            closing_tab_index + 1
        } else {
            closing_tab_index.wrapping_sub(1)
        };
        let mut out = Vec::new();
        if let Some(sid) = pane.tabs.get(primary).and_then(|t| t.focused_surface_id()) {
            out.push(sid);
        }
        for (idx, tab) in pane.tabs.iter().enumerate() {
            if idx == closing_tab_index || idx == primary {
                continue;
            }
            if let Some(sid) = tab.focused_surface_id() {
                out.push(sid);
            }
        }
        out
    }

    /// 같은 분할 탭의 다른 surface를 순서대로 후보에 넣는다.
    /// 단일 surface 탭이면 pane_sibling_tab_focus_candidates에 위임한다.
    #[cfg(any(feature = "gui", test))]
    fn active_surface_close_focus_candidates(
        &self,
        engine: &CoreState,
        surface_id: u32,
    ) -> Vec<u32> {
        let ws = self.active_workspace(engine);
        let Some(pane) = ws.pane_layout().find_pane(ws.focused_pane) else {
            return Vec::new();
        };
        let tab_index = pane.active_tab;
        let Some(tab) = pane.tabs.get(tab_index) else {
            return Vec::new();
        };
        if tab.is_split() {
            tab.layout_opt
                .as_ref()
                .map(|l| l.all_surface_ids())
                .unwrap_or_default()
                .into_iter()
                .filter(|&sid| sid != surface_id)
                .collect()
        } else {
            Self::pane_sibling_tab_focus_candidates(pane, tab_index)
        }
    }

    /// 포커스된 pane 닫기를 처리한다. mirror 요청을 전달한 경우에도 true다.
    #[cfg(any(feature = "gui", test))]
    pub fn close_active_pane(&mut self, engine: &mut CoreState) -> bool {
        let mirror_op = self.focused_surface_id(engine).map(|sid| {
            crate::ipc::stream::StructuralOp::ClosePane {
                anchor_surface_id: sid,
            }
        });
        // pane 닫기는 같은 pane 안의 인접 포커스 후보를 사용하지 않는다.
        if self.forward_mirror_structural(engine, mirror_op, Vec::new()) {
            return true;
        }
        let target_id = self.active_workspace(engine).focused_pane;
        let in_pane: Vec<u32> = {
            let ws = self.active_workspace(engine);
            ws.pane_layout()
                .find_pane(target_id)
                .map(|pane| {
                    let mut t: Vec<(u32, Option<String>)> = Vec::new();
                    for tab in &pane.tabs {
                        crate::core::impl_close::collect_close_targets(tab, engine, &mut t);
                    }
                    t.into_iter().map(|(sid, _)| sid).collect()
                })
                .unwrap_or_default()
        };
        if self.refuse_if_hard_occupied(engine, in_pane) {
            return false;
        }

        // 제거 후에는 부모 Split 정보를 잃으므로 트리 변경 전에 복원 사본을 만든다.
        if let Some(item) = engine.capture_closed_pane(target_id) {
            engine.push_closed_item(item);
        }

        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = self.active_workspace(engine);
            if let Some(pane) = ws.pane_layout().find_pane(target_id) {
                for tab in &pane.tabs {
                    crate::core::impl_close::collect_close_targets(tab, engine, &mut targets);
                }
            }
        }

        let ws = self.active_workspace_mut(engine);
        let removed = ws.pane_layout_mut().close_pane(target_id);
        if removed {
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, true);
            }
            engine.mark_layout_dirty();
        }
        removed
    }

    /// 포커스된 surface를 닫고 필요하면 빈 탭·pane·워크스페이스도 정리한다.
    #[cfg(any(feature = "gui", test))]
    pub fn close_active_surface(&mut self, engine: &mut CoreState) -> bool {
        let focused_sid = self.focused_surface_id(engine);
        let mirror_op = focused_sid
            .map(|sid| crate::ipc::stream::StructuralOp::CloseSurface { surface_id: sid });
        let candidates = focused_sid
            .map(|sid| self.active_surface_close_focus_candidates(engine, sid))
            .unwrap_or_default();
        if self.forward_mirror_structural(engine, mirror_op, candidates) {
            return true;
        }
        if self.refuse_if_hard_occupied(engine, focused_sid) {
            return false;
        }
        let surface_id;
        // 탭을 변경하기 전에 복원할 이름을 보관한다.
        let mut tab_name_for_snapshot: Option<String> = None;
        if let Some(pane) = self.focused_pane(engine) {
            let tab = match pane.tabs.get(pane.active_tab) {
                Some(t) => t,
                None => return false,
            };
            surface_id = tab.focused_surface;
            if tab.is_split() && terminal_surface_in_tab(tab, surface_id).is_some() {
                tab_name_for_snapshot = Some(tab.display_name().to_string());
            }
        } else {
            return false;
        }
        let persist_id = engine
            .terminals
            .scrollback_persist_id(surface_id)
            .map(str::to_string);
        let kind = self.surface_kind(engine, surface_id);
        // surface를 제거하기 전에 복원 사본을 완성한다.
        let split_snapshot =
            tab_name_for_snapshot.map(|tab_name| crate::model::ClosedItem::Surface {
                surface: crate::model::closed_item::ClosedSurface::from_surface_id(
                    surface_id,
                    engine.terminals.get(surface_id),
                ),
                tab_name,
            });
        let split_handled;
        if let Some(pane) = self.focused_pane_mut(engine) {
            let tab = match pane.active_tab_mut() {
                Some(t) => t,
                None => return false,
            };
            if tab.is_split() {
                if !tab.close_surface(surface_id) {
                    return self.close_surface_by_id(engine, surface_id, true);
                }
                split_handled = true;
            } else {
                return self.close_surface_by_id(engine, surface_id, true);
            }
        } else {
            return false;
        }
        if split_handled {
            if let Some(item) = split_snapshot {
                engine.push_closed_item(item);
            }
            self.cleanup_surface(engine, surface_id, persist_id);
            self.enqueue_surface_closed(surface_id, kind, true);
            engine.mark_layout_dirty();
        }
        true
    }

    /// ID로 surface를 닫으며 복원 사본을 저장한다.
    /// 사본이 필요 없는 경로는 close_surface_by_id_no_snapshot을 사용한다.
    #[cfg(any(feature = "gui", test))]
    pub fn close_surface_by_id(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        is_user_close: bool,
    ) -> bool {
        self.close_surface_by_id_inner(engine, surface_id, true, is_user_close)
    }

    /// 복원 사본 없이 닫는다. 워크스페이스가 모두 사라지면 다음 화면 처리에 필요한 기본 항목을 만든다.
    pub fn close_surface_by_id_no_snapshot(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        is_user_close: bool,
    ) -> bool {
        let closed = self.close_surface_by_id_inner(engine, surface_id, false, is_user_close);
        if closed {
            self.recreate_workspace_if_empty(engine, "close_surface_by_id_no_snapshot");
        }
        closed
    }

    /// 닫힐 surface의 위치에 따라 탭·pane·워크스페이스 정리까지 직접 실행한다.
    /// 복원 사본 저장 여부와 사용자 닫기 표시는 별개다.
    /// PTY 종료 정리는 save_snapshot=false이지만 is_user_close=true로 보고한다.
    /// 이 조합은 pty_exit_close_skips_the_snapshot_but_still_reports_a_user_close가 검사한다.
    fn close_surface_by_id_inner(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        let loc = match crate::core::locate_surface_in_pane(engine, surface_id) {
            Some(l) => l,
            None => return false,
        };
        if !loc.surface_is_sole_in_tab && loc.can_close_surface_in_group {
            return self.close_case_split(engine, &loc, surface_id, save_snapshot, is_user_close);
        }
        if self.close_case_tab(engine, &loc, save_snapshot, is_user_close) {
            return true;
        }
        if self.close_case_pane(engine, &loc, save_snapshot, is_user_close) {
            return true;
        }
        self.close_case_workspace(engine, &loc, save_snapshot, is_user_close)
    }

    fn close_case_split(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        if save_snapshot {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            let tab = &pane.tabs[loc.tab_idx];
            if terminal_surface_in_tab(tab, surface_id).is_some() {
                let snapshot = crate::model::closed_item::ClosedSurface::from_surface_id(
                    surface_id,
                    engine.terminals.get(surface_id),
                );
                let tab_name = tab.display_name().to_string();
                engine.push_closed_item(crate::model::ClosedItem::Surface {
                    surface: snapshot,
                    tab_name,
                });
            }
        }
        let persist_id = engine
            .terminals
            .scrollback_persist_id(surface_id)
            .map(str::to_string);
        let kind = self.surface_kind(engine, surface_id);
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        let tab = &mut pane.tabs[loc.tab_idx];
        if tab.close_surface(surface_id) {
            self.cleanup_surface(engine, surface_id, persist_id);
            self.enqueue_surface_closed(surface_id, kind, is_user_close);
            engine.mark_layout_dirty();
            return true;
        }
        false
    }

    fn close_case_tab(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        if save_snapshot {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                let snapshot_opt = {
                    let mut snap_fn =
                        crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
                    let terminals = &engine.terminals;
                    crate::model::closed_item::ClosedTab::from_tab(
                        &pane.tabs[loc.tab_idx],
                        &mut snap_fn,
                        &|id| terminals.get(id),
                    )
                };
                if let Some(snapshot) = snapshot_opt {
                    engine.push_closed_item(crate::model::ClosedItem::Tab(snapshot));
                }
            }
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                crate::core::impl_close::collect_close_targets(
                    &pane.tabs[loc.tab_idx],
                    engine,
                    &mut targets,
                );
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        if pane.tabs.len() > 1 {
            pane.remove_tab_preserving_active(loc.tab_idx);
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, is_user_close);
            }
            engine.mark_layout_dirty();
            return true;
        }
        false
    }

    fn close_case_pane(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        // 부모 Split 정보가 사라지기 전에 pane의 복원 사본을 만든다.
        if save_snapshot {
            let ws = &engine.workspaces[loc.ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
                && let Some((direction, ratio, was_first, sibling_pane_id)) =
                    ws.pane_layout().locate_split_context(loc.pane_id)
            {
                let snapshot = {
                    let mut snap_fn =
                        crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
                    let terminals = &engine.terminals;
                    crate::model::ClosedItem::from_pane(
                        pane,
                        sibling_pane_id,
                        direction,
                        ratio,
                        was_first,
                        &mut snap_fn,
                        &|id| terminals.get(id),
                    )
                };
                engine.push_closed_item(snapshot);
            }
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
            {
                for tab in &pane.tabs {
                    crate::core::impl_close::collect_close_targets(tab, engine, &mut targets);
                }
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        if ws.pane_layout().all_pane_ids().len() > 1 {
            ws.close_pane_preserving_focus(loc.pane_id);
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, is_user_close);
            }
            engine.mark_layout_dirty();
            return true;
        }
        false
    }

    fn close_case_workspace(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        use crate::close_trace;
        use std::time::Instant;

        const PATH: &str = "inline";

        let t_close = Instant::now();
        if save_snapshot {
            let t = Instant::now();
            let item = Self::capture_workspace_snapshot(engine, loc.ws_idx);
            close_trace::log_snapshot(t, &item, PATH);
            let t = Instant::now();
            engine.push_closed_item(item).log(t.elapsed(), PATH);
        }
        let t = Instant::now();
        let targets = Self::collect_workspace_close_targets(engine, loc.ws_idx);
        close_trace::log_collect(t, targets.len(), PATH);
        let target_kinds: Vec<Option<&'static str>> = targets
            .iter()
            .map(|(sid, _)| self.surface_kind(engine, *sid))
            .collect();
        let workspace_id = engine.workspaces[loc.ws_idx].id;
        engine.workspaces.remove(loc.ws_idx);
        self.fix_workspace_pointers_after_removal(loc.ws_idx, engine.workspaces.len());
        self.after_workspace_removed(workspace_id, PATH);
        let zipped: Vec<(u32, Option<String>, Option<&'static str>)> = targets
            .into_iter()
            .zip(target_kinds)
            .map(|((sid, pid), kind)| (sid, pid, kind))
            .collect();
        let surfaces = zipped.len();
        self.cleanup_targets(engine, zipped, is_user_close, Some(PATH));
        engine.mark_layout_dirty();
        close_trace::log_total(t_close, surfaces, save_snapshot, PATH);
        true
    }
}

#[cfg(test)]
impl AppState {
    /// 시험 준비용 직접 분할. 제품 코드는 Core의 DomainIntent::SplitPane을 사용한다.
    pub(crate) fn test_split_pane(
        &mut self,
        engine: &mut CoreState,
        direction: SplitDirection,
    ) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd(engine);
        let new_pane_id = engine.next_ids.next_pane();
        let new_tab_id = engine.next_ids.next_tab();
        let new_surface_id = engine.next_ids.next_surface();
        let cols = engine.default_cols;
        let rows = engine.default_rows;

        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let terminal = crate::model::Pane::spawn_terminal(
            new_surface_id,
            crate::model::ShellSpawnOpts {
                cols,
                rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                extra_env: &sh.envs_ref(),
                waker: engine.make_waker(new_surface_id),
                working_dir: cwd.as_deref(),
            },
        )?;
        engine.terminals.insert(new_surface_id, terminal);
        let new_pane =
            crate::model::Pane::new_with_terminal_marker(new_pane_id, new_tab_id, new_surface_id);

        let ws = self.active_workspace_mut(engine);
        let target_pane_id = ws.focused_pane;
        ws.pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);
        ws.focused_pane = new_pane_id;
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();
        self.enqueue_host_event(super::PendingHostEvent::PaneSplit {
            original_pane: target_pane_id,
            new_pane: new_pane_id,
            direction,
        });
        Ok(())
    }
}

#[cfg(feature = "gui")]
impl AppState {
    pub(crate) fn observe_tutorial_surface_split(
        &mut self,
        engine: &CoreState,
        workspace_index: usize,
        pane_id: u32,
        new_surface_id: u32,
    ) {
        if let Some(ws) = engine.workspaces.get(workspace_index)
            && let Some(pane) = ws.pane_layout().find_pane(pane_id)
            && let Some(tab) = pane
                .tabs
                .iter()
                .find(|tab| tab.contains_surface(new_surface_id))
        {
            self.tutorial
                .observe(crate::adapters::ui::tutorial::PracticeEvent::SplitSurface {
                    workspace: ws.id,
                    pane: pane_id,
                    tab: tab.id,
                });
        }
    }

    pub(crate) fn observe_tutorial_pane_split(
        &mut self,
        workspace: u32,
        original: u32,
        new_pane: u32,
    ) {
        self.tutorial
            .observe(crate::adapters::ui::tutorial::PracticeEvent::SplitPane {
                workspace,
                original,
                new_pane,
            });
    }
}
