use serde_json::Value;

use crate::model::Workspace;

use super::AppState;

impl AppState {
    /// Add a new workspace with one pane, one tab, one terminal.
    pub fn add_workspace(&mut self) -> anyhow::Result<()> {
        let cwd = self.resolve_inherit_cwd();
        let ws_id = self.engine.next_ids.next_workspace();
        let pane_id = self.engine.next_ids.next_pane();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();

        let name = format!("Workspace {}", self.engine.workspaces.len() + 1);
        let shell = if self.engine.settings.general.shell.is_empty() {
            None
        } else {
            Some(self.engine.settings.general.shell.as_str())
        };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let ws = Workspace::new_with_shell(
            ws_id,
            name,
            self.engine.default_cols,
            self.engine.default_rows,
            pane_id,
            tab_id,
            surface_id,
            shell,
            &shell_args,
            self.engine.make_waker(surface_id),
            cwd.as_deref(),
        )?;
        self.engine.workspaces.push(ws);
        self.active_workspace = self.engine.workspaces.len() - 1;
        self.send_fast_init(surface_id);
        self.engine.mark_layout_dirty();
        Ok(())
    }

    /// Add a new workspace without switching to it. Used by IPC/CLI.
    /// `kind`은 SurfaceKindRegistry 식별자. `"terminal"`은 PTY spawn 경로,
    /// 그 외는 registry.create로 생성한다. `"empty"`로 워크스페이스를 만들 수는 없다.
    /// Returns the new workspace index.
    pub fn add_workspace_background(
        &mut self,
        explicit_cwd: Option<std::path::PathBuf>,
        kind: &str,
        params: &Value,
    ) -> anyhow::Result<usize> {
        let ws_id = self.engine.next_ids.next_workspace();
        let pane_id = self.engine.next_ids.next_pane();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();

        let name = format!("Workspace {}", self.engine.workspaces.len() + 1);
        let is_terminal = kind == "terminal";

        let ws = if kind == "terminal" {
            let cwd = explicit_cwd.or_else(|| self.resolve_inherit_cwd());
            let shell = if self.engine.settings.general.shell.is_empty() {
                None
            } else {
                Some(self.engine.settings.general.shell.as_str())
            };
            let shell_args_owned = self.engine.settings.general.effective_shell_args();
            let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
            Workspace::new_with_shell(
                ws_id,
                name,
                self.engine.default_cols,
                self.engine.default_rows,
                pane_id,
                tab_id,
                surface_id,
                shell,
                &shell_args,
                self.engine.make_waker(surface_id),
                cwd.as_deref(),
            )?
        } else if kind == "empty" {
            anyhow::bail!("Cannot create workspace with empty surface kind");
        } else {
            let surface = self.create_surface_via_registry(kind, surface_id, params)?;
            let tab_name = super::pane::default_tab_name_for_kind(kind, params);
            let pane = crate::model::Pane::new_with_surface(pane_id, tab_id, tab_name, surface);
            Workspace::new_with_pane(ws_id, name, pane)
        };

        self.engine.workspaces.push(ws);
        let idx = self.engine.workspaces.len() - 1;
        if is_terminal {
            self.send_fast_init(surface_id);
        }
        self.engine.mark_layout_dirty();
        Ok(idx)
    }

    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, index: usize) {
        if index < self.engine.workspaces.len() {
            self.active_workspace = index;
            self.ensure_active_workspace_initialized();
        }
    }

    /// Move a workspace from one index to another, adjusting active_workspace accordingly.
    /// Returns false if indices are out of bounds or equal.
    pub fn move_workspace(&mut self, from: usize, to: usize) -> bool {
        let len = self.engine.workspaces.len();
        if from == to || from >= len || to >= len {
            return false;
        }
        let ws = self.engine.workspaces.remove(from);
        self.engine.workspaces.insert(to, ws);
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
    fn ensure_active_workspace_initialized(&mut self) {
        let mut spawned_ids = Vec::new();
        {
            let ws = &mut self.engine.workspaces[self.active_workspace];
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
            self.engine.send_fast_init(surface_id);
        }
    }

    /// Close the active workspace. Returns true if the workspace was removed.
    /// Cleans up all surfaces (claude parent-child, surface meta) in the workspace.
    pub fn close_active_workspace(&mut self) -> bool {
        if self.engine.workspaces.is_empty() {
            return false;
        }
        let ws_idx = self.active_workspace;
        // Capture workspace snapshot before closing
        let snapshot = crate::model::ClosedItem::from_workspace(&self.engine.workspaces[ws_idx]);
        self.engine.push_closed_item(snapshot);
        // Collect all surface IDs for cleanup
        let surface_ids = self.engine.workspaces[ws_idx].all_surface_ids();
        self.engine.workspaces.remove(ws_idx);
        // Adjust active workspace index
        if self.active_workspace >= self.engine.workspaces.len()
            && !self.engine.workspaces.is_empty()
        {
            self.active_workspace = self.engine.workspaces.len() - 1;
        }
        // Cleanup
        for sid in surface_ids {
            self.cleanup_surface(sid);
        }
        self.engine.mark_layout_dirty();
        true
    }

    /// Ensure at least one workspace exists. If none exist, create a new one.
    /// Returns true if a new workspace was created.
    pub fn ensure_workspace_exists(&mut self) -> bool {
        if !self.engine.workspaces.is_empty() {
            return false;
        }
        match self.add_workspace() {
            Ok(()) => true,
            Err(e) => {
                tracing::error!("Failed to create workspace: {}", e);
                false
            }
        }
    }
}
