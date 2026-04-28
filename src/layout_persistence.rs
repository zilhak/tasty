//! Layout persistence: save/restore workspace layout to `~/.tasty/layout.json`.
//!
//! Captures the structural tree (workspaces → pane nodes → panes → tabs → surface layouts)
//! with minimal per-surface info (cwd, file path, url). No screen/scrollback content.

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::engine_state::{EngineState, ShellConfig};
use crate::model::{
    ExplorerPanel, HtmlPanel, ImagePanel, MarkdownPanel, Pane, PaneNode, SplitDirection, Surface,
    SurfaceLayout, Tab, TerminalSurface, Workspace,
};

const LAYOUT_VERSION: u32 = 1;
const DEBOUNCE_MS: u128 = 500;

// ── Serializable structs ──

#[derive(Serialize, Deserialize)]
pub struct SavedLayout {
    pub version: u32,
    pub workspaces: Vec<SavedWorkspace>,
    pub active_workspace: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub pane_layout: SavedPaneNode,
    /// Index of the focused pane among all leaf panes (left-to-right DFS order).
    pub focused_pane_index: usize,
}

#[derive(Serialize, Deserialize)]
pub enum SavedPaneNode {
    Leaf(SavedPane),
    Split {
        direction: SavedSplitDirection,
        ratio: f32,
        first: Box<SavedPaneNode>,
        second: Box<SavedPaneNode>,
    },
}

#[derive(Serialize, Deserialize)]
pub struct SavedPane {
    pub tabs: Vec<SavedTab>,
    pub active_tab: usize,
}

#[derive(Serialize, Deserialize)]
pub struct SavedTab {
    pub name: String,
    pub explicit_name: Option<String>,
    pub surface: SavedSurfaceLayout,
}

#[derive(Serialize, Deserialize)]
pub enum SavedSurfaceLayout {
    Leaf(SavedSurface),
    Split {
        direction: SavedSplitDirection,
        ratio: f32,
        first: Box<SavedSurfaceLayout>,
        second: Box<SavedSurfaceLayout>,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum SavedSplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Serialize, Deserialize)]
pub enum SavedSurface {
    Terminal { cwd: Option<String> },
    Markdown { path: String },
    Explorer { root_path: String },
    Html { url: String },
    Image { path: Option<String> },
    Empty,
}

// ── Direction conversion ──

impl From<SplitDirection> for SavedSplitDirection {
    fn from(d: SplitDirection) -> Self {
        match d {
            SplitDirection::Horizontal => SavedSplitDirection::Horizontal,
            SplitDirection::Vertical => SavedSplitDirection::Vertical,
        }
    }
}

impl From<SavedSplitDirection> for SplitDirection {
    fn from(d: SavedSplitDirection) -> Self {
        match d {
            SavedSplitDirection::Horizontal => SplitDirection::Horizontal,
            SavedSplitDirection::Vertical => SplitDirection::Vertical,
        }
    }
}

// ── Capture: live model → SavedLayout ──

impl SavedLayout {
    /// Capture current layout from engine state.
    pub fn capture(engine: &EngineState, active_workspace: usize) -> Self {
        let workspaces = engine
            .workspaces
            .iter()
            .map(SavedWorkspace::capture)
            .collect();
        Self {
            version: LAYOUT_VERSION,
            workspaces,
            active_workspace,
        }
    }
}

impl SavedWorkspace {
    fn capture(ws: &Workspace) -> Self {
        let pane_layout = SavedPaneNode::capture(ws.pane_layout());
        // Find the index of the focused pane among all leaf panes.
        let all_ids = ws.pane_layout().all_pane_ids();
        let focused_pane_index = all_ids
            .iter()
            .position(|&id| id == ws.focused_pane)
            .unwrap_or(0);
        Self {
            name: ws.name.clone(),
            subtitle: ws.subtitle.clone(),
            description: ws.description.clone(),
            pane_layout,
            focused_pane_index,
        }
    }
}

impl SavedPaneNode {
    fn capture(node: &PaneNode) -> Self {
        match node {
            PaneNode::Leaf(pane) => SavedPaneNode::Leaf(SavedPane::capture(pane)),
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => SavedPaneNode::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(SavedPaneNode::capture(first)),
                second: Box::new(SavedPaneNode::capture(second)),
            },
        }
    }
}

impl SavedPane {
    fn capture(pane: &Pane) -> Self {
        let tabs = pane.tabs.iter().map(SavedTab::capture).collect();
        Self {
            tabs,
            active_tab: pane.active_tab,
        }
    }
}

impl SavedTab {
    fn capture(tab: &Tab) -> Self {
        let surface = if tab.is_split() {
            SavedSurfaceLayout::capture_layout(tab.layout())
        } else {
            SavedSurfaceLayout::Leaf(SavedSurface::capture_surface(tab.surface()))
        };
        Self {
            name: tab.name.clone(),
            explicit_name: tab.explicit_name.clone(),
            surface,
        }
    }
}

impl SavedSurfaceLayout {
    fn capture_layout(layout: &SurfaceLayout) -> Self {
        match layout {
            SurfaceLayout::Leaf(surface) => {
                SavedSurfaceLayout::Leaf(SavedSurface::capture_surface(&**surface))
            }
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => SavedSurfaceLayout::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(SavedSurfaceLayout::capture_layout(first)),
                second: Box::new(SavedSurfaceLayout::capture_layout(second)),
            },
        }
    }
}

impl SavedSurface {
    fn capture_surface(surface: &dyn Surface) -> Self {
        if let Some(ts) = surface.as_terminal_surface() {
            return SavedSurface::Terminal {
                cwd: ts.terminal.get_cwd().map(|p| p.to_string_lossy().to_string()),
            };
        }
        if let Some(md) = surface.as_markdown() {
            return SavedSurface::Markdown {
                path: md.file_path.clone(),
            };
        }
        if let Some(ex) = surface.as_explorer() {
            return SavedSurface::Explorer {
                root_path: ex.root_path.clone(),
            };
        }
        if let Some(html) = surface.as_html() {
            return SavedSurface::Html {
                url: html.url.clone(),
            };
        }
        if let Some(img) = surface.as_image() {
            return SavedSurface::Image {
                path: img.file_path.clone(),
            };
        }
        // ClipboardViewer, Empty, etc. — store as Empty
        SavedSurface::Empty
    }
}

// ── Restore: SavedLayout → live model ──

impl SavedLayout {
    /// Restore layout into engine state. Returns true on success.
    /// On failure, engine state is left unchanged (caller should create default workspace).
    pub fn restore(self, engine: &mut EngineState) -> bool {
        if self.workspaces.is_empty() {
            return false;
        }

        let active_idx = self.active_workspace.min(self.workspaces.len() - 1);
        let mut workspaces = Vec::new();
        for (i, saved_ws) in self.workspaces.into_iter().enumerate() {
            let name = saved_ws.name.clone();
            let is_active = i == active_idx;
            match saved_ws.restore(engine, is_active) {
                Some(ws) => workspaces.push(ws),
                None => {
                    tracing::warn!("Failed to restore workspace '{}', skipping", name);
                }
            }
        }

        if workspaces.is_empty() {
            return false;
        }

        let active = self.active_workspace.min(workspaces.len() - 1);
        engine.workspaces = workspaces;
        engine.restored_active_workspace = Some(active);
        true
    }
}

impl SavedWorkspace {
    fn restore(self, engine: &mut EngineState, is_active: bool) -> Option<Workspace> {
        let ws_id = engine.next_ids.next_workspace();
        let pane_layout = self.pane_layout.restore(engine, is_active)?;

        // Resolve focused pane by index.
        let all_ids = pane_layout.all_pane_ids();
        let focused_pane = all_ids
            .get(self.focused_pane_index)
            .copied()
            .or_else(|| all_ids.first().copied())
            .unwrap_or(0);

        Some(Workspace::from_restored(
            ws_id,
            self.name,
            self.subtitle,
            pane_layout,
            focused_pane,
        ))
    }
}

impl SavedPaneNode {
    fn restore(self, engine: &mut EngineState, is_active: bool) -> Option<PaneNode> {
        match self {
            SavedPaneNode::Leaf(saved_pane) => {
                let pane = saved_pane.restore(engine, is_active)?;
                Some(PaneNode::Leaf(pane))
            }
            SavedPaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let first = first.restore(engine, is_active)?;
                let second = second.restore(engine, is_active)?;
                Some(PaneNode::Split {
                    direction: direction.into(),
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
        }
    }
}

impl SavedPane {
    fn restore(self, engine: &mut EngineState, is_active: bool) -> Option<Pane> {
        let pane_id = engine.next_ids.next_pane();
        let mut tabs = Vec::new();
        for saved_tab in self.tabs {
            match saved_tab.restore(engine, is_active) {
                Some(tab) => tabs.push(tab),
                None => {
                    tracing::warn!("Failed to restore tab, skipping");
                }
            }
        }
        if tabs.is_empty() {
            return None;
        }
        let active_tab = self.active_tab.min(tabs.len() - 1);
        Some(Pane {
            id: pane_id,
            tabs,
            active_tab,
            tab_scroll_offset: 0.0,
        })
    }
}

impl SavedTab {
    fn restore(self, engine: &mut EngineState, is_active: bool) -> Option<Tab> {
        let tab_id = engine.next_ids.next_tab();
        let result = self.surface.restore(engine, is_active)?;
        match result {
            RestoreResult::Ready(layout) => {
                let focused_surface = layout.first_surface_id().unwrap_or(0);
                Some(Tab {
                    id: tab_id,
                    name: self.name,
                    explicit_name: self.explicit_name,
                    layout_opt: Some(layout),
                    focused_surface,
                    deferred_spawn: None,
                    deferred_surface_id: None,
                    cached_display_name: None,
                })
            }
            RestoreResult::Deferred {
                surface_id,
                spawn,
            } => {
                // Placeholder surface — replaced by actual terminal on workspace switch.
                let placeholder = crate::model::EmptySurface::new(surface_id);
                Some(Tab {
                    id: tab_id,
                    name: self.name,
                    explicit_name: self.explicit_name,
                    layout_opt: Some(SurfaceLayout::Leaf(Box::new(placeholder))),
                    focused_surface: surface_id,
                    deferred_spawn: Some(spawn),
                    deferred_surface_id: Some(surface_id),
                    cached_display_name: None,
                })
            }
        }
    }
}

/// Result of restoring a surface: either ready (PTY spawned) or deferred.
enum RestoreResult {
    Ready(SurfaceLayout),
    Deferred {
        surface_id: u32,
        spawn: crate::model::DeferredSpawn,
    },
}

impl SavedSurfaceLayout {
    fn restore(self, engine: &mut EngineState, is_active: bool) -> Option<RestoreResult> {
        match self {
            SavedSurfaceLayout::Leaf(saved) => saved.restore_result(engine, is_active),
            SavedSurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                // Split surfaces: 내부에 deferred가 섞이면 복잡해지므로,
                // 비활성이어도 split 내부는 즉시 생성한다.
                // (split이 있는 탭은 보통 1~2개 surface이므로 부담 적음)
                let first = first.restore_ready(engine)?;
                let second = second.restore_ready(engine)?;
                Some(RestoreResult::Ready(SurfaceLayout::Split {
                    direction: direction.into(),
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                    focus_second: false,
                }))
            }
        }
    }

    /// 항상 즉시 생성 (split 내부용).
    fn restore_ready(self, engine: &mut EngineState) -> Option<SurfaceLayout> {
        match self {
            SavedSurfaceLayout::Leaf(saved) => {
                let surface = saved.restore_immediate(engine)?;
                Some(SurfaceLayout::Leaf(surface))
            }
            SavedSurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let first = first.restore_ready(engine)?;
                let second = second.restore_ready(engine)?;
                Some(SurfaceLayout::Split {
                    direction: direction.into(),
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                    focus_second: false,
                })
            }
        }
    }
}

impl SavedSurface {
    /// 비활성 워크스페이스 터미널은 deferred, 그 외는 즉시 생성.
    fn restore_result(self, engine: &mut EngineState, is_active: bool) -> Option<RestoreResult> {
        if !is_active {
            if let SavedSurface::Terminal { ref cwd } = self {
                let surface_id = engine.next_ids.next_surface();
                let sh = ShellConfig::from_settings(&engine.settings);
                let waker = engine.make_waker(surface_id);
                let spawn = crate::model::DeferredSpawn {
                    shell: sh.shell_ref().map(|s| s.to_string()),
                    shell_args: sh.args_ref().iter().map(|s| s.to_string()).collect(),
                    cols: engine.default_cols,
                    rows: engine.default_rows,
                    waker,
                    working_dir: cwd.as_ref().map(PathBuf::from),
                };
                return Some(RestoreResult::Deferred { surface_id, spawn });
            }
        }
        let surface = self.restore_immediate(engine)?;
        Some(RestoreResult::Ready(SurfaceLayout::Leaf(surface)))
    }

    /// 항상 즉시 PTY를 spawn하여 Surface를 반환.
    fn restore_immediate(self, engine: &mut EngineState) -> Option<Box<dyn Surface>> {
        let surface_id = engine.next_ids.next_surface();
        match self {
            SavedSurface::Terminal { cwd } => {
                let sh = ShellConfig::from_settings(&engine.settings);
                let waker = engine.make_waker(surface_id);
                let working_dir = cwd.as_ref().map(PathBuf::from);
                let terminal = match tasty_terminal::Terminal::new_with_shell_args_cwd(
                    engine.default_cols,
                    engine.default_rows,
                    sh.shell_ref(),
                    &sh.args_ref(),
                    surface_id,
                    waker,
                    working_dir.as_deref(),
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("Failed to create terminal for restored surface: {e}");
                        return None;
                    }
                };
                engine.send_fast_init(surface_id);
                Some(Box::new(TerminalSurface {
                    id: surface_id,
                    terminal,
                    deferred_spawn: None,
                }))
            }
            SavedSurface::Markdown { path } => {
                Some(Box::new(MarkdownPanel::new(surface_id, path)))
            }
            SavedSurface::Explorer { root_path } => {
                Some(Box::new(ExplorerPanel::new(surface_id, root_path)))
            }
            SavedSurface::Html { url } => Some(Box::new(HtmlPanel::new(surface_id, url))),
            SavedSurface::Image { path } => match path {
                Some(p) => Some(Box::new(ImagePanel::new(surface_id, p))),
                None => Some(Box::new(ImagePanel::new_blank(surface_id))),
            },
            SavedSurface::Empty => {
                Some(Box::new(crate::model::EmptySurface::new(surface_id)))
            }
        }
    }
}

// ── Disk I/O ──

fn layout_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty").join("layout.json"))
}

/// Save layout to disk. Non-blocking best-effort.
pub fn save_to_disk(engine: &EngineState, active_workspace: usize) {
    let path = match layout_path() {
        Some(p) => p,
        None => {
            tracing::warn!("Cannot determine ~/.tasty path for layout save");
            return;
        }
    };
    let saved = SavedLayout::capture(engine, active_workspace);
    let json = match serde_json::to_string_pretty(&saved) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("Failed to serialize layout: {e}");
            return;
        }
    };
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("Failed to create dir for layout.json: {e}");
        return;
    }
    if let Err(e) = std::fs::write(&path, json) {
        tracing::warn!("Failed to write layout.json: {e}");
    }
}

/// Load layout from disk. Returns None if file doesn't exist or is invalid.
pub fn load_from_disk() -> Option<SavedLayout> {
    let path = layout_path()?;
    let json = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<SavedLayout>(&json) {
        Ok(layout) => {
            if layout.version > LAYOUT_VERSION {
                tracing::warn!(
                    "layout.json version {} is newer than supported {}",
                    layout.version,
                    LAYOUT_VERSION
                );
                return None;
            }
            Some(layout)
        }
        Err(e) => {
            tracing::warn!("Failed to parse layout.json: {e}");
            None
        }
    }
}

// ── Dirty flag / debounce state ──

/// Tracks whether the layout has been modified and needs saving.
pub struct LayoutDirtyTracker {
    dirty: bool,
    dirty_since: Option<Instant>,
}

impl LayoutDirtyTracker {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for LayoutDirtyTracker {
    fn default() -> Self {
        Self {
            dirty: false,
            dirty_since: None,
        }
    }
}

impl LayoutDirtyTracker {
    /// Mark layout as dirty (called on structural changes).
    pub fn mark_dirty(&mut self) {
        if !self.dirty {
            self.dirty = true;
            self.dirty_since = Some(Instant::now());
        }
    }

    /// Check if enough time has elapsed and a flush is needed.
    /// Returns true if the caller should save now.
    pub fn should_flush(&self) -> bool {
        if !self.dirty {
            return false;
        }
        match self.dirty_since {
            Some(since) => since.elapsed().as_millis() >= DEBOUNCE_MS,
            None => false,
        }
    }

    /// Reset after a successful save.
    pub fn clear(&mut self) {
        self.dirty = false;
        self.dirty_since = None;
    }

    /// Force check if dirty (for shutdown flush).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}
