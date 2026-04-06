use tasty_terminal::{Terminal, Waker};
use super::{
    ExplorerPanel, MarkdownPanel, PaneId, Panel, SplitDirection, SurfaceId,
    SurfaceNode, TabId,
};
use super::tab::Tab;
/// A screen region with its own independent tab bar.
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    /// Horizontal scroll offset for the tab bar (in logical pixels).
    #[cfg_attr(test, allow(dead_code))]
    pub tab_scroll_offset: f32,
}

impl Default for Pane {
    fn default() -> Self {
        Self { id: 0, tabs: Vec::new(), active_tab: 0, tab_scroll_offset: 0.0 }
    }
}

impl Pane {
    /// Create a Pane with a custom shell and optional working directory.
    pub fn new_with_shell(
        id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
        working_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<Self> {
        let terminal = Terminal::new_with_shell_args_cwd(cols, rows, shell, shell_args, surface_id, waker, working_dir)?;
        let tab = Tab {
            id: tab_id,
            name: "Shell".to_string(),
            panel_opt: Some(Panel::Terminal(SurfaceNode {
                id: surface_id,
                terminal,
                deferred_spawn: None,
            })),
            deferred_spawn: None,
            explicit_name: None, deferred_surface_id: None,
        };
        Ok(Self {
            id,
            tabs: vec![tab],
            active_tab: 0,
            tab_scroll_offset: 0.0,
        })
    }

    /// Add a new tab with a custom shell and optional working directory.
    pub fn add_tab_with_shell(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
        working_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<()> {
        let terminal = Terminal::new_with_shell_args_cwd(cols, rows, shell, shell_args, surface_id, waker, working_dir)?;
        let tab = Tab {
            id: tab_id,
            name: "Shell".to_string(),
            panel_opt: Some(Panel::Terminal(SurfaceNode {
                id: surface_id,
                terminal,
                deferred_spawn: None,
            })),
            deferred_spawn: None,
            explicit_name: None, deferred_surface_id: None,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        Ok(())
    }

    /// Add a new tab without changing the active tab, with optional working directory.
    pub fn add_tab_background_with_shell(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
        working_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<()> {
        let terminal = Terminal::new_with_shell_args_cwd(cols, rows, shell, shell_args, surface_id, waker, working_dir)?;
        let tab = Tab {
            id: tab_id,
            name: "Shell".to_string(),
            panel_opt: Some(Panel::Terminal(SurfaceNode {
                id: surface_id,
                terminal,
                deferred_spawn: None,
            })),
            deferred_spawn: None,
            explicit_name: None, deferred_surface_id: None,
        };
        self.tabs.push(tab);
        // Do NOT change self.active_tab
        Ok(())
    }

    /// Add a deferred tab (lazy PTY init). The terminal will be spawned when the tab is first accessed.
    pub fn add_tab_deferred(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        shell: Option<&str>,
        shell_args: &[&str],
        cols: usize,
        rows: usize,
        waker: Waker,
        working_dir: Option<&std::path::Path>,
    ) {
        let tab = Tab {
            id: tab_id,
            name: "Shell".to_string(),
            panel_opt: None,
            deferred_spawn: Some(super::surface_group::DeferredSpawn {
                shell: shell.map(|s| s.to_string()),
                shell_args: shell_args.iter().map(|s| s.to_string()).collect(),
                cols,
                rows,
                waker,
                working_dir: working_dir.map(|p| p.to_path_buf()),
            }),
            explicit_name: None, deferred_surface_id: Some(surface_id),
        };
        self.tabs.push(tab);
    }

    /// Collect all surface IDs across all tabs in this pane.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        let mut ids = Vec::new();
        for tab in &self.tabs {
            if let Some(panel) = tab.panel_if_initialized() {
                ids.extend(panel.all_surface_ids());
            }
        }
        ids
    }

    /// Get the active tab's panel. Returns None if tabs are empty or deferred (not yet initialized).
    pub fn active_panel(&self) -> Option<&Panel> {
        if self.tabs.is_empty() { return None; }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        self.tabs[idx].panel_if_initialized()
    }

    /// Get the active tab's panel (mutable). Returns None if tabs are empty or deferred.
    pub fn active_panel_mut(&mut self) -> Option<&mut Panel> {
        if self.tabs.is_empty() { return None; }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        self.tabs[idx].panel_mut_if_initialized()
    }

    /// Ensure the active tab is initialized (lazy PTY spawn). Returns true if spawned.
    pub fn ensure_active_tab_initialized(&mut self, surface_id: SurfaceId) -> bool {
        if self.tabs.is_empty() { return false; }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        self.tabs[idx].ensure_initialized(surface_id)
    }

    /// Split the active panel's focused surface with a custom shell and optional working directory.
    pub fn split_active_surface_with_shell(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
        working_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<()> {
        let new_terminal = Terminal::new_with_shell_args_cwd(cols, rows, shell, shell_args, new_surface_id, waker, working_dir)?;
        if self.tabs.is_empty() {
            return Ok(()); // nothing to split
        }
        let active = self.active_tab.min(self.tabs.len() - 1);
        let tab = &mut self.tabs[active];
        // take/put is safe here: split_surface_with_terminal is infallible.
        let old_panel = tab.take_panel();
        tab.put_panel(old_panel.split_surface_with_terminal(direction, new_surface_id, new_terminal));
        Ok(())
    }

    /// Split a specific surface by ID with optional working directory.
    pub fn split_surface_by_id_with_shell(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
        shell: Option<&str>,
        shell_args: &[&str],
        waker: Waker,
        working_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<()> {
        let new_terminal = Terminal::new_with_shell_args_cwd(cols, rows, shell, shell_args, new_surface_id, waker, working_dir)?;
        for tab in &mut self.tabs {
            let has_target = tab.panel_if_initialized()
                .map(|p| p.find_terminal(target_surface_id).is_some())
                .unwrap_or(false);
            if has_target {
                let old_panel = tab.take_panel();
                tab.put_panel(old_panel.split_surface_by_id_with_terminal(
                    target_surface_id, direction, new_surface_id, new_terminal,
                ));
                return Ok(());
            }
        }
        anyhow::bail!("surface {} not found in this pane", target_surface_id)
    }

    /// Close the tab at the given index. Returns false if the tab can't be closed
    /// (e.g., it's the last tab).
    pub fn close_tab(&mut self, tab_index: usize) -> bool {
        if self.tabs.len() <= 1 {
            return false; // Can't close last tab
        }
        if tab_index < self.tabs.len() {
            self.tabs.remove(tab_index);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            true
        } else {
            false
        }
    }

    /// Close the currently active tab. Returns false if it's the last tab.
    pub fn close_active_tab(&mut self) -> bool {
        self.close_tab(self.active_tab)
    }

    /// Get the focused terminal (follows through Panel -> SurfaceGroup).
    pub fn active_terminal(&self) -> Option<&Terminal> {
        self.active_panel()?.focused_terminal()
    }

    /// Get the focused terminal (mutable).
    pub fn active_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.active_panel_mut()?.focused_terminal_mut()
    }

    /// Find a terminal by surface ID across all tabs (immutable).
    pub fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        for tab in &self.tabs {
            if let Some(panel) = tab.panel_if_initialized() {
                if let Some(t) = panel.find_terminal(surface_id) {
                    return Some(t);
                }
            }
        }
        None
    }

    /// Find a terminal by surface ID across all tabs (mutable).
    pub fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        for tab in &mut self.tabs {
            if let Some(t) = tab.panel_mut_if_initialized().and_then(|p| p.find_terminal_mut(surface_id)) {
                return Some(t);
            }
        }
        None
    }

    /// Switch to tab by index (0-based). Returns true if switched.
    pub fn goto_tab(&mut self, index: usize) -> bool {
        if index < self.tabs.len() && index != self.active_tab {
            self.active_tab = index;
            true
        } else {
            false
        }
    }

    /// Switch to next tab.
    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Switch to previous tab.
    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab = (self.active_tab + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// Add a Markdown viewer tab.
    pub fn add_markdown_tab(&mut self, tab_id: TabId, panel_id: u32, file_path: String) {
        let name = file_path
            .split(['/', '\\'])
            .last()
            .unwrap_or("Markdown")
            .to_string();
        let panel = Panel::Markdown(MarkdownPanel::new(panel_id, file_path));
        let tab = Tab {
            id: tab_id,
            name,
            panel_opt: Some(panel),
            deferred_spawn: None,
            explicit_name: None, deferred_surface_id: None,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// Add a file explorer tab.
    pub fn add_explorer_tab(&mut self, tab_id: TabId, panel_id: u32, root_path: String) {
        let name = root_path
            .split(['/', '\\'])
            .last()
            .unwrap_or("Explorer")
            .to_string();
        let panel = Panel::Explorer(ExplorerPanel::new(panel_id, root_path));
        let tab = Tab {
            id: tab_id,
            name,
            panel_opt: Some(panel),
            deferred_spawn: None,
            explicit_name: None, deferred_surface_id: None,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
    }

    /// Get the active tab (mutable). Returns None if tabs are empty.
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        if self.tabs.is_empty() {
            return None;
        }
        let idx = self.active_tab.min(self.tabs.len() - 1);
        Some(&mut self.tabs[idx])
    }

    /// Collect all terminals (mutable) from all tabs in this Pane.
    pub fn all_terminals_mut(&mut self) -> Vec<&mut Terminal> {
        let mut result = Vec::new();
        for tab in &mut self.tabs {
            if let Some(panel) = tab.panel_mut_if_initialized() {
                panel.collect_terminals_mut(&mut result);
            }
        }
        result
    }
}
