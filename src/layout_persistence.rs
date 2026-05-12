//! Layout persistence: save/restore workspace layout to `~/.tasty/layout.json`.
//!
//! Captures the structural tree (workspaces → pane nodes → panes → tabs → surface layouts)
//! with minimal per-surface info (cwd, file path, url). No screen/scrollback content.
//!
//! # Versions
//!
//! - **v1** (legacy): `SavedSurface` had explicit `Markdown / Explorer / Html / Image / Empty`
//!   variants alongside `Terminal`. v1 files are auto-migrated to v2 on load.
//! - **v2** (current): `SavedSurface` is `Terminal` + `Generic { kind, data }`. New surface
//!   kinds (including plugins, eventually) round-trip via the SurfaceKindRegistry without
//!   touching this file.

use std::path::PathBuf;
use std::time::Instant;

use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::engine_state::{EngineState, ShellConfig};
use crate::model::{Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab, TerminalSurface, Workspace};
use crate::surface_registry::SurfaceKindRegistry;

const LAYOUT_VERSION: u32 = 2;
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

/// Persistent surface representation.
///
/// `Terminal` stays its own variant because PTY spawn is host-managed and needs
/// engine state (cols/rows/shell/waker) at restore time; routing it through the
/// registry would muddle that path. Every other surface kind goes through `Generic`
/// where the per-kind shape is opaque JSON, defined by the `SurfaceKindDef::snapshot`
/// / `restore` pair in the registry.
///
/// v1 files (separate `Markdown / Explorer / Html / Image / Empty` variants) are
/// transparently migrated by the manual `Deserialize` impl below.
#[derive(Serialize)]
pub enum SavedSurface {
    Terminal {
        cwd: Option<String>,
        /// Command to re-launch the TUI app that was running (e.g. "claude -r <session-id>").
        /// Populated from surface-meta `claude-session-id` at capture time.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_command: Option<String>,
    },
    Generic {
        kind: String,
        data: Value,
    },
}

impl<'de> Deserialize<'de> for SavedSurface {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        // v1 unit variant was serialised as the bare string "Empty".
        if let Some(s) = v.as_str() {
            return match s {
                "Empty" => Ok(SavedSurface::Generic {
                    kind: "empty".into(),
                    data: json!({}),
                }),
                other => Err(de::Error::unknown_variant(
                    other,
                    &["Terminal", "Generic", "Empty"],
                )),
            };
        }
        let obj = v
            .as_object()
            .ok_or_else(|| de::Error::custom("SavedSurface must be an object or 'Empty' string"))?;
        if obj.len() != 1 {
            return Err(de::Error::custom(
                "SavedSurface object must have exactly one variant key",
            ));
        }
        let (key, inner) = obj.iter().next().unwrap();
        match key.as_str() {
            "Terminal" => {
                let cwd = inner
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let restore_command = inner
                    .get("restore_command")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Ok(SavedSurface::Terminal {
                    cwd,
                    restore_command,
                })
            }
            "Generic" => {
                let kind = inner
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::custom("Generic missing 'kind'"))?
                    .to_string();
                let data = inner.get("data").cloned().unwrap_or_else(|| json!({}));
                Ok(SavedSurface::Generic { kind, data })
            }
            // ── v1 migration ───────────────────────────────────────────────
            "Markdown" => {
                let path = inner
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::custom("v1 Markdown missing 'path'"))?
                    .to_string();
                Ok(SavedSurface::Generic {
                    kind: "markdown".into(),
                    data: json!({ "path": path }),
                })
            }
            "Explorer" => {
                let root = inner
                    .get("root_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::custom("v1 Explorer missing 'root_path'"))?
                    .to_string();
                Ok(SavedSurface::Generic {
                    kind: "explorer".into(),
                    data: json!({ "path": root }),
                })
            }
            "Html" => {
                let url = inner
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| de::Error::custom("v1 Html missing 'url'"))?
                    .to_string();
                Ok(SavedSurface::Generic {
                    kind: "html".into(),
                    data: json!({ "url": url }),
                })
            }
            "Image" => {
                // v1 Image: path is Option<String> — null/absent means blank canvas.
                let path = inner
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Ok(SavedSurface::Generic {
                    kind: "image".into(),
                    data: json!({ "path": path }),
                })
            }
            "Empty" => Ok(SavedSurface::Generic {
                kind: "empty".into(),
                data: json!({}),
            }),
            other => Err(de::Error::unknown_variant(
                other,
                &[
                    "Terminal", "Generic", "Markdown", "Explorer", "Html", "Image", "Empty",
                ],
            )),
        }
    }
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
        let registry = engine.surface_registry.as_ref();
        let workspaces = engine
            .workspaces
            .iter()
            .map(|ws| SavedWorkspace::capture(ws, registry))
            .collect();
        Self {
            version: LAYOUT_VERSION,
            workspaces,
            active_workspace,
        }
    }
}

impl SavedWorkspace {
    fn capture(ws: &Workspace, registry: &SurfaceKindRegistry) -> Self {
        let pane_layout = SavedPaneNode::capture(ws.pane_layout(), registry);
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
    fn capture(node: &PaneNode, registry: &SurfaceKindRegistry) -> Self {
        match node {
            PaneNode::Leaf(pane) => SavedPaneNode::Leaf(SavedPane::capture(pane, registry)),
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => SavedPaneNode::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(SavedPaneNode::capture(first, registry)),
                second: Box::new(SavedPaneNode::capture(second, registry)),
            },
        }
    }
}

impl SavedPane {
    fn capture(pane: &Pane, registry: &SurfaceKindRegistry) -> Self {
        let tabs = pane
            .tabs
            .iter()
            .map(|t| SavedTab::capture(t, registry))
            .collect();
        Self {
            tabs,
            active_tab: pane.active_tab,
        }
    }
}

impl SavedTab {
    fn capture(tab: &Tab, registry: &SurfaceKindRegistry) -> Self {
        let surface = if tab.is_split() {
            SavedSurfaceLayout::capture_layout(tab.layout(), registry)
        } else {
            SavedSurfaceLayout::Leaf(SavedSurface::capture_surface(tab.surface(), registry))
        };
        Self {
            name: tab.name.clone(),
            explicit_name: tab.explicit_name.clone(),
            surface,
        }
    }
}

impl SavedSurfaceLayout {
    fn capture_layout(layout: &SurfaceLayout, registry: &SurfaceKindRegistry) -> Self {
        match layout {
            SurfaceLayout::Leaf(surface) => SavedSurfaceLayout::Leaf(
                SavedSurface::capture_surface(&**surface, registry),
            ),
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => SavedSurfaceLayout::Split {
                direction: (*direction).into(),
                ratio: *ratio,
                first: Box::new(SavedSurfaceLayout::capture_layout(first, registry)),
                second: Box::new(SavedSurfaceLayout::capture_layout(second, registry)),
            },
        }
    }
}

impl SavedSurface {
    fn capture_surface(surface: &dyn Surface, registry: &SurfaceKindRegistry) -> Self {
        if let Some(ts) = surface.as_terminal_surface() {
            let restore_command = crate::surface_meta::SurfaceMetaStore::get(
                ts.id,
                "claude-session-id",
            )
            .map(|session_id| format!("claude -r {}", session_id));

            return SavedSurface::Terminal {
                cwd: ts.terminal.get_cwd().map(|p| p.to_string_lossy().to_string()),
                restore_command,
            };
        }
        let kind = surface.kind().to_string();
        if let Some(def) = registry.get(&kind) {
            if let Some(data) = (def.snapshot)(surface) {
                return SavedSurface::Generic { kind, data };
            }
        }
        // snapshot 함수가 None을 반환했거나 (예: ClipboardViewer는 휘발성)
        // registry에 없는 kind면 Empty로 fallback.
        SavedSurface::Generic {
            kind: "empty".into(),
            data: json!({}),
        }
    }
}

// ── Restore: SavedLayout → live model ──

impl SavedLayout {
    /// Layout 안의 모든 Generic surface kind 토큰을 수집. 호출자는 첫 plugin pump
    /// 후에 registry에 이 kind들이 등록됐는지 확인하여 복원 시점을 결정한다.
    pub fn required_plugin_kinds(&self) -> Vec<String> {
        let mut kinds = std::collections::HashSet::new();
        for ws in &self.workspaces {
            Self::collect_kinds_in_pane(&ws.pane_layout, &mut kinds);
        }
        kinds.into_iter().collect()
    }

    fn collect_kinds_in_pane(
        node: &SavedPaneNode,
        out: &mut std::collections::HashSet<String>,
    ) {
        match node {
            SavedPaneNode::Leaf(pane) => {
                for tab in &pane.tabs {
                    Self::collect_kinds_in_layout(&tab.surface, out);
                }
            }
            SavedPaneNode::Split { first, second, .. } => {
                Self::collect_kinds_in_pane(first, out);
                Self::collect_kinds_in_pane(second, out);
            }
        }
    }

    fn collect_kinds_in_layout(
        layout: &SavedSurfaceLayout,
        out: &mut std::collections::HashSet<String>,
    ) {
        match layout {
            SavedSurfaceLayout::Leaf(SavedSurface::Generic { kind, .. }) => {
                out.insert(kind.clone());
            }
            SavedSurfaceLayout::Leaf(_) => {}
            SavedSurfaceLayout::Split { first, second, .. } => {
                Self::collect_kinds_in_layout(first, out);
                Self::collect_kinds_in_layout(second, out);
            }
        }
    }

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
    fn restore(self, engine: &mut EngineState, is_active_workspace: bool) -> Option<Pane> {
        let pane_id = engine.next_ids.next_pane();
        let saved_active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));
        let mut tabs = Vec::new();
        for (idx, saved_tab) in self.tabs.into_iter().enumerate() {
            // 활성 workspace 안에서도 사용자가 보고 있는 active_tab만 즉시 PTY spawn.
            // 나머지 tab은 비활성 workspace와 동일하게 deferred — tab 전환 시 깨워짐.
            let tab_is_active = is_active_workspace && idx == saved_active_tab;
            match saved_tab.restore(engine, tab_is_active) {
                Some(tab) => tabs.push(tab),
                None => {
                    tracing::warn!("Failed to restore tab, skipping");
                }
            }
        }
        if tabs.is_empty() {
            return None;
        }
        let active_tab = saved_active_tab.min(tabs.len() - 1);
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
            if let SavedSurface::Terminal {
                ref cwd,
                ref restore_command,
            } = self
            {
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
                if let Some(cmd) = restore_command {
                    engine.pending_restore_commands.push((surface_id, cmd.clone()));
                }
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
            SavedSurface::Terminal {
                cwd,
                restore_command,
            } => {
                let sh = ShellConfig::from_settings(&engine.settings);
                let waker = engine.make_waker(surface_id);
                let working_dir = cwd.as_ref().map(PathBuf::from);
                let terminal = match tasty_terminal::Terminal::new(
                    tasty_terminal::TerminalConfig {
                        cols: engine.default_cols,
                        rows: engine.default_rows,
                        shell: sh.shell_ref(),
                        args: &sh.args_ref(),
                        surface_id,
                        working_dir: working_dir.as_deref(),
                    },
                    waker,
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("Failed to create terminal for restored surface: {e}");
                        return None;
                    }
                };
                engine.send_fast_init(surface_id);
                if let Some(cmd) = restore_command {
                    engine.pending_restore_commands.push((surface_id, cmd));
                }
                Some(Box::new(TerminalSurface {
                    id: surface_id,
                    terminal,
                    deferred_spawn: None,
                }))
            }
            SavedSurface::Generic { kind, data } => {
                let registry = engine.surface_registry.clone();
                let def = match registry.get(&kind) {
                    Some(d) => d,
                    None => {
                        tracing::warn!(
                            "Generic restore skipped: unknown kind '{}' (plugin not loaded?)",
                            kind
                        );
                        return None;
                    }
                };
                match (def.restore)(surface_id, &data) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        tracing::warn!("Generic restore failed (kind={kind}): {e}");
                        None
                    }
                }
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

#[cfg(test)]
mod migration_tests {
    use super::*;

    fn parse(s: &str) -> SavedSurface {
        serde_json::from_str(s).unwrap_or_else(|e| panic!("parse failed for {s:?}: {e}"))
    }

    #[test]
    fn v1_markdown_migrates_to_generic() {
        match parse(r#"{"Markdown":{"path":"/tmp/x.md"}}"#) {
            SavedSurface::Generic { kind, data } => {
                assert_eq!(kind, "markdown");
                assert_eq!(data["path"], "/tmp/x.md");
            }
            other => panic!("expected Generic, got {:?}", serde_json::to_string(&other)),
        }
    }

    #[test]
    fn v1_explorer_renames_root_path_to_path() {
        match parse(r#"{"Explorer":{"root_path":"/home/u/proj"}}"#) {
            SavedSurface::Generic { kind, data } => {
                assert_eq!(kind, "explorer");
                assert_eq!(data["path"], "/home/u/proj");
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn v1_html_migrates() {
        match parse(r#"{"Html":{"url":"https://example.com"}}"#) {
            SavedSurface::Generic { kind, data } => {
                assert_eq!(kind, "html");
                assert_eq!(data["url"], "https://example.com");
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn v1_image_with_path_migrates() {
        match parse(r#"{"Image":{"path":"/tmp/p.png"}}"#) {
            SavedSurface::Generic { kind, data } => {
                assert_eq!(kind, "image");
                assert_eq!(data["path"], "/tmp/p.png");
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn v1_image_with_null_path_migrates_to_blank() {
        match parse(r#"{"Image":{"path":null}}"#) {
            SavedSurface::Generic { kind, data } => {
                assert_eq!(kind, "image");
                assert!(data["path"].is_null());
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn v1_empty_string_form_migrates() {
        // v1 unit variant serialised as bare "Empty".
        match parse(r#""Empty""#) {
            SavedSurface::Generic { kind, data } => {
                assert_eq!(kind, "empty");
                assert!(data.is_object());
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn v1_empty_object_form_migrates() {
        // {"Empty": {}} 또는 {"Empty": null} 형태도 가능.
        match parse(r#"{"Empty":{}}"#) {
            SavedSurface::Generic { kind, .. } => {
                assert_eq!(kind, "empty");
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn v2_terminal_round_trips() {
        let s = SavedSurface::Terminal {
            cwd: Some("/tmp".into()),
            restore_command: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        match parse(&json) {
            SavedSurface::Terminal { cwd, restore_command } => {
                assert_eq!(cwd.as_deref(), Some("/tmp"));
                assert!(restore_command.is_none());
            }
            _ => panic!("expected Terminal"),
        }
    }

    #[test]
    fn v2_generic_round_trips() {
        let s = SavedSurface::Generic {
            kind: "markdown".into(),
            data: json!({"path": "/x.md"}),
        };
        let json = serde_json::to_string(&s).unwrap();
        match parse(&json) {
            SavedSurface::Generic { kind, data } => {
                assert_eq!(kind, "markdown");
                assert_eq!(data["path"], "/x.md");
            }
            _ => panic!("expected Generic"),
        }
    }

    #[test]
    fn unknown_variant_is_rejected() {
        let result: Result<SavedSurface, _> =
            serde_json::from_str(r#"{"Bogus":{}}"#);
        assert!(result.is_err());
    }
}
