use serde_json::Value;

use crate::engine_state::CoreState;
use crate::model::Workspace;

use super::AppState;

impl AppState {
    /// Add a new workspace with one pane, one tab, one terminal.
    pub fn add_workspace(&mut self, engine: &mut CoreState) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd(engine);
        let ws_id = engine.next_ids.next_workspace();
        let pane_id = engine.next_ids.next_pane();
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();

        let name = format!("Workspace {}", engine.workspaces.len() + 1);
        let shell = if engine.settings.general.shell.is_empty() {
            None
        } else {
            Some(engine.settings.general.shell.as_str())
        };
        let shell_args_owned = engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let ws = Workspace::new_with_shell(
            ws_id,
            name,
            pane_id,
            tab_id,
            surface_id,
            crate::model::ShellSpawnOpts {
                cols: engine.default_cols,
                rows: engine.default_rows,
                shell: shell,
                shell_args: &shell_args,
                waker: engine.make_waker(surface_id),
                working_dir: cwd.as_deref(),
            },
        )?;
        engine.workspaces.push(ws);
        self.active_workspace = engine.workspaces.len() - 1;
        engine.send_fast_init(surface_id);
        engine.mark_layout_dirty();
        Ok(())
    }

    /// Add a new workspace without switching to it. Used by IPC/CLI.
    /// `kind`은 SurfaceKindRegistry 식별자. `"terminal"`은 PTY spawn 경로,
    /// 그 외는 registry.create로 생성한다. `"empty"`로 워크스페이스를 만들 수는 없다.
    /// Returns the new workspace index.
    pub fn add_workspace_background(
        &mut self,
        engine: &mut CoreState,
        explicit_cwd: Option<std::path::PathBuf>,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<usize> {
        let ws_id = engine.next_ids.next_workspace();
        let pane_id = engine.next_ids.next_pane();
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();

        let name = format!("Workspace {}", engine.workspaces.len() + 1);
        let is_terminal = kind == "terminal";

        let ws = if kind == "terminal" {
            let cwd = explicit_cwd.or_else(|| self.resolve_inherit_cwd(engine));
            let shell = if engine.settings.general.shell.is_empty() {
                None
            } else {
                Some(engine.settings.general.shell.as_str())
            };
            let shell_args_owned = engine.settings.general.effective_shell_args();
            let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
            Workspace::new_with_shell(
                ws_id,
                name,
                pane_id,
                tab_id,
                surface_id,
                crate::model::ShellSpawnOpts {
                    cols: engine.default_cols,
                    rows: engine.default_rows,
                    shell: shell,
                    shell_args: &shell_args,
                    waker: engine.make_waker(surface_id),
                    working_dir: cwd.as_deref(),
                },
            )?
        } else if kind == "empty" {
            anyhow::bail!("Cannot create workspace with empty surface kind");
        } else {
            let surface = self.create_surface_via_registry(engine, kind, surface_id, params)?;
            let tab_name = super::pane::default_tab_name_for_kind(kind, params);
            let pane = crate::model::Pane::new_with_surface(pane_id, tab_id, tab_name, surface);
            Workspace::new_with_pane(ws_id, name, pane)
        };

        engine.workspaces.push(ws);
        let idx = engine.workspaces.len() - 1;
        if is_terminal {
            engine.send_fast_init(surface_id);
        }
        engine.mark_layout_dirty();
        Ok(idx)
    }

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
        let mut spawned_ids = Vec::new();
        {
            let ws = &mut engine.workspaces[self.active_workspace];
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    let active_idx = pane.active_tab;
                    if let Some(tab) = pane.tabs.get_mut(active_idx) {
                        let mut ids = tab.ensure_all_initialized();
                        spawned_ids.append(&mut ids);
                    }
                }
            }
        }
        for surface_id in spawned_ids {
            engine.send_fast_init(surface_id);
            engine.apply_pending_scrollback_inject(surface_id);
        }
    }

    /// Close the active workspace. Returns true if the workspace was removed.
    /// Cleans up all surfaces (surface meta + per-surface view state) in the workspace.
    pub fn close_active_workspace(&mut self, engine: &mut CoreState) -> bool {
        if engine.workspaces.is_empty() {
            return false;
        }
        let ws_idx = self.active_workspace;
        // Capture workspace snapshot before closing
        let snapshot = {
            let mut snap_fn =
                crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
            crate::model::ClosedItem::from_workspace(&engine.workspaces[ws_idx], &mut snap_fn)
        };
        engine.push_closed_item(snapshot);
        // Collect all (surface_id, persist_id) for cleanup before removing the workspace.
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        super::AppState::collect_close_targets(tab, &mut targets);
                    }
                }
            }
        }
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        // Workspace scope 의 memory entry 정리. 안의 surface 들은 아래 cleanup_surface
        // 에서 각자 자기 scope 를 purge 한다.
        let ws_scope = tasty_memory::Scope::Workspace(workspace_id);
        {
            let mut guard = match self.memory.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            match guard.purge_scope(&ws_scope) {
                Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                    workspace_id,
                    regular = stats.regular,
                    secret = stats.secret,
                    "memory: purged closed-workspace scope",
                ),
                Ok(_) => {}
                Err(e) => tracing::warn!(workspace_id, "memory: purge_scope failed: {e}"),
            }
        }
        // Adjust active workspace index
        if self.active_workspace >= engine.workspaces.len() && !engine.workspaces.is_empty() {
            self.active_workspace = engine.workspaces.len() - 1;
        }
        // Cleanup
        for (sid, pid) in targets {
            self.cleanup_surface(engine, sid, pid);
        }
        engine.mark_layout_dirty();
        true
    }

    /// Ensure at least one workspace exists. If none exist, create a new one.
    /// Returns true if a new workspace was created.
    pub fn ensure_workspace_exists(&mut self, engine: &mut CoreState) -> bool {
        if !engine.workspaces.is_empty() {
            return false;
        }
        match self.add_workspace(engine) {
            Ok(()) => true,
            Err(e) => {
                tracing::error!("Failed to create workspace: {}", e);
                false
            }
        }
    }
}
