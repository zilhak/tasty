use super::tab::Tab;
use super::{PaneId, SplitDirection, SurfaceId, TabId, TerminalSurface};
use tasty_terminal::{Terminal, Waker};
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
        Self {
            id: 0,
            tabs: Vec::new(),
            active_tab: 0,
            tab_scroll_offset: 0.0,
        }
    }
}

impl Pane {
    /// Create a Pane with a Surface trait object.
    pub fn new_with_surface(
        id: PaneId,
        tab_id: TabId,
        name: String,
        surface: Box<dyn super::Surface>,
    ) -> Self {
        let tab = super::tab::Tab::new_with_surface(tab_id, name, surface);
        Self {
            id,
            tabs: vec![tab],
            active_tab: 0,
            tab_scroll_offset: 0.0,
        }
    }

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
        let terminal = Terminal::new_with_shell_args_cwd(
            cols,
            rows,
            shell,
            shell_args,
            surface_id,
            waker,
            working_dir,
        )?;
        let surface: Box<dyn super::Surface> = Box::new(TerminalSurface {
            id: surface_id,
            terminal,
            deferred_spawn: None,
        });
        let tab = Tab::new_with_surface(tab_id, "Shell".to_string(), surface);
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
        let terminal = Terminal::new_with_shell_args_cwd(
            cols,
            rows,
            shell,
            shell_args,
            surface_id,
            waker,
            working_dir,
        )?;
        let surface: Box<dyn super::Surface> = Box::new(TerminalSurface {
            id: surface_id,
            terminal,
            deferred_spawn: None,
        });
        let tab = Tab::new_with_surface(tab_id, "Shell".to_string(), surface);
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
        let terminal = Terminal::new_with_shell_args_cwd(
            cols,
            rows,
            shell,
            shell_args,
            surface_id,
            waker,
            working_dir,
        )?;
        let surface: Box<dyn super::Surface> = Box::new(TerminalSurface {
            id: surface_id,
            terminal,
            deferred_spawn: None,
        });
        let tab = Tab::new_with_surface(tab_id, "Shell".to_string(), surface);
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
            deferred_spawn: Some(super::surface_group::DeferredSpawn {
                shell: shell.map(|s| s.to_string()),
                shell_args: shell_args.iter().map(|s| s.to_string()).collect(),
                cols,
                rows,
                waker,
                working_dir: working_dir.map(|p| p.to_path_buf()),
            }),
            layout_opt: None,
            focused_surface: 0,
            explicit_name: None,
            deferred_surface_id: Some(surface_id),
        };
        self.tabs.push(tab);
    }

    /// Collect all surface IDs across all tabs in this pane.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        let mut ids = Vec::new();
        for tab in &self.tabs {
            ids.extend(tab.all_surface_ids());
        }
        ids
    }

    /// Ensure the active tab is initialized (lazy PTY spawn). Returns true if spawned.
    pub fn ensure_active_tab_initialized(&mut self, surface_id: SurfaceId) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
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
        let new_terminal = Terminal::new_with_shell_args_cwd(
            cols,
            rows,
            shell,
            shell_args,
            new_surface_id,
            waker,
            working_dir,
        )?;
        if self.tabs.is_empty() {
            return Ok(()); // nothing to split
        }
        let active = self.active_tab.min(self.tabs.len() - 1);
        self.tabs[active].split_focused_surface(direction, new_surface_id, new_terminal);
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
        let new_terminal = Terminal::new_with_shell_args_cwd(
            cols,
            rows,
            shell,
            shell_args,
            new_surface_id,
            waker,
            working_dir,
        )?;
        for tab in &mut self.tabs {
            if tab.find_terminal(target_surface_id).is_some() {
                tab.split_surface_by_id(target_surface_id, direction, new_surface_id, new_terminal);
                return Ok(());
            }
        }
        anyhow::bail!("surface {} not found in this pane", target_surface_id)
    }

    /// Split a specific surface by ID with any surface type (not just terminal).
    pub fn split_surface_by_id_with_surface(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface: Box<dyn super::Surface>,
    ) -> anyhow::Result<()> {
        for tab in &mut self.tabs {
            if tab.contains_surface(target_surface_id) {
                tab.split_surface_by_id_generic(target_surface_id, direction, new_surface);
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

    /// Close a tab by its ID. Returns false if not found or it's the last tab.
    pub fn close_tab_by_id(&mut self, tab_id: TabId) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) {
            self.tabs.remove(idx);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            }
            true
        } else {
            false
        }
    }

    /// Get the focused terminal.
    pub fn active_terminal(&self) -> Option<&Terminal> {
        let tab = self
            .tabs
            .get(self.active_tab.min(self.tabs.len().saturating_sub(1)))?;
        tab.focused_terminal()
    }

    /// Get the focused terminal (mutable).
    pub fn active_terminal_mut(&mut self) -> Option<&mut Terminal> {
        let idx = self.active_tab.min(self.tabs.len().saturating_sub(1));
        let tab = self.tabs.get_mut(idx)?;
        tab.focused_terminal_mut()
    }

    /// Check if any tab in this pane contains the given surface ID.
    pub fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        self.tabs.iter().any(|tab| tab.contains_surface(surface_id))
    }

    /// Find a terminal by surface ID across all tabs (immutable).
    pub fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        for tab in &self.tabs {
            if let Some(t) = tab.find_terminal(surface_id) {
                return Some(t);
            }
        }
        None
    }

    /// Find a terminal by surface ID across all tabs (mutable).
    pub fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        for tab in &mut self.tabs {
            if let Some(t) = tab.find_terminal_mut(surface_id) {
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

    /// Add a tab with a Surface trait object and switch to it.
    pub fn add_surface_tab(
        &mut self,
        tab_id: TabId,
        name: String,
        surface: Box<dyn super::Surface>,
    ) {
        let tab = super::tab::Tab::new_with_surface(tab_id, name, surface);
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
            tab.collect_terminals_mut(&mut result);
        }
        result
    }

    /// Produce a JSON tree representation of this pane.
    pub fn to_tree_json(&self) -> serde_json::Value {
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let mut t = tab.to_tree_json();
                t["active"] = serde_json::json!(i == self.active_tab);
                t
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "tabs": tabs,
        })
    }
}
