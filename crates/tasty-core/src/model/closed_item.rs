use std::collections::VecDeque;
use std::path::PathBuf;

use termwiz::cell::CellAttributes;

use super::{PaneId, SplitDirection, SurfaceId, TabId, WorkspaceId};

/// Maximum number of closed items to keep.
const MAX_CLOSED_ITEMS: usize = 10;

/// Snapshot of a surface's content at close time.
pub struct ClosedSurface {
    pub id: SurfaceId,
    pub cwd: Option<PathBuf>,
    /// Command to re-launch the TUI app that was running (e.g. "claude -r <session-id>").
    pub restore_command: Option<String>,
    /// Screen content: rows of (text, attrs) cells.
    pub screen: Vec<Vec<(String, CellAttributes)>>,
    /// Scrollback buffer (oldest first).
    pub scrollback: VecDeque<Vec<(String, CellAttributes)>>,
}

/// Snapshot of a closed panel (terminal, tab with split surfaces, etc).
pub enum ClosedPanel {
    Terminal(ClosedSurface),
    Tab {
        layout: ClosedSurfaceLayout,
        focused_surface: SurfaceId,
    },
    /// Panels without PTY store just enough to recreate.
    Markdown {
        path: PathBuf,
    },
    Image {
        path: Option<PathBuf>,
    },
}

/// Mirrors SurfaceLayout but with ClosedSurface instead of live Terminal.
pub enum ClosedSurfaceLayout {
    Single(ClosedSurface),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<ClosedSurfaceLayout>,
        second: Box<ClosedSurfaceLayout>,
    },
}

/// Snapshot of a closed tab.
pub struct ClosedTab {
    pub id: TabId,
    pub name: String,
    pub explicit_name: Option<String>,
    pub panel: ClosedPanel,
}

/// Snapshot of a closed pane tree (mirrors PaneNode).
pub enum ClosedPaneNode {
    Leaf(ClosedPane),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<ClosedPaneNode>,
        second: Box<ClosedPaneNode>,
    },
}

/// Snapshot of a closed pane.
pub struct ClosedPane {
    pub id: PaneId,
    pub tabs: Vec<ClosedTab>,
    pub active_tab: usize,
}

/// A recently closed item, ready for restoration.
pub enum ClosedItem {
    Surface {
        surface: ClosedSurface,
        /// The tab name this surface belonged to.
        tab_name: String,
    },
    Tab(ClosedTab),
    Workspace {
        id: WorkspaceId,
        name: String,
        subtitle: String,
        pane_layout: ClosedPaneNode,
        focused_pane: PaneId,
    },
}

// ── Capture functions: live model → closed snapshot ──

impl ClosedSurface {
    /// Capture a snapshot from a live TerminalSurface.
    pub fn from_surface_node(node: &super::TerminalSurface) -> Self {
        Self::from_surface_node_with_restore(node, None)
    }

    /// Capture a snapshot with an optional restore command (e.g. "claude -r <session-id>").
    pub fn from_surface_node_with_restore(node: &super::TerminalSurface, restore_command: Option<String>) -> Self {
        let terminal = &node.terminal;
        let surface = terminal.surface();
        let lines = surface.screen_lines();

        let screen: Vec<Vec<(String, CellAttributes)>> = lines
            .iter()
            .map(|line| {
                line.visible_cells()
                    .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                    .collect()
            })
            .collect();

        // Move scrollback data (capture owned copies)
        let scrollback_len = terminal.scrollback_len();
        let mut scrollback = VecDeque::with_capacity(scrollback_len);
        for i in 0..scrollback_len {
            if let Some(line) = terminal.scrollback_line_owned(i) {
                scrollback.push_back(line);
            }
        }

        Self {
            id: node.id,
            cwd: terminal.get_cwd(),
            restore_command,
            screen,
            scrollback,
        }
    }
}

impl ClosedSurfaceLayout {
    /// Capture from a live SurfaceLayout.
    pub fn from_layout(layout: &super::SurfaceLayout) -> Self {
        match layout {
            super::SurfaceLayout::Leaf(surface) => {
                if let Some(node) = surface.as_terminal_surface() {
                    ClosedSurfaceLayout::Single(ClosedSurface::from_surface_node(node))
                } else {
                    // Non-terminal surfaces: store minimal placeholder with the surface ID.
                    ClosedSurfaceLayout::Single(ClosedSurface {
                        id: surface.surface_id().unwrap_or(0),
                        cwd: None,
                        restore_command: None,
                        screen: Vec::new(),
                        scrollback: VecDeque::new(),
                    })
                }
            }
            super::SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => ClosedSurfaceLayout::Split {
                direction: *direction,
                ratio: *ratio,
                first: Box::new(Self::from_layout(first)),
                second: Box::new(Self::from_layout(second)),
            },
        }
    }
}

impl ClosedPanel {
    /// Capture from a live Tab. Returns `None` for surfaces that are not
    /// restorable by the host (plugin-provided RemoteSurface, Html, Empty 등).
    pub fn from_tab(tab: &super::tab::Tab) -> Option<Self> {
        if tab.is_split() {
            return Some(ClosedPanel::Tab {
                layout: ClosedSurfaceLayout::from_layout(tab.layout()),
                focused_surface: tab.focused_surface,
            });
        }
        // Single surface tab
        let surface = tab.surface();
        Self::from_surface(surface)
    }

    /// Capture from a single Surface (trait object). Returns `None` if the
    /// surface kind is not restorable (e.g. plugin RemoteSurface, Html, Empty,
    /// ClipboardViewer).
    pub fn from_surface(surface: &dyn super::Surface) -> Option<Self> {
        if let Some(node) = surface.as_terminal_surface() {
            return Some(ClosedPanel::Terminal(ClosedSurface::from_surface_node(node)));
        }
        let any = surface.as_any();
        if let Some(md) = any.downcast_ref::<super::MarkdownPanel>() {
            return Some(ClosedPanel::Markdown {
                path: PathBuf::from(&md.file_path),
            });
        }
        if let Some(img) = any.downcast_ref::<super::ImagePanel>() {
            return Some(ClosedPanel::Image {
                path: img.file_path.as_ref().map(PathBuf::from),
            });
        }
        // Html, Empty, ClipboardViewer, RemoteSurface — not restorable by host.
        None
    }
}

impl ClosedTab {
    /// Capture from a live Tab. Returns `None` if the tab's surface is not
    /// restorable (plugin RemoteSurface 등).
    pub fn from_tab(tab: &super::tab::Tab) -> Option<Self> {
        let panel = ClosedPanel::from_tab(tab)?;
        Some(Self {
            id: tab.id,
            name: tab.name.clone(),
            explicit_name: tab.explicit_name.clone(),
            panel,
        })
    }
}

impl ClosedPane {
    /// Capture from a live Pane. Tabs that are not restorable are skipped.
    pub fn from_pane(pane: &super::Pane) -> Self {
        Self {
            id: pane.id,
            tabs: pane.tabs.iter().filter_map(ClosedTab::from_tab).collect(),
            active_tab: pane.active_tab,
        }
    }
}

impl ClosedPaneNode {
    /// Capture from a live PaneNode.
    pub fn from_pane_node(node: &super::PaneNode) -> Self {
        match node {
            super::PaneNode::Leaf(pane) => ClosedPaneNode::Leaf(ClosedPane::from_pane(pane)),
            super::PaneNode::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => ClosedPaneNode::Split {
                direction: *direction,
                ratio: *ratio,
                first: Box::new(Self::from_pane_node(first)),
                second: Box::new(Self::from_pane_node(second)),
            },
        }
    }
}

impl ClosedItem {
    /// Capture a workspace snapshot.
    pub fn from_workspace(ws: &super::Workspace) -> Self {
        ClosedItem::Workspace {
            id: ws.id,
            name: ws.name.clone(),
            subtitle: ws.subtitle.clone(),
            pane_layout: ClosedPaneNode::from_pane_node(ws.pane_layout()),
            focused_pane: ws.focused_pane,
        }
    }
}

/// Inject restore_command into all ClosedSurface nodes using a lookup function.
/// Called after capture to populate restore commands from surface metadata.
pub fn inject_restore_commands(item: &mut ClosedItem, lookup: &dyn Fn(SurfaceId) -> Option<String>) {
    match item {
        ClosedItem::Surface { surface, .. } => {
            surface.restore_command = lookup(surface.id);
        }
        ClosedItem::Tab(tab) => inject_into_panel(&mut tab.panel, lookup),
        ClosedItem::Workspace { pane_layout, .. } => {
            inject_into_pane_node(pane_layout, lookup);
        }
    }
}

fn inject_into_panel(panel: &mut ClosedPanel, lookup: &dyn Fn(SurfaceId) -> Option<String>) {
    match panel {
        ClosedPanel::Terminal(s) => {
            s.restore_command = lookup(s.id);
        }
        ClosedPanel::Tab { layout, .. } => inject_into_surface_layout(layout, lookup),
        _ => {}
    }
}

fn inject_into_surface_layout(layout: &mut ClosedSurfaceLayout, lookup: &dyn Fn(SurfaceId) -> Option<String>) {
    match layout {
        ClosedSurfaceLayout::Single(s) => {
            s.restore_command = lookup(s.id);
        }
        ClosedSurfaceLayout::Split { first, second, .. } => {
            inject_into_surface_layout(first, lookup);
            inject_into_surface_layout(second, lookup);
        }
    }
}

fn inject_into_pane_node(node: &mut ClosedPaneNode, lookup: &dyn Fn(SurfaceId) -> Option<String>) {
    match node {
        ClosedPaneNode::Leaf(pane) => {
            for tab in &mut pane.tabs {
                inject_into_panel(&mut tab.panel, lookup);
            }
        }
        ClosedPaneNode::Split { first, second, .. } => {
            inject_into_pane_node(first, lookup);
            inject_into_pane_node(second, lookup);
        }
    }
}

/// LIFO store for recently closed items.
pub struct ClosedItemStore {
    items: VecDeque<ClosedItem>,
}

impl ClosedItemStore {
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
        }
    }

    pub fn push(&mut self, item: ClosedItem) {
        if self.items.len() >= MAX_CLOSED_ITEMS {
            self.items.pop_front(); // Drop oldest
        }
        self.items.push_back(item);
    }

    pub fn pop(&mut self) -> Option<ClosedItem> {
        self.items.pop_back()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// List items for display (newest first).
    pub fn list(&self) -> impl Iterator<Item = &ClosedItem> {
        self.items.iter().rev()
    }
}
