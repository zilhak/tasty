use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::global_hooks::GlobalHookManager;
use crate::model::Workspace;
use crate::notification::NotificationStore;
use crate::settings::Settings;
use crate::state::{ClaudeChildEntry, SurfaceMessage};
use crate::surface_registry::SurfaceKindRegistry;
use tasty_hooks::HookManager;
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

/// All Claude agent relationship state.
pub struct ClaudeState {
    pub parent_children: HashMap<u32, Vec<ClaudeChildEntry>>,
    pub child_parent: HashMap<u32, u32>,
    pub closed_parents: HashSet<u32>,
    pub(crate) next_child_index: HashMap<u32, u32>,
    pub idle_state: HashMap<u32, bool>,
    pub needs_input_state: HashMap<u32, bool>,
    /// Maps (parent_surface_id, workspace_id) → spawn_pane_id for --workspace spawning.
    pub spawn_panes: HashMap<(u32, u32), u32>,
    /// Surfaces for which the ClaudeError PTY scanner should run on every redraw.
    /// Populated automatically when a Claude child is spawned via
    /// `claude.spawn` / `claude.launch`; never populated for plain shells.
    pub error_scan_enabled: HashSet<u32>,
}

impl ClaudeState {
    pub fn new() -> Self {
        Self {
            parent_children: HashMap::new(),
            child_parent: HashMap::new(),
            closed_parents: HashSet::new(),
            next_child_index: HashMap::new(),
            idle_state: HashMap::new(),
            needs_input_state: HashMap::new(),
            spawn_panes: HashMap::new(),
            error_scan_enabled: HashSet::new(),
        }
    }
}

/// Helper to extract shell configuration from settings, avoiding boilerplate.
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
}

impl ShellConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            shell: settings.general.shell.clone(),
            args: settings.general.effective_shell_args(),
        }
    }

    pub fn shell_ref(&self) -> Option<&str> {
        if self.shell.is_empty() {
            None
        } else {
            Some(&self.shell)
        }
    }

    pub fn args_ref(&self) -> Vec<&str> {
        self.args.iter().map(|s| s.as_str()).collect()
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
    pub claude: ClaudeState,

    // ── Closed item history ──
    pub closed_items: crate::model::ClosedItemStore,

    // ── System clipboard history (memory-only) ──
    pub clipboard_history: crate::clipboard_history::ClipboardHistory,

    // ── Messaging / Typing detection ──
    pub surface_messages: HashMap<u32, Vec<SurfaceMessage>>,
    pub(crate) surface_next_message_id: u32,
    pub last_key_input: HashMap<u32, std::time::Instant>,

    // ── Busy state cache (foreground process != shell). Updated by BusyPoll.
    // Set membership = busy. Surfaces missing from the set are treated as idle.
    pub busy_surfaces: std::collections::HashSet<u32>,

    /// Targeted waker creation. winit `EventLoopProxy`를 직접 들지 않고 trait 뒤로
    /// 추상화하여 헤드리스/플러그인 호스트 컨텍스트에서도 동일 인터페이스를 쓴다.
    /// `App`이 EngineState 생성 후 본체에서 `WinitWakerFactory`를 주입한다.
    pub waker_factory: Option<tasty_core::SharedWakerFactory>,

    // ── CWD polling (round-robin) ──
    // macOS/Linux 전용. Windows에서는 폴링을 돌지 않아 필드 자체가 없음.
    // ── Surface kind registry ──
    /// Surface 종류별 메타·동작 lookup. 단계 03C에서는 빈 레지스트리만 보유한다 —
    /// 03D에서 본체 7종이 등록되며, 단계 05에서 plugin이 추가될 예정.
    pub surface_registry: Arc<SurfaceKindRegistry>,

    // ── Layout persistence ──
    pub layout_dirty: crate::layout_persistence::LayoutDirtyTracker,
    /// Active workspace index restored from layout.json. Consumed once by AppState::new().
    pub restored_active_workspace: Option<usize>,
    /// Restore commands queued during layout restore. (surface_id, command).
    /// Consumed by AppState after shell initialization.
    pub pending_restore_commands: Vec<(u32, String)>,
    /// 첫 plugin pump 후 적용할 layout. plugin이 제공하는 surface kind가
    /// 등록되기 전에 복원하면 사라지므로 한 번 미뤄둔다. `App::apply_pending_layout_restore`가 소비.
    pub pending_layout_restore: Option<crate::layout_persistence::SavedLayout>,

    /// Whether input simulation IPC is enabled (debug builds only, --enable-input-simulation).
    #[cfg(debug_assertions)]
    pub input_simulation_enabled: bool,
}

impl EngineState {
    /// Create a new EngineState with default settings.
    pub fn new(cols: usize, rows: usize, waker: Waker) -> anyhow::Result<Self> {
        let settings = Settings::load();
        let restore_layout = settings.general.restore_layout;

        // Create engine with empty workspaces first; we'll fill them below.
        let mut engine = Self {
            workspaces: Vec::new(),
            next_ids: IdGenerator::new(),
            default_cols: cols,
            default_rows: rows,
            waker: waker.clone(),
            settings,
            notifications: NotificationStore::with_coalesce_ms(500),
            hook_manager: HookManager::new(),
            global_hook_manager: GlobalHookManager::new(),
            claude: ClaudeState::new(),
            closed_items: crate::model::ClosedItemStore::new(),
            clipboard_history: crate::clipboard_history::ClipboardHistory::new(100),
            surface_messages: HashMap::new(),
            surface_next_message_id: 0,
            last_key_input: HashMap::new(),
            busy_surfaces: std::collections::HashSet::new(),
            waker_factory: None,
            surface_registry: {
                let reg = SurfaceKindRegistry::new();
                crate::surface_registry::register_builtin_kinds(&reg);
                Arc::new(reg)
            },
            layout_dirty: crate::layout_persistence::LayoutDirtyTracker::new(),
            restored_active_workspace: None,
            pending_restore_commands: Vec::new(),
            pending_layout_restore: None,
            #[cfg(debug_assertions)]
            input_simulation_enabled: false,
        };

        // Re-apply coalesce_ms from actual settings
        engine.notifications =
            NotificationStore::with_coalesce_ms(engine.settings.notification.coalesce_ms);

        // Apply clipboard history max from settings.
        engine
            .clipboard_history
            .set_max(engine.settings.clipboard.history_max);

        // Try restoring saved layout. plugin이 제공하는 surface kind(예: explorer)는
        // PluginManager가 hello를 처리한 후에야 registry에 등록되므로, 여기서 즉시
        // 복원하면 그런 surface가 사라진다. 따라서 layout 복원은 첫 plugin pump 후로
        // 지연한다 (`App::apply_pending_layout_restore`).
        let mut restored = false;
        if restore_layout {
            if let Some(saved) = crate::layout_persistence::load_from_disk() {
                engine.pending_layout_restore = Some(saved);
                // 첫 화면이 비지 않도록 default workspace는 일단 fallback에서 만들고,
                // pending_layout_restore가 적용될 때 교체된다.
                restored = false;
            }
        }

        // Fallback: create default workspace
        if !restored {
            let ws_id = engine.next_ids.next_workspace();
            let pane_id = engine.next_ids.next_pane();
            let tab_id = engine.next_ids.next_tab();
            let surface_id = engine.next_ids.next_surface();
            let sh = ShellConfig::from_settings(&engine.settings);
            let ws = Workspace::new_with_shell(
                ws_id,
                "Workspace 1".to_string(),
                cols,
                rows,
                pane_id,
                tab_id,
                surface_id,
                sh.shell_ref(),
                &sh.args_ref(),
                waker,
                None,
            )?;
            engine.workspaces = vec![ws];
            engine.send_fast_init(surface_id);
        }

        Ok(engine)
    }

    /// Send fast-mode init command to a terminal by surface ID and apply scrollback limit.
    /// Create a waker for a terminal. If targeted_pty_polling is enabled,
    /// the waker includes the surface_id so only that terminal is processed.
    /// Otherwise, returns the shared waker (all terminals polled).
    pub fn make_waker(&self, surface_id: u32) -> Waker {
        // targeted_pty_polling이 켜져 있고 factory가 주입되어 있으면 surface별 waker 생성.
        // 그 외에는 EngineState 생성 시 받은 base waker(`TerminalOutput(None)`)를 그대로 공유.
        if self.settings.performance.targeted_pty_polling {
            if let Some(factory) = &self.waker_factory {
                return factory.make_targeted_waker(surface_id);
            }
        }
        self.waker.clone()
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
        if let Some(cmd) = self.settings.general.tasty_mode_init_command() {
            if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(&cmd);
            }
        }
    }

    /// Push a closed item, automatically injecting restore commands from surface metadata.
    pub fn push_closed_item(&mut self, mut item: crate::model::ClosedItem) {
        crate::model::closed_item::inject_restore_commands(&mut item, &|sid| {
            crate::surface_meta::SurfaceMetaStore::get(sid, "claude-session-id")
                .map(|session_id| format!("claude -r {}", session_id))
        });
        self.closed_items.push(item);
    }

    /// Record that the user typed on the given surface.
    pub fn record_typing(&mut self, surface_id: u32) {
        self.last_key_input
            .insert(surface_id, std::time::Instant::now());
    }

    /// Internally-originated clipboard copy (selection copy 등). 히스토리에 저장하되
    /// `Source::Internal`로 태깅. `history_enabled`가 false면 no-op.
    pub fn record_internal_copy(&mut self, text: &str) {
        if !self.settings.clipboard.history_enabled {
            return;
        }
        self.clipboard_history.record(
            text.to_string(),
            crate::clipboard_history::ClipboardSource::Internal,
        );
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

    /// Replace the terminal in a TerminalSurface, keeping the surface/layout intact.
    /// The old terminal's PTY process is dropped (SIGHUP sent).
    /// Returns Ok(()) on success, Err if the surface was not found.
    pub fn replace_terminal_by_id(
        &mut self,
        surface_id: u32,
        mut new_terminal: Terminal,
    ) -> anyhow::Result<()> {
        if let Some(old) = self.find_terminal_by_id_mut(surface_id) {
            std::mem::swap(old, &mut new_terminal);
            // old terminal (now in new_terminal) is dropped here, sending SIGHUP
            drop(new_terminal);
            Ok(())
        } else {
            anyhow::bail!("Surface {} not found", surface_id)
        }
    }

    fn find_terminal_in_layout(
        layout: &crate::model::PaneNode,
        surface_id: u32,
    ) -> Option<&Terminal> {
        match layout {
            crate::model::PaneNode::Leaf(pane) => pane.find_terminal(surface_id),
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::find_terminal_in_layout(first, surface_id)
                    .or_else(|| Self::find_terminal_in_layout(second, surface_id))
            }
        }
    }

    fn find_terminal_in_layout_mut(
        layout: &mut crate::model::PaneNode,
        surface_id: u32,
    ) -> Option<&mut Terminal> {
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

    /// Flush deferred PTY resizes (throttled). Returns true if any terminal still has pending resize.
    pub fn flush_all_pty_resizes(&mut self) -> bool {
        let mut any_pending = false;
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |_sid, terminal| {
                    terminal.flush_pty_resize();
                    if terminal.has_pending_pty_resize() {
                        any_pending = true;
                    }
                });
        }
        any_pending
    }

    /// Mark layout as dirty for persistence.
    pub fn mark_layout_dirty(&mut self) {
        self.layout_dirty.mark_dirty();
    }

    /// Force flush deferred PTY resizes (ignores throttle).
    /// Used after discrete events like pane split/close.
    pub fn force_flush_all_pty_resizes(&mut self) {
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |_sid, terminal| {
                    terminal.force_flush_pty_resize();
                });
        }
    }

    /// Collect events from all terminals.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |sid, terminal| {
                    let mut events = terminal.take_events();
                    for event in &mut events {
                        event.surface_id = sid;
                    }
                    all_events.extend(events);
                });
        }
        all_events
    }

    /// Collect all terminal surface IDs across all workspaces.
    /// 현재는 CWD 폴링(macOS/Linux)에서만 사용된다.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn all_terminal_surface_ids(&mut self) -> Vec<u32> {
        let mut ids = Vec::new();
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |sid, _terminal| {
                    ids.push(sid);
                });
        }
        ids
    }

}
