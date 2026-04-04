use tasty_terminal::Terminal;
use super::{Panel, SurfaceId, SurfaceNode, TabId};

pub struct Tab {
    pub id: TabId,
    pub name: String,
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations
    /// or when lazy_pty_init is enabled and the tab hasn't been focused yet.
    pub(crate) panel_opt: Option<Panel>,
    /// When lazy_pty_init is enabled, stores parameters to spawn PTY on first access.
    pub(crate) deferred_spawn: Option<super::surface_group::DeferredSpawn>,
    /// Surface ID reserved for deferred spawn (set when lazy_pty_init creates the tab).
    pub(crate) deferred_surface_id: Option<SurfaceId>,
}

impl Tab {
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
