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
        let shell = if self.engine.settings.general.shell.is_empty() { None } else { Some(self.engine.settings.general.shell.as_str()) };
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
        Ok(())
    }

    /// Add a new workspace without switching to it, with optional explicit cwd. Used by IPC/CLI.
    /// Returns the new workspace index.
    pub fn add_workspace_background(&mut self, explicit_cwd: Option<std::path::PathBuf>) -> anyhow::Result<usize> {
        let cwd = explicit_cwd.or_else(|| self.resolve_inherit_cwd());
        let ws_id = self.engine.next_ids.next_workspace();
        let pane_id = self.engine.next_ids.next_pane();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();

        let name = format!("Workspace {}", self.engine.workspaces.len() + 1);
        let shell = if self.engine.settings.general.shell.is_empty() { None } else { Some(self.engine.settings.general.shell.as_str()) };
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
        let idx = self.engine.workspaces.len() - 1;
        self.send_fast_init(surface_id);
        Ok(idx)
    }

    /// Switch to workspace by index (0-based).
    pub fn switch_workspace(&mut self, index: usize) {
        if index < self.engine.workspaces.len() {
            self.active_workspace = index;
        }
    }

    /// Close the active workspace. Returns true if the workspace was removed.
    /// Cleans up all surfaces (claude parent-child, surface meta) in the workspace.
    pub fn close_active_workspace(&mut self) -> bool {
        if self.engine.workspaces.is_empty() {
            return false;
        }
        let ws_idx = self.active_workspace;
        // Collect all surface IDs for cleanup
        let surface_ids = self.engine.workspaces[ws_idx].all_surface_ids();
        self.engine.workspaces.remove(ws_idx);
        // Adjust active workspace index
        if self.active_workspace >= self.engine.workspaces.len() && !self.engine.workspaces.is_empty() {
            self.active_workspace = self.engine.workspaces.len() - 1;
        }
        // Cleanup
        for sid in surface_ids {
            self.unregister_child(sid);
            self.mark_parent_closed(sid);
            crate::surface_meta::SurfaceMetaStore::remove(sid);
        }
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
