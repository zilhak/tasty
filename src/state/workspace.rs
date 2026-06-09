use crate::core::CoreState;

use super::AppState;

impl AppState {
    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, engine: &mut CoreState, index: usize) {
        if index < engine.workspaces.len() {
            self.active_workspace = index;
            self.ensure_active_workspace_initialized(engine);
        }
    }

    /// Move a workspace from one index to another, adjusting active_workspace accordingly.
    /// Returns false if indices are out of bounds or equal.
    pub fn move_workspace(&mut self, engine: &mut CoreState, from: usize, to: usize) -> bool {
        let len = engine.workspaces.len();
        if from == to || from >= len || to >= len {
            return false;
        }
        let ws = engine.workspaces.remove(from);
        engine.workspaces.insert(to, ws);
        // Adjust active_workspace to follow the moved workspace or account for the shift
        if self.active_workspace == from {
            self.active_workspace = to;
        } else if from < to && self.active_workspace > from && self.active_workspace <= to {
            self.active_workspace -= 1;
        } else if from > to && self.active_workspace >= to && self.active_workspace < from {
            self.active_workspace += 1;
        }
        true
    }

    /// 활성 workspace에서 사용자가 보고 있는 active_tab의 deferred surface(들)만 PTY를
    /// spawn. 같은 pane의 비활성 tab은 deferred로 남았다가 tab 전환 시 깨어난다.
    /// active_tab이 split layout이면 그 안의 모든 deferred placeholder를 한번에 spawn한다.
    fn ensure_active_workspace_initialized(&mut self, engine: &mut CoreState) {
        let mut spawned: Vec<(u32, tasty_terminal::Terminal, Option<String>)> = Vec::new();
        {
            let ws = &mut engine.workspaces[self.active_workspace];
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    let active_idx = pane.active_tab;
                    if let Some(tab) = pane.tabs.get_mut(active_idx) {
                        let mut entries = tab.ensure_all_initialized();
                        spawned.append(&mut entries);
                    }
                }
            }
        }
        for (surface_id, terminal, persist_id) in spawned {
            engine.terminals.insert(surface_id, terminal);
            if let Some(pid) = persist_id {
                engine.terminals.set_scrollback_persist_id(surface_id, pid);
            }
            engine.send_fast_init(surface_id);
            engine.apply_pending_scrollback_inject(surface_id);
        }
    }

    /// Close the active workspace. Returns true if the workspace was removed.
    /// Cleans up all surfaces (surface meta + per-surface view state) in the workspace.
    pub fn close_active_workspace(&mut self, engine: &mut CoreState) -> bool {
        self.close_workspace_at(engine, self.active_workspace)
    }

    /// Close a specific workspace by index (context menu 등 임의 지정 close).
    /// Cleans up all surfaces + closed_item snapshot + memory scope purge.
    pub fn close_workspace_at(&mut self, engine: &mut CoreState, ws_idx: usize) -> bool {
        if ws_idx >= engine.workspaces.len() {
            return false;
        }
        // Capture workspace snapshot before closing
        let snapshot = {
            let mut snap_fn =
                crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
            let terminals = &engine.terminals;
            crate::model::ClosedItem::from_workspace(
                &engine.workspaces[ws_idx],
                &mut snap_fn,
                &|id| terminals.get(id),
            )
        };
        engine.push_closed_item(snapshot);
        // Collect all (surface_id, persist_id) for cleanup before removing the workspace.
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        super::AppState::collect_close_targets(tab, engine, &mut targets);
                    }
                }
            }
        }
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        // Workspace scope 의 memory entry 정리. 안의 surface 들은 아래 cleanup_surface
        // 에서 각자 자기 scope 를 purge 한다.
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
        // Adjust active workspace index
        if self.active_workspace >= engine.workspaces.len() && !engine.workspaces.is_empty() {
            self.active_workspace = engine.workspaces.len() - 1;
        }
        // Cleanup
        // workspace.remove 후엔 surface_kind 가 None 을 반환할 수 있으나, plugin
        // lifecycle 구독자는 surface_id 만으로 cleanup 가능 (R1 분석).
        for (sid, pid) in targets {
            let kind = self.surface_kind(engine, sid);
            self.cleanup_surface(engine, sid, pid);
            self.enqueue_surface_closed(sid, kind, true);
        }
        engine.mark_layout_dirty();
        true
    }
}
