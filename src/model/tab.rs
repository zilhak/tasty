use tasty_terminal::Terminal;
use super::{SurfaceId, TerminalSurface, TabId};
use super::surface_trait::Surface;

pub struct Tab {
    pub id: TabId,
    /// Auto-generated name (e.g. "Shell"). Used as fallback when explicit_name is None.
    pub name: String,
    /// Explicitly set tab name. When Some, overrides the auto-generated name.
    pub explicit_name: Option<String>,
    /// The surface content of this tab. Temporarily `None` during structural mutations
    /// or when lazy_pty_init is enabled and the tab hasn't been focused yet.
    pub(crate) surface_opt: Option<Box<dyn Surface>>,
    /// When lazy_pty_init is enabled, stores parameters to spawn PTY on first access.
    pub(crate) deferred_spawn: Option<super::surface_group::DeferredSpawn>,
    /// Surface ID reserved for deferred spawn (set when lazy_pty_init creates the tab).
    #[allow(dead_code)]
    pub(crate) deferred_surface_id: Option<SurfaceId>,
}

impl Tab {
    /// Create a tab with a Surface trait object.
    pub fn new_with_surface(id: TabId, name: String, surface: Box<dyn Surface>) -> Self {
        Self {
            id,
            name,
            explicit_name: None,
            surface_opt: Some(surface),
            deferred_spawn: None,
            deferred_surface_id: None,
        }
    }

    /// Get the display name for this tab.
    /// Priority: explicit_name > auto-derived from focused surface CWD > fallback "name" field.
    pub fn display_name(&self) -> String {
        if let Some(ref explicit) = self.explicit_name {
            return explicit.clone();
        }
        // Try to derive name from the focused terminal's CWD
        let terminal = self.surface_opt.as_ref()
            .and_then(|s| s.focused_terminal());
        if let Some(terminal) = terminal {
            if let Some(cwd) = terminal.get_cwd() {
                let path_str = cwd.to_string_lossy();
                if let Some(home) = dirs_home() {
                    if cwd == home {
                        return "~".to_string();
                    }
                }
                if path_str == "/" {
                    return "/".to_string();
                }
                if let Some(name) = cwd.file_name() {
                    return name.to_string_lossy().to_string();
                }
            }
        }
        self.name.clone()
    }

    // ── Surface-based accessors ──

    /// Access the surface.
    #[track_caller]
    pub fn surface(&self) -> &dyn Surface {
        self.surface_opt.as_ref().map(|s| s.as_ref())
            .expect("BUG: no surface (deferred tab not initialized?)")
    }

    /// Access the surface mutably.
    #[track_caller]
    pub fn surface_mut(&mut self) -> &mut dyn Surface {
        self.surface_opt.as_mut().map(|s| s.as_mut())
            .expect("BUG: no surface (deferred tab not initialized?)")
    }

    /// Access the surface if initialized.
    pub fn surface_if_initialized(&self) -> Option<&dyn Surface> {
        self.surface_opt.as_ref().map(|s| s.as_ref())
    }

    /// Access the surface mutably if initialized.
    pub fn surface_mut_if_initialized(&mut self) -> Option<&mut dyn Surface> {
        match self.surface_opt {
            Some(ref mut s) => Some(s.as_mut()),
            None => None,
        }
    }

    /// Ensure the surface is initialized (lazy spawn if needed). Returns true if spawned.
    pub fn ensure_initialized(&mut self, surface_id: SurfaceId) -> bool {
        if self.surface_opt.is_some() || self.deferred_spawn.is_none() {
            return false;
        }
        let spawn = self.deferred_spawn.take().unwrap();
        let shell_ref = spawn.shell.as_deref();
        let shell_args: Vec<&str> = spawn.shell_args.iter().map(|s| s.as_str()).collect();
        let working_dir = spawn.working_dir.as_deref();
        match Terminal::new_with_shell_args_cwd(spawn.cols, spawn.rows, shell_ref, &shell_args, surface_id, spawn.waker, working_dir) {
            Ok(terminal) => {
                self.surface_opt = Some(Box::new(TerminalSurface {
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

    /// Returns true if this tab has a deferred spawn pending.
    pub fn is_deferred(&self) -> bool {
        self.surface_opt.is_none() && self.deferred_spawn.is_some()
    }

    /// Replace the surface.
    pub fn put_surface(&mut self, surface: Box<dyn Surface>) {
        self.surface_opt = Some(surface);
    }

    /// Split the focused surface within this tab. Creates a SurfaceGroup if needed.
    /// Moves focus to the new surface.
    pub fn split_focused_surface(
        &mut self,
        direction: super::SplitDirection,
        new_surface_id: super::SurfaceId,
        new_terminal: tasty_terminal::Terminal,
    ) {
        // Case 1: Already a SurfaceGroup — add to it
        if self.surface_mut().as_surface_group_mut().is_some() {
            let new_node = super::TerminalSurface { id: new_surface_id, terminal: new_terminal, deferred_spawn: None };
            let group = self.surface_mut().as_surface_group_mut().unwrap();
            let target = group.focused_surface;
            let old_layout = group.take_layout();
            let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
            group.put_layout(new_layout);
            group.focused_surface = new_surface_id;
            return;
        }

        // Case 2: Single TerminalSurface → wrap into SurfaceGroupNode
        if self.surface_opt.as_ref().is_some_and(|s| s.as_terminal_surface().is_some()) {
            let old_surface = self.surface_opt.take().unwrap();
            let old_node = old_surface.take_terminal_surface()
                .expect("BUG: as_terminal_surface() was Some but take failed");
            let old_surface_id = old_node.id;
            let group = super::SurfaceGroupNode {
                layout_opt: Some(super::SurfaceGroupLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(super::SurfaceGroupLayout::Leaf(Box::new(old_node))),
                    second: Box::new(super::SurfaceGroupLayout::Leaf(Box::new(TerminalSurface {
                        id: new_surface_id,
                        terminal: new_terminal,
                        deferred_spawn: None,
                    }))),
                    focus_second: true,
                }),
                focused_surface: new_surface_id,
                _first_surface: old_surface_id,
            };
            self.surface_opt = Some(Box::new(group));
        }
    }

    /// Split a specific surface by ID. Does NOT change focused_surface.
    pub fn split_surface_by_id(
        &mut self,
        target_surface_id: super::SurfaceId,
        direction: super::SplitDirection,
        new_surface_id: super::SurfaceId,
        new_terminal: tasty_terminal::Terminal,
    ) -> bool {
        // Already a SurfaceGroup — add to it
        if self.surface_mut().as_surface_group_mut().is_some() {
            let new_node = super::TerminalSurface { id: new_surface_id, terminal: new_terminal, deferred_spawn: None };
            let group = self.surface_mut().as_surface_group_mut().unwrap();
            let old_layout = group.take_layout();
            let (new_layout, remaining) = old_layout.split_with_node(target_surface_id, direction, new_node);
            group.put_layout(new_layout);
            return remaining.is_none();
        }

        // Single TerminalSurface → wrap into SurfaceGroupNode
        if self.surface_opt.as_ref().is_some_and(|s| s.as_terminal_surface().is_some_and(|n| n.id == target_surface_id)) {
            let old_surface = self.surface_opt.take().unwrap();
            let old_node = old_surface.take_terminal_surface()
                .expect("BUG: as_terminal_surface() was Some but take failed");
            let old_surface_id = old_node.id;
            let group = super::SurfaceGroupNode {
                layout_opt: Some(super::SurfaceGroupLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(super::SurfaceGroupLayout::Leaf(Box::new(old_node))),
                    second: Box::new(super::SurfaceGroupLayout::Leaf(Box::new(TerminalSurface {
                        id: new_surface_id,
                        terminal: new_terminal,
                        deferred_spawn: None,
                    }))),
                    focus_second: false,
                }),
                focused_surface: old_surface_id,
                _first_surface: old_surface_id,
            };
            self.surface_opt = Some(Box::new(group));
            return true;
        }

        false
    }

    /// Split a specific surface by ID with any surface type. Generic version of split_surface_by_id.
    pub fn split_surface_by_id_generic(
        &mut self,
        target_surface_id: super::SurfaceId,
        direction: super::SplitDirection,
        new_surface: Box<dyn super::Surface>,
    ) -> bool {
        // Already a SurfaceGroup — add to it
        if self.surface_mut().as_surface_group_mut().is_some() {
            let group = self.surface_mut().as_surface_group_mut().unwrap();
            let old_layout = group.take_layout();
            let (new_layout, remaining) = old_layout.split_with_surface(target_surface_id, direction, new_surface);
            group.put_layout(new_layout);
            if remaining.is_some() {
                tracing::warn!("split_surface_by_id_generic: target {} not found in group", target_surface_id);
            }
            return remaining.is_none();
        }

        // Single surface (any type) → wrap into SurfaceGroupNode
        if self.surface_opt.as_ref().is_some_and(|s| s.surface_id() == Some(target_surface_id)) {
            let old_surface = self.surface_opt.take().unwrap();
            let old_surface_id = old_surface.surface_id().unwrap_or(0);
            let group = super::SurfaceGroupNode {
                layout_opt: Some(super::SurfaceGroupLayout::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(super::SurfaceGroupLayout::Leaf(old_surface)),
                    second: Box::new(super::SurfaceGroupLayout::Leaf(new_surface)),
                    focus_second: false,
                }),
                focused_surface: old_surface_id,
                _first_surface: old_surface_id,
            };
            self.surface_opt = Some(Box::new(group));
            return true;
        }

        // For non-terminal single surfaces that don't match, check if it's a non-terminal
        // surface with the target ID inside it (shouldn't happen for single surfaces)
        if self.surface_opt.as_ref().is_some_and(|s| s.contains_surface(target_surface_id)) {
            tracing::warn!("split_surface_by_id_generic: unexpected containment for target {}", target_surface_id);
        }

        false
    }

    /// Produce a JSON tree representation of this tab.
    pub fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.display_name(),
            "surface": self.surface().to_tree_json(),
        })
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
