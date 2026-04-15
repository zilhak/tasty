use tasty_terminal::Terminal;
use super::{Panel, PanelBehavior, SurfaceId, TerminalSurface, TabId};
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
    /// Legacy panel (kept during migration, will be removed).
    pub(crate) panel_opt: Option<Panel>,
    /// When lazy_pty_init is enabled, stores parameters to spawn PTY on first access.
    pub(crate) deferred_spawn: Option<super::surface_group::DeferredSpawn>,
    /// Surface ID reserved for deferred spawn (set when lazy_pty_init creates the tab).
    #[allow(dead_code)]
    pub(crate) deferred_surface_id: Option<SurfaceId>,
}

impl Tab {
    /// Create a tab with a pre-built panel (legacy, used by restore).
    pub fn new_with_panel(id: TabId, name: String, panel: Panel) -> Self {
        Self {
            id,
            name,
            explicit_name: None,
            surface_opt: None,
            panel_opt: Some(panel),
            deferred_spawn: None,
            deferred_surface_id: None,
        }
    }

    /// Create a tab with a Surface trait object.
    pub fn new_with_surface(id: TabId, name: String, surface: Box<dyn Surface>) -> Self {
        Self {
            id,
            name,
            explicit_name: None,
            surface_opt: Some(surface),
            panel_opt: None,
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
        let terminal = if let Some(surface) = self.surface_opt.as_ref() {
            surface.focused_terminal()
        } else if let Some(panel) = self.panel_opt.as_ref() {
            // Legacy path — disambiguate between PanelBehavior and Surface
            PanelBehavior::focused_terminal(panel)
        } else {
            None
        };
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

    /// Access the surface. Falls back to legacy panel.
    #[track_caller]
    pub fn surface(&self) -> &dyn Surface {
        if let Some(s) = self.surface_opt.as_ref() {
            return s.as_ref();
        }
        // Legacy fallback
        self.panel_opt.as_ref().expect("BUG: no surface or panel")
    }

    /// Access the surface mutably. Falls back to legacy panel.
    #[track_caller]
    pub fn surface_mut(&mut self) -> &mut dyn Surface {
        if let Some(s) = self.surface_opt.as_mut() {
            return s.as_mut();
        }
        // Legacy fallback
        self.panel_opt.as_mut().expect("BUG: no surface or panel")
    }

    /// Access the surface if initialized.
    pub fn surface_if_initialized(&self) -> Option<&dyn Surface> {
        if let Some(s) = self.surface_opt.as_ref() {
            return Some(s.as_ref());
        }
        self.panel_opt.as_ref().map(|p| p as &dyn Surface)
    }

    /// Access the surface mutably if initialized.
    pub fn surface_mut_if_initialized(&mut self) -> Option<&mut dyn Surface> {
        if let Some(s) = self.surface_opt.as_mut() {
            return Some(s.as_mut());
        }
        self.panel_opt.as_mut().map(|p| p as &mut dyn Surface)
    }

    // ── Legacy Panel accessors (to be removed after full migration) ──

    /// Access the panel. If lazy init is pending, spawns the terminal first.
    #[track_caller]
    pub fn panel(&self) -> &Panel {
        self.panel_opt.as_ref().expect("BUG: panel accessed during structural mutation or before lazy init")
    }

    /// Ensure the panel is initialized (lazy spawn if needed). Returns true if spawned.
    pub fn ensure_initialized(&mut self, surface_id: SurfaceId) -> bool {
        if self.panel_opt.is_some() || self.surface_opt.is_some() || self.deferred_spawn.is_none() {
            return false;
        }
        let spawn = self.deferred_spawn.take().unwrap();
        let shell_ref = spawn.shell.as_deref();
        let shell_args: Vec<&str> = spawn.shell_args.iter().map(|s| s.as_str()).collect();
        let working_dir = spawn.working_dir.as_deref();
        match Terminal::new_with_shell_args_cwd(spawn.cols, spawn.rows, shell_ref, &shell_args, surface_id, spawn.waker, working_dir) {
            Ok(terminal) => {
                self.panel_opt = Some(Panel::Terminal(TerminalSurface {
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
        self.panel_opt.is_none() && self.surface_opt.is_none() && self.deferred_spawn.is_some()
    }

    /// Access the panel mutably.
    #[track_caller]
    pub fn panel_mut(&mut self) -> &mut Panel {
        self.panel_opt.as_mut().expect("BUG: panel accessed during structural mutation (between take/put)")
    }

    /// Take ownership of the panel for structural mutations.
    #[track_caller]
    pub(crate) fn take_panel(&mut self) -> Panel {
        self.panel_opt.take().expect("BUG: panel already taken")
    }

    /// Put the panel back after structural mutations.
    pub(crate) fn put_panel(&mut self, panel: Panel) {
        self.panel_opt = Some(panel);
    }

    /// Replace the surface (drops both panel_opt and surface_opt, sets surface_opt).
    pub fn put_surface(&mut self, surface: Box<dyn Surface>) {
        self.panel_opt = None;
        self.surface_opt = Some(surface);
    }
}

/// Implement Surface for Panel enum (legacy bridge).
/// This allows Panel to be used as &dyn Surface during migration.
impl Surface for Panel {
    fn type_name(&self) -> &'static str {
        use super::PanelBehavior;
        PanelBehavior::type_name(self)
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        use super::PanelBehavior;
        PanelBehavior::surface_id(self)
    }
    fn all_surface_ids(&self) -> Vec<SurfaceId> {
        use super::PanelBehavior;
        PanelBehavior::all_surface_ids(self)
    }
    fn focused_surface_id(&self) -> Option<SurfaceId> {
        use super::PanelBehavior;
        PanelBehavior::focused_surface_id(self)
    }
    fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        use super::PanelBehavior;
        PanelBehavior::contains_surface(self, surface_id)
    }
    fn has_terminal(&self) -> bool {
        use super::PanelBehavior;
        PanelBehavior::has_terminal(self)
    }
    fn focused_terminal(&self) -> Option<&Terminal> {
        use super::PanelBehavior;
        PanelBehavior::focused_terminal(self)
    }
    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        use super::PanelBehavior;
        PanelBehavior::focused_terminal_mut(self)
    }
    fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        use super::PanelBehavior;
        PanelBehavior::find_terminal(self, surface_id)
    }
    fn find_terminal_surface(&self, surface_id: SurfaceId) -> Option<&TerminalSurface> {
        use super::PanelBehavior;
        PanelBehavior::find_terminal_node(self, surface_id)
    }
    fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        use super::PanelBehavior;
        PanelBehavior::find_terminal_mut(self, surface_id)
    }
    fn render_regions(&self, rect: super::Rect) -> Vec<(SurfaceId, &Terminal, super::Rect)> {
        use super::PanelBehavior;
        PanelBehavior::render_regions(self, rect)
    }
    fn resize_all(&mut self, rect: super::Rect, cell_width: f32, cell_height: f32) {
        use super::PanelBehavior;
        PanelBehavior::resize_all(self, rect, cell_width, cell_height)
    }
    fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        use super::PanelBehavior;
        PanelBehavior::collect_terminals_mut(self, out)
    }
    fn for_each_terminal_mut(&mut self, f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {
        // Bridge: PanelBehavior uses generic, Surface uses dyn
        use super::PanelBehavior;
        PanelBehavior::for_each_terminal_mut(self, f)
    }
    fn as_surface_group(&self) -> Option<&super::SurfaceGroupNode> {
        Panel::as_surface_group(self)
    }
    fn as_surface_group_mut(&mut self) -> Option<&mut super::SurfaceGroupNode> {
        Panel::as_surface_group_mut(self)
    }
    fn as_terminal_surface(&self) -> Option<&super::TerminalSurface> {
        match self { Panel::Terminal(node) => Some(node), _ => None }
    }
    fn as_terminal_surface_mut(&mut self) -> Option<&mut super::TerminalSurface> {
        match self { Panel::Terminal(node) => Some(node), _ => None }
    }
    fn as_markdown(&self) -> Option<&super::MarkdownPanel> {
        match self { Panel::Markdown(md) => Some(md), _ => None }
    }
    fn as_markdown_mut(&mut self) -> Option<&mut super::MarkdownPanel> {
        match self { Panel::Markdown(md) => Some(md), _ => None }
    }
    fn as_explorer(&self) -> Option<&super::ExplorerPanel> {
        match self { Panel::Explorer(ex) => Some(ex), _ => None }
    }
    fn as_explorer_mut(&mut self) -> Option<&mut super::ExplorerPanel> {
        match self { Panel::Explorer(ex) => Some(ex), _ => None }
    }
    fn as_html(&self) -> Option<&super::HtmlPanel> {
        match self { Panel::Html(html) => Some(html), _ => None }
    }
    fn as_html_mut(&mut self) -> Option<&mut super::HtmlPanel> {
        match self { Panel::Html(html) => Some(html), _ => None }
    }
    fn as_empty_surface(&self) -> Option<&super::EmptySurface> { None }
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
