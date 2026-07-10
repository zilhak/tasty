use serde_json::Value;

use crate::core::CoreState;
#[cfg(test)]
use crate::model::SplitDirection;

use super::AppState;

/// Helper: tab 내 surface_id 에 해당하는 TerminalSurface 를 찾는다 (downcast).
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
    /// active 워크스페이스가 mirror(원격 attach client)면 차단 toast 를 띄우고 `true`.
    /// mirror 워크스페이스는 원격 워크스페이스의 뷰라, 그 안의 로컬 구조 변경
    /// (new-tab / close / move-tab)은 "workspace 전체가 remote" 불변식을 깬다 —
    /// `Core::apply` 를 우회하는 UI-layer 직접 조작 경로(`add_tab`·`close_active_*`·
    /// tab drag/context-menu)가 공통으로 이 가드를 태워 로컬 실행을 막는다.
    ///
    /// mirror 워크스페이스 **자체를 닫는 것**(`close_active_workspace`)은 로컬 mirror
    /// 뷰를 걷어내는 정당한 로컬 동작이므로 가드하지 않는다. 구조 변경 원격 forward 는
    /// 2단계에서 붙는다.
    pub(crate) fn block_mirror_structural(&mut self, engine: &CoreState) -> bool {
        if self.active_workspace(engine).mirror {
            #[cfg(feature = "gui")]
            self.toasts.push(
                crate::i18n::t("attach.toast.mirror_structural_blocked"),
                crate::model::toast_kind::ToastKind::Warning,
                crate::model::toast_kind::ToastScope::Window,
            );
            true
        } else {
            false
        }
    }

    /// Close the focused pane (unsplit). Returns true if a pane was removed.
    pub fn close_active_pane(&mut self, engine: &mut CoreState) -> bool {
        // mirror 누출 차단 — pane close 는 mirror 트리를 원격과 어긋나게 한다.
        // 가드가 true 를 돌려 fallback 체인(→ close_active_workspace)을 멈춘다.
        if self.block_mirror_structural(engine) {
            return true;
        }
        let target_id = self.active_workspace(engine).focused_pane;

        // Collect all (surface_id, persist_id) in the pane being closed for cleanup.
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = self.active_workspace(engine);
            if let Some(pane) = ws.pane_layout().find_pane(target_id) {
                for tab in &pane.tabs {
                    Self::collect_close_targets(tab, engine, &mut targets);
                }
            }
        }

        let ws = self.active_workspace_mut(engine);
        let removed = ws.pane_layout_mut().close_pane(target_id);
        if removed {
            // Update focus to the first available pane
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
            // cleanup_surface 직후엔 layout 에서 surface 가 사라져 kind 조회 불가 →
            // 미리 캡쳐. plugin lifecycle 큐에 cleanup_targets 모두 enqueue (R1 분석).
            for (sid, pid) in targets {
                let kind = self.surface_kind(engine, sid);
                self.cleanup_surface(engine, sid, pid);
                self.enqueue_surface_closed(sid, kind, true);
            }
            engine.mark_layout_dirty();
        }
        removed
    }

    /// Close the focused surface. For split tabs, closes the focused surface
    /// within the tab. For single-surface tabs, delegates to close_surface_by_id
    /// which handles tab/pane/workspace cascading.
    pub fn close_active_surface(&mut self, engine: &mut CoreState) -> bool {
        // mirror 누출 차단 — surface close 는 mirror 트리를 원격과 어긋나게 한다.
        // true 를 돌려 호출부의 close fallback 체인을 멈춘다(전체 mirror 는 유지).
        if self.block_mirror_structural(engine) {
            return true;
        }
        let surface_id;

        if let Some(pane) = self.focused_pane(engine) {
            let tab = match pane.tabs.get(pane.active_tab) {
                Some(t) => t,
                None => return false,
            };
            surface_id = tab.focused_surface;
        } else {
            return false;
        }
        let persist_id = engine
            .terminals
            .scrollback_persist_id(surface_id)
            .map(str::to_string);
        let kind = self.surface_kind(engine, surface_id);
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
            self.cleanup_surface(engine, surface_id, persist_id);
            self.enqueue_surface_closed(surface_id, kind, true);
            engine.mark_layout_dirty();
        }
        true
    }

    /// Close a specific surface by ID. Cascades up the hierarchy:
    /// surface -> tab -> pane -> workspace as needed.
    /// When `save_snapshot` is true, the closed item is saved for user restore (Ctrl+Shift+T).
    /// Agent/IPC closures should pass false to avoid polluting the user's undo stack.
    pub fn close_surface_by_id(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        is_user_close: bool,
    ) -> bool {
        self.close_surface_by_id_inner(engine, surface_id, true, is_user_close)
    }

    /// Close without saving snapshot (for IPC/agent-initiated closures).
    ///
    /// Agent 가 마지막 workspace 까지 닫아 windows 상태가 비어 버리면, 다음
    /// redraw 가 `active_workspace()` 를 호출하다 패닉한다. 사용자의 window 를
    /// 에이전트가 끄는 부작용도 피해야 하므로 (CLAUDE.md "사용자 행동과 에이전트
    /// 행동의 분리"), cascade 결과 workspaces 가 비면 즉시 새 empty workspace
    /// 를 만들어 invariant 를 유지한다.
    pub fn close_surface_by_id_no_snapshot(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        is_user_close: bool,
    ) -> bool {
        let closed = self.close_surface_by_id_inner(engine, surface_id, false, is_user_close);
        if closed && engine.workspaces.is_empty() {
            // free fn 으로 직접 호출 — Core 통과 없이 invariant 복구 (apply_create_workspace_inner
            // 은 engine-only 라 self/Core 의존 없음). 본 호출처는 PTY exit cleanup
            // (cascade_terminal_process_exited) 과 egui 의 diff close 두 곳에서 도달 — 둘 다
            // *시스템 invariant restorer* 라 host event 발화 불필요.
            match crate::core::apply_create_workspace_inner(
                engine,
                None,
                "terminal".to_string(),
                serde_json::Value::Null,
                None,
                None,
                None,
                None,
            ) {
                Ok(crate::core::intent::CoreEvent::WorkspaceCreated { index, .. }) => {
                    self.active_workspace = index;
                }
                Ok(_) => unreachable!("apply_create_workspace_inner 는 WorkspaceCreated 만 반환"),
                Err(e) => tracing::warn!(
                    "close_surface_by_id_no_snapshot: auto-recreate workspace failed: {e}"
                ),
            }
        }
        closed
    }

    /// surface→tab→pane→workspace cascade close 의 인라인 실행형 디스패처.
    /// Step1 판정(공유 `locate_surface_in_pane`)으로 위치를 잡고 case1..4 메서드에
    /// 순차 위임한다. C2(`Core::apply_close_surface`)가 CoreEvent 를 *반환*해
    /// caller 가 cleanup 을 실행하는 것과 달리, 여기서는 각 case 메서드가
    /// cleanup/enqueue 를 *직접 실행*한다.
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
        if self.close_case_pane(engine, &loc, is_user_close) {
            return true;
        }
        self.close_case_workspace(engine, &loc, save_snapshot, is_user_close)
    }

    /// Case 1: split tab 내 다중 surface 중 하나 close. 닫혔으면 true.
    fn close_case_split(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        surface_id: u32,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        // Capture surface snapshot before closing (user actions only)
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
        // close 이전에 leaf surface 의 persist_id 를 추출해 둔다.
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

    /// Case 2: surface 가 tab 유일 content 이고 pane.tabs>1 — tab close.
    /// 처리했으면 true, 조건 불충족이면 false(fallthrough).
    fn close_case_tab(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        // Capture tab snapshot before removing (user actions only).
        // Must be done in a separate scope to avoid borrow conflicts.
        if save_snapshot {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                let snapshot_opt = {
                    let mut snap_fn =
                        crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
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
        // tab 의 모든 leaf surface 의 persist_id 수집 후 close.
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            let pane = ws.pane_layout().find_pane(loc.pane_id).unwrap();
            if pane.tabs.len() > 1 {
                Self::collect_close_targets(&pane.tabs[loc.tab_idx], engine, &mut targets);
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(loc.pane_id).unwrap();
        if pane.tabs.len() > 1 {
            pane.tabs.remove(loc.tab_idx);
            if pane.active_tab >= pane.tabs.len() {
                pane.active_tab = pane.tabs.len() - 1;
            }
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

    /// Case 3: pane 의 마지막 tab 이고 ws 안 pane>1 — pane close.
    /// 처리했으면 true, 조건 불충족이면 false(fallthrough).
    fn close_case_pane(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        is_user_close: bool,
    ) -> bool {
        // pane 내 모든 tab 의 leaf surface persist_id 수집.
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1
                && let Some(pane) = ws.pane_layout().find_pane(loc.pane_id)
            {
                for tab in &pane.tabs {
                    Self::collect_close_targets(tab, engine, &mut targets);
                }
            }
        }
        let ws = &mut engine.workspaces[loc.ws_idx];
        if ws.pane_layout().all_pane_ids().len() > 1 {
            ws.pane_layout_mut().close_pane(loc.pane_id);
            if let Some(first) = ws.pane_layout().first_pane() {
                ws.focused_pane = first.id;
            }
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

    /// Case 4 & 5: workspace 의 마지막 pane — workspace close. 항상 true.
    /// `target_kinds` 를 `workspaces.remove` **전에** 선캡처하고(remove 후
    /// surface_kind 조회 불가), memory purge + active_workspace 보정을 포함한다.
    fn close_case_workspace(
        &mut self,
        engine: &mut CoreState,
        loc: &crate::core::SurfaceCloseLocation,
        save_snapshot: bool,
        is_user_close: bool,
    ) -> bool {
        // Capture workspace snapshot before removing (user actions only)
        if save_snapshot {
            let item = {
                let mut snap_fn =
                    crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
                let ws = &engine.workspaces[loc.ws_idx];
                let terminals = &engine.terminals;
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn, &|id| terminals.get(id))
            };
            engine.push_closed_item(item);
        }
        // Workspace 전체의 모든 leaf surface persist_id 수집 (제거 전).
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[loc.ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        Self::collect_close_targets(tab, engine, &mut targets);
                    }
                }
            }
        }
        // workspaces.remove 이후엔 surface_kind 조회 불가 → 미리 캡쳐.
        let target_kinds: Vec<Option<&'static str>> = targets
            .iter()
            .map(|(sid, _)| self.surface_kind(engine, *sid))
            .collect();
        let workspace_id = engine.workspaces[loc.ws_idx].id;
        engine.workspaces.remove(loc.ws_idx);
        if self.active_workspace >= engine.workspaces.len() && !engine.workspaces.is_empty() {
            self.active_workspace = engine.workspaces.len() - 1;
        }
        // Workspace scope 의 memory entry 정리 (마지막 surface 가 닫혀 workspace 도 사라지는 경로).
        let ws_scope = tasty_memory::Scope::Workspace(workspace_id);
        match self.with_memory(|m| m.purge_scope(&ws_scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                workspace_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-workspace scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(workspace_id, "memory: purge_scope failed: {e}"),
        }
        for ((sid, pid), kind) in targets.into_iter().zip(target_kinds) {
            self.cleanup_surface(engine, sid, pid);
            self.enqueue_surface_closed(sid, kind, is_user_close);
        }
        engine.mark_layout_dirty();
        true
    }
}

#[cfg(test)]
impl AppState {
    /// Test-only helper: split the focused pane along the given direction.
    ///
    /// Production code uses `DomainIntent::SplitPane` dispatched through `Core`.
    /// 테스트 setup 편의 용도로만 직접 호출한다.
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

/// kind+params로부터 합리적인 탭 표시명을 도출한다.
///
/// kind 가 매니페스트 `name_from_param`(registry `SurfaceKindDef.name_from_param`)을
/// 선언하면 그 params 키의 값 basename 을 표시명으로 쓴다(예: markdown="file" →
/// `README.md`). 미선언이거나 그 키가 params 에 없으면 kind 의 표시명 fallback
/// (`display_name_i18n_key` 번역, 미등록이면 kind 문자열)으로 떨어진다. 본체의
/// `kind == "markdown"` basename 명명 하드코딩을 generic 화한다.
pub(crate) fn default_tab_name_for_kind(
    kind: &str,
    params: &Value,
    def: Option<&crate::engine::surface_registry::SurfaceKindDef>,
) -> String {
    fn basename_or(path: &str, fallback: &str) -> String {
        path.split(['/', '\\'])
            .rfind(|s| !s.is_empty())
            .unwrap_or(fallback)
            .to_string()
    }
    // kind 표시명 fallback: registry display_name_i18n_key 번역(미등록이면 kind 그대로).
    let fallback = || {
        def.map(|d| crate::i18n::t(d.display_name_i18n_key).to_string())
            .unwrap_or_else(|| kind.to_string())
    };
    if let Some(key) = def.and_then(|d| d.name_from_param.as_deref())
        && let Some(p) = params.get(key).and_then(|v| v.as_str())
    {
        let fb = fallback();
        return basename_or(p, &fb);
    }
    fallback()
}
