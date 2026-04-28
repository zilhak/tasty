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

    /// Add a new workspace without switching to it, with optional explicit cwd and surface type. Used by IPC/CLI.
    /// Returns the new workspace index.
    pub fn add_workspace_background(
        &mut self,
        explicit_cwd: Option<std::path::PathBuf>,
        surface_type: crate::model::SurfaceType,
    ) -> anyhow::Result<usize> {
        let ws_id = self.engine.next_ids.next_workspace();
        let pane_id = self.engine.next_ids.next_pane();
        let tab_id = self.engine.next_ids.next_tab();
        let surface_id = self.engine.next_ids.next_surface();

        let name = format!("Workspace {}", self.engine.workspaces.len() + 1);
        let is_terminal = matches!(surface_type, crate::model::SurfaceType::Terminal);

        let ws = match surface_type {
            crate::model::SurfaceType::Terminal => {
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
            }
            crate::model::SurfaceType::Markdown { file } => {
                let surface: Box<dyn crate::model::Surface> =
                    Box::new(crate::model::MarkdownPanel::new(surface_id, file));
                let pane = crate::model::Pane::new_with_surface(
                    pane_id,
                    tab_id,
                    "Markdown".to_string(),
                    surface,
                );
                Workspace::new_with_pane(ws_id, name, pane)
            }
            crate::model::SurfaceType::Explorer { path } => {
                let root = path.unwrap_or_else(|| {
                    directories::BaseDirs::new()
                        .map(|d| d.home_dir().to_string_lossy().to_string())
                        .unwrap_or_else(|| ".".to_string())
                });
                let surface: Box<dyn crate::model::Surface> =
                    Box::new(crate::model::ExplorerPanel::new(surface_id, root));
                let pane = crate::model::Pane::new_with_surface(
                    pane_id,
                    tab_id,
                    "Explorer".to_string(),
                    surface,
                );
                Workspace::new_with_pane(ws_id, name, pane)
            }
            crate::model::SurfaceType::Html { url } => {
                let surface: Box<dyn crate::model::Surface> =
                    Box::new(crate::model::HtmlPanel::new(surface_id, url));
                let pane = crate::model::Pane::new_with_surface(
                    pane_id,
                    tab_id,
                    "Html".to_string(),
                    surface,
                );
                Workspace::new_with_pane(ws_id, name, pane)
            }
            crate::model::SurfaceType::Image { file } => {
                let surface: Box<dyn crate::model::Surface> = match file {
                    Some(path) => Box::new(crate::model::ImagePanel::new(surface_id, path)),
                    None => Box::new(crate::model::ImagePanel::new_blank(surface_id)),
                };
                let pane = crate::model::Pane::new_with_surface(
                    pane_id,
                    tab_id,
                    "Image".to_string(),
                    surface,
                );
                Workspace::new_with_pane(ws_id, name, pane)
            }
            crate::model::SurfaceType::Empty => {
                anyhow::bail!("Cannot create workspace with empty surface type");
            }
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

    /// Ensure all deferred tabs in the active workspace are initialized.
    /// Called on workspace switch to lazily spawn PTYs for restored terminals.
    fn ensure_active_workspace_initialized(&mut self) {
        let mut spawned_ids = Vec::new();
        {
            let ws = &mut self.engine.workspaces[self.active_workspace];
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    for tab in &mut pane.tabs {
                        if tab.is_deferred() {
                            let surface_id = tab.deferred_surface_id.unwrap_or(0);
                            if tab.ensure_initialized(surface_id) {
                                spawned_ids.push(surface_id);
                            }
                        }
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
        self.engine.closed_items.push(snapshot);
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
