use tasty_terminal::Terminal;
use super::{Panel, SurfaceId, SurfaceNode, TabId};

pub struct Tab {
    pub id: TabId,
    /// Auto-generated name (e.g. "Shell"). Used as fallback when explicit_name is None.
    pub name: String,
    /// Explicitly set tab name. When Some, overrides the auto-generated name.
    pub explicit_name: Option<String>,
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations
    /// or when lazy_pty_init is enabled and the tab hasn't been focused yet.
    pub(crate) panel_opt: Option<Panel>,
    /// When lazy_pty_init is enabled, stores parameters to spawn PTY on first access.
    pub(crate) deferred_spawn: Option<super::surface_group::DeferredSpawn>,
    /// Surface ID reserved for deferred spawn (set when lazy_pty_init creates the tab).
    #[allow(dead_code)]
    pub(crate) deferred_surface_id: Option<SurfaceId>,
}

impl Tab {
    /// Get the display name for this tab.
    /// Priority: explicit_name > auto-derived from focused surface CWD > fallback "name" field.
    pub fn display_name(&self) -> String {
        if let Some(ref explicit) = self.explicit_name {
            return explicit.clone();
        }
        // Try to derive name from the focused terminal's CWD
        if let Some(panel) = self.panel_opt.as_ref() {
            if let Some(terminal) = match panel {
                Panel::Terminal(node) => Some(&node.terminal),
                Panel::SurfaceGroup(group) => group.layout().find_terminal(group.focused_surface),
                _ => None,
            } {
                if let Some(cwd) = terminal.get_cwd() {
                    let path_str = cwd.to_string_lossy();
                    // Home directory → "~"
                    if let Some(home) = dirs_home() {
                        if cwd == home {
                            return "~".to_string();
                        }
                    }
                    // Root → "/"
                    if path_str == "/" {
                        return "/".to_string();
                    }
                    // Otherwise → last component (folder name)
                    if let Some(name) = cwd.file_name() {
                        return name.to_string_lossy().to_string();
                    }
                }
            }
        }
        self.name.clone()
    }


    /// Access the panel. If lazy init is pending, spawns the terminal first.
    #[track_caller]
    pub fn panel(&self) -> &Panel {
        self.panel_opt.as_ref().expect("BUG: panel accessed during structural mutation or before lazy init")
    }

    /// Ensure the panel is initialized (lazy spawn if needed). Returns true if spawned.
    pub fn ensure_initialized(&mut self, surface_id: SurfaceId) -> bool {
        if self.panel_opt.is_some() || self.deferred_spawn.is_none() {
            return false;
        }
        let spawn = self.deferred_spawn.take().unwrap();
        let shell_ref = spawn.shell.as_deref();
        let shell_args: Vec<&str> = spawn.shell_args.iter().map(|s| s.as_str()).collect();
        let working_dir = spawn.working_dir.as_deref();
        match Terminal::new_with_shell_args_cwd(spawn.cols, spawn.rows, shell_ref, &shell_args, surface_id, spawn.waker, working_dir) {
            Ok(terminal) => {
                self.panel_opt = Some(Panel::Terminal(SurfaceNode {
                    id: surface_id,
                    terminal,
                    deferred_spawn: None,
                }));
                true
            }
            Err(e) => {
                tracing::error!("lazy PTY init failed: {e}");
                false
            }
        }
    }

    /// Access the panel if already initialized. Returns None for deferred tabs.
    pub fn panel_if_initialized(&self) -> Option<&Panel> {
        self.panel_opt.as_ref()
    }

    /// Access the panel mutably if already initialized. Returns None for deferred tabs.
    pub fn panel_mut_if_initialized(&mut self) -> Option<&mut Panel> {
        self.panel_opt.as_mut()
    }

    /// Returns true if this tab has a deferred spawn pending.
    pub fn is_deferred(&self) -> bool {
        self.panel_opt.is_none() && self.deferred_spawn.is_some()
    }

    /// Access the panel mutably.
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn panel_mut(&mut self) -> &mut Panel {
        self.panel_opt.as_mut().expect("BUG: panel accessed during structural mutation (between take/put)")
    }

    /// Take ownership of the panel for structural mutations.
    /// MUST be followed by `put_panel()`. Panics if already taken.
    #[track_caller]
    pub(crate) fn take_panel(&mut self) -> Panel {
        self.panel_opt.take().expect("BUG: panel already taken")
    }

    /// Put the panel back after structural mutations.
    pub(crate) fn put_panel(&mut self, panel: Panel) {
        self.panel_opt = Some(panel);
    }
}

fn dirs_home() -> Option<std::path::PathBuf> {
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(std::path::PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").ok().map(std::path::PathBuf::from)
    }
}
