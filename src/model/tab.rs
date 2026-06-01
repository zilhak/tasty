use super::FocusDirection;
use super::surface_layout::SurfaceLayout;
use super::surface_trait::Surface;
use super::{SplitDirection, SurfaceId, TabId, TerminalSurface};
use tasty_terminal::Terminal;

pub struct Tab {
    pub id: TabId,
    /// Auto-generated name (e.g. "Shell"). Used as fallback when explicit_name is None.
    pub name: String,
    /// Explicitly set tab name. When Some, overrides everything else.
    pub explicit_name: Option<String>,
    /// OSC 0/2 로 받은 terminal window title. `explicit_name` 보다 낮고
    /// `cached_display_name` 보다 높은 우선순위. cwd 변경 시 shell prompt 가
    /// 새 OSC title 을 자연 발화하므로 cwd 도 자연 반영된다. layout.json
    /// 영속 대상 아님 — runtime only.
    pub osc_title: Option<String>,
    /// The layout tree of surfaces. Always a binary tree; a single leaf = unsplit state.
    /// Temporarily `None` during structural mutations (take_layout/put_layout pattern).
    /// Deferred terminals live inside the layout as `EmptySurface { deferred_spawn: Some(..) }`
    /// placeholders, NOT as a None layout.
    pub layout_opt: Option<SurfaceLayout>,
    /// The focused surface ID within this tab's layout.
    pub focused_surface: SurfaceId,
    /// Cached display name. Updated on CwdChanged/explicit_name change, not every frame.
    pub cached_display_name: Option<String>,
}

impl Tab {
    /// Create a tab with a Surface trait object.
    pub fn new_with_surface(id: TabId, name: String, surface: Box<dyn Surface>) -> Self {
        let surface_id = surface.surface_id().unwrap_or(0);
        Self {
            id,
            name,
            explicit_name: None,
            osc_title: None,
            layout_opt: Some(SurfaceLayout::Leaf(surface)),
            focused_surface: surface_id,
            cached_display_name: None,
        }
    }

    /// Get the display name for this tab (cached, no syscalls).
    /// Priority: explicit_name > osc_title > cached CWD-derived name > fallback "name" field.
    pub fn display_name(&self) -> String {
        if let Some(ref explicit) = self.explicit_name {
            return explicit.clone();
        }
        if let Some(ref osc) = self.osc_title {
            return osc.clone();
        }
        if let Some(ref cached) = self.cached_display_name {
            return cached.clone();
        }
        self.name.clone()
    }

    /// Recompute and cache the display name from the focused terminal's CWD.
    /// Call this when CWD changes (CwdChanged event) or when the tab is first created.
    pub fn refresh_display_name(&mut self) {
        if self.explicit_name.is_some() {
            return;
        }
        let terminal = self.focused_terminal();
        if let Some(terminal) = terminal
            && let Some(cwd) = terminal.get_cwd()
        {
            if let Some(home) = dirs_home()
                && cwd == home
            {
                self.cached_display_name = Some("~".to_string());
                return;
            }
            let path_str = cwd.to_string_lossy();
            if path_str == "/" {
                self.cached_display_name = Some("/".to_string());
                return;
            }
            if let Some(name) = cwd.file_name() {
                self.cached_display_name = Some(name.to_string_lossy().to_string());
                return;
            }
        }
        self.cached_display_name = None;
    }

    // ── Layout-based accessors ──

    /// Access the layout.
    #[track_caller]
    pub fn layout(&self) -> &SurfaceLayout {
        self.layout_opt
            .as_ref()
            .expect("BUG: no layout (deferred tab not initialized?)")
    }

    /// Access the layout mutably.
    #[track_caller]
    pub fn layout_mut(&mut self) -> &mut SurfaceLayout {
        self.layout_opt
            .as_mut()
            .expect("BUG: no layout (deferred tab not initialized?)")
    }

    /// Access the layout if initialized.
    pub fn layout_if_initialized(&self) -> Option<&SurfaceLayout> {
        self.layout_opt.as_ref()
    }

    /// Take the layout out (for structural mutation). Must be followed by put_layout.
    #[track_caller]
    pub fn take_layout(&mut self) -> SurfaceLayout {
        self.layout_opt.take().expect("BUG: layout already taken")
    }

    /// Put the layout back after structural mutation.
    pub fn put_layout(&mut self, layout: SurfaceLayout) {
        self.layout_opt = Some(layout);
    }

    // ── Surface delegation (backward-compat helpers) ──

    /// Get the "surface" for this tab. For a single leaf, returns the leaf.
    /// For a split, returns the focused leaf surface.
    /// NOTE: Callers that need the full layout tree should use layout() instead.
    #[track_caller]
    pub fn surface(&self) -> &dyn Surface {
        let layout = self.layout();
        // For a single leaf, return it directly
        if let SurfaceLayout::Leaf(surface) = layout {
            return surface.as_ref();
        }
        // For splits, return the focused leaf
        if let Some(leaf) = layout.find_surface(self.focused_surface) {
            return leaf;
        }
        // Fallback: first leaf
        if let Some(first_id) = layout.first_surface_id()
            && let Some(leaf) = layout.find_surface(first_id)
        {
            return leaf;
        }
        panic!("BUG: layout has no surfaces");
    }

    /// Get the focused surface mutably.
    /// NOTE: Callers that need the full layout tree should use layout_mut() instead.
    #[track_caller]
    pub fn surface_mut(&mut self) -> &mut dyn Surface {
        let focused = self.focused_surface;
        let layout = self.layout_mut();
        // For a single leaf, return it directly
        if let SurfaceLayout::Leaf(surface) = layout {
            return surface.as_mut();
        }
        // Determine which ID to look up
        let target_id = if layout.contains_surface(focused) {
            focused
        } else {
            layout
                .first_surface_id()
                .expect("BUG: layout has no surfaces")
        };
        layout
            .find_leaf_mut(target_id)
            .map(|b| b.as_mut())
            .expect("BUG: layout has no surfaces")
    }

    /// Access the surface if initialized (for backward compat).
    pub fn surface_if_initialized(&self) -> Option<&dyn Surface> {
        let layout = self.layout_opt.as_ref()?;
        if let SurfaceLayout::Leaf(surface) = layout {
            return Some(surface.as_ref());
        }
        if let Some(leaf) = layout.find_surface(self.focused_surface) {
            return Some(leaf);
        }
        layout
            .first_surface_id()
            .and_then(|id| layout.find_surface(id))
    }

    /// Whether the layout is a split (more than one surface).
    pub fn is_split(&self) -> bool {
        matches!(self.layout_opt.as_ref(), Some(SurfaceLayout::Split { .. }))
    }

    /// All surface IDs in this tab.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.layout().all_surface_ids()
    }

    /// Whether this tab contains the given surface ID.
    pub fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        match &self.layout_opt {
            Some(layout) => layout.contains_surface(surface_id),
            None => false,
        }
    }

    /// Get the focused terminal.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        let layout = self.layout_opt.as_ref()?;
        layout
            .find_terminal(self.focused_surface)
            .or_else(|| layout.first_terminal())
    }

    /// Get the focused terminal mutably.
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        let id = self.focused_surface;
        let layout = self.layout_opt.as_mut()?;
        layout.find_terminal(id)?;
        layout.find_terminal_mut(id)
    }

    /// Find a terminal by surface ID.
    pub fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        self.layout_opt.as_ref()?.find_terminal(surface_id)
    }

    /// Find a terminal by surface ID (mutable).
    pub fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        self.layout_opt.as_mut()?.find_terminal_mut(surface_id)
    }

    /// Find a TerminalSurface by surface ID.
    pub fn find_terminal_surface(&self, surface_id: SurfaceId) -> Option<&TerminalSurface> {
        self.layout_opt.as_ref()?.find_surface_node(surface_id)
    }

    /// Get the focused surface ID.
    pub fn focused_surface_id(&self) -> Option<SurfaceId> {
        Some(self.focused_surface)
    }

    /// Visit all terminals with their surface IDs.
    pub fn for_each_terminal_mut(&mut self, f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {
        if let Some(layout) = self.layout_opt.as_mut() {
            layout.for_each_terminal_mut_dyn(f);
        }
    }

    /// Visit every leaf Surface (read-only). 닫기 경로의 persist_id 수집용.
    pub fn for_each_surface(&self, f: &mut dyn FnMut(&dyn crate::model::Surface)) {
        if let Some(layout) = self.layout_opt.as_ref() {
            layout.for_each_surface(f);
        }
    }

    /// Collect all terminals (mutable).
    pub fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        if let Some(layout) = self.layout_opt.as_mut() {
            layout.collect_terminals_mut(out);
        }
    }

    // ── Surface layout operations ──

    /// Close a surface within this tab. Returns true if found and closed.
    pub fn close_surface(&mut self, target_id: SurfaceId) -> bool {
        let old_layout = self.take_layout();
        let (new_layout, found) = old_layout.close_surface(target_id);
        self.put_layout(new_layout);
        if found
            && self.focused_surface == target_id
            && let Some(first_id) = self.layout().first_surface_id()
        {
            self.focused_surface = first_id;
        }
        found
    }

    /// Move focus to the next surface.
    pub fn move_focus_forward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 {
            return;
        }
        let pos = ids
            .iter()
            .position(|&id| id == self.focused_surface)
            .unwrap_or(0);
        self.focused_surface = ids[(pos + 1) % ids.len()];
    }

    /// Move focus to the previous surface.
    pub fn move_focus_backward(&mut self) {
        let ids = self.layout().all_surface_ids();
        if ids.len() <= 1 {
            return;
        }
        let pos = ids
            .iter()
            .position(|&id| id == self.focused_surface)
            .unwrap_or(0);
        self.focused_surface = ids[(pos + ids.len() - 1) % ids.len()];
    }

    /// Directional focus navigation.
    pub fn directional_focus(&self, direction: FocusDirection) -> Option<SurfaceId> {
        self.layout()
            .directional_focus(self.focused_surface, direction)
    }

    /// Resize all surfaces within the layout.
    pub fn resize_all(&mut self, rect: super::PhysicalRect, cell_width: f32, cell_height: f32) {
        if let Some(layout) = self.layout_opt.as_mut() {
            layout.resize_all(rect, cell_width, cell_height);
        }
    }

    /// All surface regions with Surface trait references.
    pub fn surface_regions(&self, rect: super::PhysicalRect) -> Vec<super::SurfaceRegion<'_>> {
        self.layout().surface_regions(rect)
    }

    // ── Initialization ──

    /// 특정 surface_id에 해당하는 deferred placeholder를 찾아 PTY를 spawn하고
    /// `TerminalSurface`로 교체. spawn된 경우 true.
    pub fn ensure_initialized(&mut self, surface_id: SurfaceId) -> bool {
        let Some(layout) = self.layout_opt.as_mut() else {
            return false;
        };
        let Some(leaf) = layout.find_leaf_mut(surface_id) else {
            return false;
        };
        let Some(empty) = leaf.as_any_mut().downcast_mut::<super::EmptySurface>() else {
            return false;
        };
        let Some(spawn) = empty.take_deferred_spawn() else {
            return false;
        };
        // DeferredSpawn 의 scrollback_persist_id 를 spawn 직후 새 TerminalSurface
        // 로 옮긴다 (spawn 함수가 spawn 을 consume 하므로 미리 빼둔다).
        let persist_id = spawn.scrollback_persist_id.clone();
        match spawn_terminal_from_deferred(surface_id, spawn) {
            Some(terminal) => {
                let ts: Box<dyn Surface> = Box::new(TerminalSurface {
                    id: surface_id,
                    terminal,
                    deferred_spawn: None,
                    scrollback_persist_id: persist_id,
                });
                *leaf = ts;
                true
            }
            None => false,
        }
    }

    /// 이 탭의 layout 안에 deferred placeholder로 남아있는 모든 surface ID를 spawn.
    /// 반환값은 새로 spawn된 surface_id 목록.
    pub fn ensure_all_initialized(&mut self) -> Vec<SurfaceId> {
        let ids = self.deferred_surface_ids();
        let mut spawned = Vec::with_capacity(ids.len());
        for sid in ids {
            if self.ensure_initialized(sid) {
                spawned.push(sid);
            }
        }
        spawned
    }

    /// layout 안에 deferred EmptySurface placeholder가 하나라도 있으면 true.
    pub fn is_deferred(&self) -> bool {
        !self.deferred_surface_ids().is_empty()
    }

    /// layout 안의 모든 deferred EmptySurface placeholder의 surface_id 목록.
    pub fn deferred_surface_ids(&self) -> Vec<SurfaceId> {
        let mut out = Vec::new();
        if let Some(layout) = self.layout_opt.as_ref() {
            collect_deferred_ids(layout, &mut out);
        }
        out
    }

    /// 주어진 surface_id가 이 탭의 deferred placeholder인지 확인.
    pub fn is_surface_deferred(&self, surface_id: SurfaceId) -> bool {
        let Some(layout) = self.layout_opt.as_ref() else {
            return false;
        };
        let Some(leaf) = layout.find_surface(surface_id) else {
            return false;
        };
        leaf.as_any()
            .downcast_ref::<super::EmptySurface>()
            .map(|e| e.is_deferred())
            .unwrap_or(false)
    }

    /// Replace the entire layout with a single surface.
    pub fn put_surface(&mut self, surface: Box<dyn Surface>) {
        let sid = surface.surface_id().unwrap_or(0);
        self.layout_opt = Some(SurfaceLayout::Leaf(surface));
        self.focused_surface = sid;
    }

    // ── Split operations ──

    /// Split the focused surface within this tab.
    /// Moves focus to the new surface.
    pub fn split_focused_surface(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        new_terminal: Terminal,
    ) {
        let new_node = TerminalSurface {
            id: new_surface_id,
            terminal: new_terminal,
            deferred_spawn: None,
            scrollback_persist_id: None,
        };
        let target = self.focused_surface;
        let old_layout = self.take_layout();
        let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
        self.put_layout(new_layout);
        self.focused_surface = new_surface_id;
    }

    /// Split a specific surface by ID. Does NOT change focused_surface.
    pub fn split_surface_by_id(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        new_terminal: Terminal,
    ) -> bool {
        let new_node = TerminalSurface {
            id: new_surface_id,
            terminal: new_terminal,
            deferred_spawn: None,
            scrollback_persist_id: None,
        };
        let old_layout = self.take_layout();
        let (new_layout, remaining) =
            old_layout.split_with_node(target_surface_id, direction, new_node);
        self.put_layout(new_layout);
        remaining.is_none()
    }

    /// Split a specific surface by ID with any surface type.
    pub fn split_surface_by_id_generic(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface: Box<dyn Surface>,
    ) -> bool {
        let old_layout = self.take_layout();
        let (new_layout, remaining) =
            old_layout.split_with_surface(target_surface_id, direction, new_surface);
        self.put_layout(new_layout);
        if remaining.is_some() {
            tracing::warn!(
                "split_surface_by_id_generic: target {} not found",
                target_surface_id
            );
        }
        remaining.is_none()
    }

    /// Produce a JSON tree representation of this tab.
    pub fn to_tree_json(&self) -> serde_json::Value {
        let layout_json = if self.is_split() {
            serde_json::json!({
                "type": "SplitLayout",
                "focused_surface": self.focused_surface,
                "surfaces": self.all_surface_ids(),
            })
        } else {
            // Single-leaf tab. EmptySurface(deferred) renders itself with pty_ready: false.
            // For a live TerminalSurface, append pty_ready: true.
            let mut v = self.surface().to_tree_json();
            if v.get("type").and_then(|t| t.as_str()) == Some("Terminal")
                && !v
                    .as_object()
                    .map(|o| o.contains_key("pty_ready"))
                    .unwrap_or(false)
                && let Some(obj) = v.as_object_mut()
            {
                obj.insert("pty_ready".into(), serde_json::json!(true));
            }
            v
        };
        serde_json::json!({
            "id": self.id,
            "name": self.display_name(),
            "surface": layout_json,
        })
    }
}

fn collect_deferred_ids(layout: &SurfaceLayout, out: &mut Vec<SurfaceId>) {
    match layout {
        SurfaceLayout::Leaf(surface) => {
            if let Some(empty) = surface.as_any().downcast_ref::<super::EmptySurface>()
                && empty.is_deferred()
            {
                out.push(empty.id);
            }
        }
        SurfaceLayout::Split { first, second, .. } => {
            collect_deferred_ids(first, out);
            collect_deferred_ids(second, out);
        }
    }
}

fn spawn_terminal_from_deferred(
    surface_id: SurfaceId,
    spawn: super::terminal_surface::DeferredSpawn,
) -> Option<Terminal> {
    let shell_ref = spawn.shell.as_deref();
    let shell_args: Vec<&str> = spawn.shell_args.iter().map(|s| s.as_str()).collect();
    let working_dir = spawn.working_dir.as_deref();
    // PTY master 의 첫 입력으로 restore_command 를 미리 적재. Terminal::new 가
    // writer thread spawn 전에 동기 write 하므로, shell 이 stdin 을 처음 read
    // 하는 순간 이 명령이 들어간다 (예: `claude -r <uuid>\r`).
    let initial = spawn.restore_command.as_deref().map(|c| format!("{c}\r"));
    let initial_input = initial.as_deref();
    match Terminal::new(
        tasty_terminal::TerminalConfig {
            cols: spawn.cols,
            rows: spawn.rows,
            shell: shell_ref,
            args: &shell_args,
            surface_id,
            working_dir,
            initial_input,
        },
        spawn.waker,
    ) {
        Ok(terminal) => Some(terminal),
        Err(e) => {
            tracing::error!("lazy PTY init failed: {e}");
            None
        }
    }
}

fn dirs_home() -> Option<std::path::PathBuf> {
    #[cfg(not(windows))]
    {
        std::env::var("HOME").ok().map(std::path::PathBuf::from)
    }
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(std::path::PathBuf::from)
    }
}
