use std::collections::{HashMap, HashSet};

use tasty_hooks::HookManager;
use crate::global_hooks::GlobalHookManager;
use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::settings::Settings;
use crate::state::{ClaudeChildEntry, SurfaceMessage};
use tasty_terminal::{Terminal, TerminalEvent, Waker};

/// ID generator for workspaces, panes, tabs, and surfaces.
pub struct IdGenerator {
    workspace: u32,
    pane: u32,
    tab: u32,
    surface: u32,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self {
            workspace: 1,
            pane: 1,
            tab: 1,
            surface: 1,
        }
    }

    pub fn next_workspace(&mut self) -> u32 {
        let id = self.workspace;
        self.workspace += 1;
        id
    }

    pub fn next_pane(&mut self) -> u32 {
        let id = self.pane;
        self.pane += 1;
        id
    }

    pub fn next_tab(&mut self) -> u32 {
        let id = self.tab;
        self.tab += 1;
        id
    }

    pub fn next_surface(&mut self) -> u32 {
        let id = self.surface;
        self.surface += 1;
        id
    }
}

/// Engine-level state shared across all windows.
/// Contains all data that is not specific to a single window's UI.
pub struct EngineState {
    // ── Workspace / Terminal management ──
    pub workspaces: Vec<Workspace>,
    pub next_ids: IdGenerator,
    pub default_cols: usize,
    pub default_rows: usize,
    pub waker: Waker,

    // ── Settings ──
    pub settings: Settings,

    // ── Notifications / Hooks ──
    pub notifications: NotificationStore,
    pub hook_manager: HookManager,
    pub global_hook_manager: GlobalHookManager,

    // ── Claude agent relationships ──
    pub claude_parent_children: HashMap<u32, Vec<ClaudeChildEntry>>,
    pub claude_child_parent: HashMap<u32, u32>,
    pub claude_closed_parents: HashSet<u32>,
    pub(crate) claude_next_child_index: HashMap<u32, u32>,
    pub claude_idle_state: HashMap<u32, bool>,
    pub claude_needs_input_state: HashMap<u32, bool>,

    // ── Messaging / Typing detection ──
    pub surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    pub(crate) surface_next_message_id: u32,
    pub last_key_input: HashMap<u32, std::time::Instant>,

    /// Event loop proxy for targeted waker creation. Set by App after EngineState creation.
    pub waker_factory: Option<winit::event_loop::EventLoopProxy<crate::AppEvent>>,
}

impl EngineState {
    /// Create a new EngineState with default settings.
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let settings = Settings::load();
        let mut next_ids = IdGenerator::new();
        let ws_id = next_ids.next_workspace();
        let pane_id = next_ids.next_pane();
        let tab_id = next_ids.next_tab();
        let surface_id = next_ids.next_surface();

        let shell = if settings.general.shell.is_empty() { None } else { Some(settings.general.shell.as_str()) };
        let shell_args_owned = settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let ws = Workspace::new_with_shell(ws_id, "Workspace 1".to_string(), cols, rows, pane_id, tab_id, surface_id, shell, &shell_args, waker.clone(), None)?;

        let mut engine = Self {
            workspaces: vec![ws],
            next_ids,
            default_cols: cols,
            default_rows: rows,
            waker,
            settings,
            notifications: NotificationStore::with_coalesce_ms(500),
            hook_manager: HookManager::new(),
            global_hook_manager: GlobalHookManager::new(),
            claude_parent_children: HashMap::new(),
            claude_child_parent: HashMap::new(),
            claude_closed_parents: HashSet::new(),
            claude_next_child_index: HashMap::new(),
            claude_idle_state: HashMap::new(),
            claude_needs_input_state: HashMap::new(),
            surface_messages: HashMap::new(),
            surface_next_message_id: 0,
            last_key_input: HashMap::new(),
            waker_factory: None,
        };

        // Re-apply coalesce_ms from actual settings
        engine.notifications = NotificationStore::with_coalesce_ms(engine.settings.notification.coalesce_ms);

        // Init fast mode + scrollback for the first surface
        engine.send_fast_init(surface_id);

        Ok(engine)
    }

    /// Send fast-mode init command to a terminal by surface ID and apply scrollback limit.
    /// Create a waker for a terminal. If targeted_pty_polling is enabled,
    /// the waker includes the surface_id so only that terminal is processed.
    /// Otherwise, returns the shared waker (all terminals polled).
    pub fn make_waker(&self, surface_id: u32) -> Waker {
        if self.settings.performance.targeted_pty_polling {
            // Import the proxy-based waker creation from the base waker
            // The base waker sends TerminalOutput(None). We create one that sends TerminalOutput(Some(id)).
            // We need access to the EventLoopProxy, which is captured in self.waker.
            // However, self.waker is a generic Fn(), not tied to proxy.
            // So we store a waker_factory alongside waker.
            if let Some(factory) = &self.waker_factory {
                let proxy = factory.clone();
                let sid = surface_id;
                std::sync::Arc::new(move || {
                    let _ = proxy.send_event(crate::AppEvent::TerminalOutput(Some(sid)));
                })
            } else {
                self.waker.clone()
            }
        } else {
            self.waker.clone()
        }
    }

    pub fn send_fast_init(&mut self, surface_id: u32) {
        crate::surface_meta::SurfaceMetaStore::ensure_created(surface_id);
        let scrollback_limit = self.settings.general.scrollback_lines;
        let disk_swap = self.settings.performance.scrollback_disk_swap;
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.set_scrollback_limit(scrollback_limit);
            if disk_swap {
                terminal.enable_disk_scrollback(surface_id);
            }
        }
        if let Some(cmd) = self.settings.general.fast_mode_init_command() {
            if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(&cmd);
            }
        }
    }

    /// Record that the user typed on the given surface.
    pub fn record_typing(&mut self, surface_id: u32) {
        self.last_key_input.insert(surface_id, std::time::Instant::now());
    }

    /// Returns true if the surface received key input within the last 5 seconds.
    pub fn is_typing(&self, surface_id: u32) -> bool {
        if let Some(last) = self.last_key_input.get(&surface_id) {
            last.elapsed().as_secs_f64() < 5.0
        } else {
            false
        }
    }

    /// Find a terminal by surface ID (immutable).
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        for workspace in &self.workspaces {
            let layout = workspace.pane_layout();
            if let Some(t) = Self::find_terminal_in_layout(layout, surface_id) {
                return Some(t);
            }
        }
        None
    }

    /// Find a terminal by surface ID (mutable).
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        for workspace in &mut self.workspaces {
            let layout = workspace.pane_layout_mut();
            if let Some(t) = Self::find_terminal_in_layout_mut(layout, surface_id) {
                return Some(t);
            }
        }
        None
    }

    fn find_terminal_in_layout(layout: &crate::model::PaneNode, surface_id: u32) -> Option<&Terminal> {
        match layout {
            crate::model::PaneNode::Leaf(pane) => pane.find_terminal(surface_id),
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::find_terminal_in_layout(first, surface_id)
                    .or_else(|| Self::find_terminal_in_layout(second, surface_id))
            }
        }
    }

    fn find_terminal_in_layout_mut(layout: &mut crate::model::PaneNode, surface_id: u32) -> Option<&mut Terminal> {
        match layout {
            crate::model::PaneNode::Leaf(pane) => pane.find_terminal_mut(surface_id),
            crate::model::PaneNode::Split { first, second, .. } => {
                if let Some(t) = Self::find_terminal_in_layout_mut(first, surface_id) {
                    return Some(t);
                }
                Self::find_terminal_in_layout_mut(second, surface_id)
            }
        }
    }

    /// Process all terminals (read PTY output).
    pub fn process_all(&mut self) -> bool {
        let mut any = false;
        for ws in &mut self.workspaces {
            if ws.pane_layout_mut().process_all() {
                any = true;
            }
        }
        any
    }

    /// Process a single terminal by surface ID (read PTY output).
    /// Returns true if data was processed.
    pub fn process_surface(&mut self, surface_id: u32) -> bool {
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.process()
        } else {
            false
        }
    }

    /// Collect events from all terminals.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for workspace in &mut self.workspaces {
            workspace.pane_layout_mut().for_each_terminal_mut(&mut |sid, terminal| {
                let mut events = terminal.take_events();
                for event in &mut events {
                    event.surface_id = sid;
                }
                all_events.extend(events);
            });
        }
        all_events
    }
}
