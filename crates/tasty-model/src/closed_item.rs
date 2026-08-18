use std::collections::VecDeque;
use std::path::PathBuf;

use tasty_terminal::ScrollbackLine;
use termwiz::cell::CellAttributes;

use super::{PaneId, SplitDirection, Surface, SurfaceId, TabId, WorkspaceId};

/// `&dyn Surface` → snapshot JSON. `None`이면 영속화에서 제외(휘발성 surface).
/// 호출자가 `SurfaceKindRegistry`를 캡처해 넘긴다 — core는 registry 타입을 알지 않는다.
pub type SnapshotFn<'a> = &'a mut dyn FnMut(&dyn Surface) -> Option<serde_json::Value>;

/// Maximum number of closed items to keep.
const MAX_CLOSED_ITEMS: usize = 10;

/// `surface_id` → `&Terminal` 매핑 함수. 캡처 시점에 `TerminalStore` 가
/// 참조로 들어오면 `&|id| store.get(id)` 같은 closure 로 wrapping 한다.
pub type TerminalLookup<'a> = dyn Fn(SurfaceId) -> Option<&'a tasty_terminal::Terminal> + 'a;

/// Scrollback payload of a closed surface.
///
/// A freshly-captured surface holds [`Inline`](ClosedScrollback::Inline) — a
/// transient in-memory copy that lives only until [`persist_closed_scrollback`]
/// runs (during the host's `push_closed_item`). Once persisted, the closed item
/// retains only a [`Persisted`](ClosedScrollback::Persisted) reference into
/// `~/.tasty/scrollback/`, so its retained scrollback cost is a single id
/// string instead of up to 10k lines per surface.
/// [`Empty`](ClosedScrollback::Empty) means the surface had no scrollback.
pub enum ClosedScrollback {
    Empty,
    Inline(VecDeque<ScrollbackLine>),
    Persisted(String),
}

/// Snapshot of a surface's content at close time.
pub struct ClosedSurface {
    pub id: SurfaceId,
    pub cwd: Option<PathBuf>,
    /// Command to re-launch the TUI app that was running (e.g. "claude -r <session-id>").
    pub restore_command: Option<String>,
    /// Screen content: rows of (text, attrs) cells.
    pub screen: Vec<Vec<(String, CellAttributes)>>,
    /// Scrollback buffer. Captured `Inline`, then persisted to disk and held as
    /// a `Persisted(persist_id)` reference (see [`ClosedScrollback`]).
    pub scrollback: ClosedScrollback,
}

/// Snapshot of a closed panel (terminal, tab with split surfaces, etc).
pub enum ClosedPanel {
    Terminal(ClosedSurface),
    Tab {
        layout: ClosedSurfaceLayout,
        focused_surface: SurfaceId,
    },
    /// Non-terminal surface — captured via SurfaceKindRegistry snapshot. `kind`은
    /// surface kind 식별자(예: `"markdown"`, `"image"`), `snapshot`은 해당 kind의
    /// `restore` 콜백이 받을 JSON. core는 kind별 분기를 알지 않는다.
    Generic {
        kind: String,
        snapshot: serde_json::Value,
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
    /// A whole pane (removed via the last-tab-in-pane cascade or the
    /// dedicated `close_pane` shortcut). `sibling_pane_id`/`direction`/
    /// `ratio`/`was_first` capture the split geometry at close time — the
    /// pane itself is always a direct leaf child of some `Split` node (see
    /// `PaneNode::close_pane`), so this is enough to splice it back in via
    /// `PaneNode::insert_pane_beside` on restore. `sibling_pane_id` is a
    /// still-live anchor pane on the *other* side of that split; if it no
    /// longer exists at restore time, restoration falls back to splitting
    /// the caller's currently focused pane instead.
    Pane {
        pane: ClosedPane,
        sibling_pane_id: PaneId,
        direction: SplitDirection,
        ratio: f32,
        was_first: bool,
    },
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
    /// Capture a snapshot from a TerminalSurface marker + its Terminal in the store.
    /// `terminal` 가 None 이면 empty snapshot 만.
    pub fn from_surface_id(id: SurfaceId, terminal: Option<&tasty_terminal::Terminal>) -> Self {
        Self::from_surface_id_with_restore(id, terminal, None)
    }

    /// Capture a snapshot with an optional restore command (e.g. "claude -r <session-id>").
    pub fn from_surface_id_with_restore(
        id: SurfaceId,
        terminal: Option<&tasty_terminal::Terminal>,
        restore_command: Option<String>,
    ) -> Self {
        let Some(terminal) = terminal else {
            return Self {
                id,
                cwd: None,
                restore_command,
                screen: Vec::new(),
                scrollback: ClosedScrollback::Empty,
            };
        };
        let lines = terminal.screen_lines();

        let screen: Vec<Vec<(String, CellAttributes)>> = lines
            .iter()
            .map(|line| {
                line.visible_cells()
                    .map(|cell| (cell.str().to_string(), cell.attrs().clone()))
                    .collect()
            })
            .collect();

        // Capture scrollback as a transient inline copy. The host's
        // `push_closed_item` persists it to disk (see `persist_closed_scrollback`)
        // and replaces it with a lightweight reference, so the retained closed
        // item does not hold the full scrollback in memory.
        let scrollback_len = terminal.scrollback_len();
        let mut scrollback = VecDeque::with_capacity(scrollback_len);
        for i in 0..scrollback_len {
            if let Some(line) = terminal.scrollback_line_full(i) {
                scrollback.push_back(line);
            }
        }
        let scrollback = if scrollback.is_empty() {
            ClosedScrollback::Empty
        } else {
            ClosedScrollback::Inline(scrollback)
        };

        Self {
            id,
            cwd: terminal.get_cwd(),
            restore_command,
            screen,
            scrollback,
        }
    }
}

impl ClosedSurfaceLayout {
    /// Capture from a live SurfaceLayout. `terminal_lookup` 은 surface_id ↔ Terminal
    /// 매핑 — 보통 `&engine.terminals` 의 wrapping closure.
    pub fn from_layout(
        layout: &super::SurfaceLayout,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Self {
        match layout {
            super::SurfaceLayout::Leaf(surface) => {
                if let Some(node) = surface.as_any().downcast_ref::<super::TerminalSurface>() {
                    ClosedSurfaceLayout::Single(ClosedSurface::from_surface_id(
                        node.id,
                        terminal_lookup(node.id),
                    ))
                } else {
                    // Non-terminal surfaces: store minimal placeholder with the surface ID.
                    ClosedSurfaceLayout::Single(ClosedSurface {
                        id: surface.surface_id().unwrap_or(0),
                        cwd: None,
                        restore_command: None,
                        screen: Vec::new(),
                        scrollback: ClosedScrollback::Empty,
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
                first: Box::new(Self::from_layout(first, terminal_lookup)),
                second: Box::new(Self::from_layout(second, terminal_lookup)),
            },
        }
    }
}

impl ClosedPanel {
    /// Capture from a live Tab. `terminal_lookup` 은 surface_id ↔ Terminal 매핑.
    pub fn from_tab(
        tab: &super::tab::Tab,
        snapshot: SnapshotFn<'_>,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Option<Self> {
        if tab.is_split() {
            return Some(ClosedPanel::Tab {
                layout: ClosedSurfaceLayout::from_layout(tab.layout(), terminal_lookup),
                focused_surface: tab.focused_surface,
            });
        }
        // Single surface tab
        let surface = tab.surface();
        Self::from_surface(surface, snapshot, terminal_lookup)
    }

    /// Capture from a single Surface (trait object). Terminal 은 PTY 로직이 별도라
    /// 직접 처리하고, 그 외는 모두 `snapshot` 클로저를 통해 registry 경로로 간다.
    /// 클로저가 `None`을 반환하면 (Html/Empty/RemoteSurface 등 휘발성)
    /// 함수도 `None`을 반환한다.
    pub fn from_surface(
        surface: &dyn Surface,
        snapshot: SnapshotFn<'_>,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Option<Self> {
        if let Some(node) = surface.as_any().downcast_ref::<super::TerminalSurface>() {
            return Some(ClosedPanel::Terminal(ClosedSurface::from_surface_id(
                node.id,
                terminal_lookup(node.id),
            )));
        }
        let snap = snapshot(surface)?;
        Some(ClosedPanel::Generic {
            kind: surface.kind().to_string(),
            snapshot: snap,
        })
    }
}

impl ClosedTab {
    /// Capture from a live Tab. Returns `None` if the tab's surface is not
    /// restorable (plugin RemoteSurface 등).
    pub fn from_tab(
        tab: &super::tab::Tab,
        snapshot: SnapshotFn<'_>,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Option<Self> {
        let panel = ClosedPanel::from_tab(tab, snapshot, terminal_lookup)?;
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
    pub fn from_pane(
        pane: &super::Pane,
        snapshot: SnapshotFn<'_>,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Self {
        Self {
            id: pane.id,
            tabs: pane
                .tabs
                .iter()
                .filter_map(|t| ClosedTab::from_tab(t, snapshot, terminal_lookup))
                .collect(),
            active_tab: pane.active_tab,
        }
    }
}

impl ClosedPaneNode {
    /// Capture from a live PaneNode.
    pub fn from_pane_node(
        node: &super::PaneNode,
        snapshot: SnapshotFn<'_>,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Self {
        match node {
            super::PaneNode::Leaf(pane) => {
                ClosedPaneNode::Leaf(ClosedPane::from_pane(pane, snapshot, terminal_lookup))
            }
            super::PaneNode::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => ClosedPaneNode::Split {
                direction: *direction,
                ratio: *ratio,
                first: Box::new(Self::from_pane_node(first, snapshot, terminal_lookup)),
                second: Box::new(Self::from_pane_node(second, snapshot, terminal_lookup)),
            },
        }
    }
}

impl ClosedItem {
    /// Capture a snapshot of a closed pane, together with the split geometry
    /// (`sibling_pane_id`/`direction`/`ratio`/`was_first`) needed to splice it
    /// back into roughly the same tree position on restore. Callers compute
    /// that geometry via `PaneNode::locate_split_context` *before* removing
    /// the pane from the tree (the parent `Split` node disappears once
    /// `PaneNode::close_pane` runs).
    #[allow(clippy::too_many_arguments)] // reason: 1:1 mirror of locate_split_context's tuple
    pub fn from_pane(
        pane: &super::Pane,
        sibling_pane_id: PaneId,
        direction: SplitDirection,
        ratio: f32,
        was_first: bool,
        snapshot: SnapshotFn<'_>,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Self {
        ClosedItem::Pane {
            pane: ClosedPane::from_pane(pane, snapshot, terminal_lookup),
            sibling_pane_id,
            direction,
            ratio,
            was_first,
        }
    }

    /// Capture a workspace snapshot.
    pub fn from_workspace(
        ws: &super::Workspace,
        snapshot: SnapshotFn<'_>,
        terminal_lookup: &TerminalLookup<'_>,
    ) -> Self {
        ClosedItem::Workspace {
            id: ws.id,
            name: ws.name.clone(),
            subtitle: ws.subtitle.clone(),
            pane_layout: ClosedPaneNode::from_pane_node(
                ws.pane_layout(),
                snapshot,
                terminal_lookup,
            ),
            focused_pane: ws.focused_pane,
        }
    }
}

/// Inject restore_command into all ClosedSurface nodes using a lookup function.
/// Called after capture to populate restore commands from surface metadata.
pub fn inject_restore_commands(
    item: &mut ClosedItem,
    lookup: &dyn Fn(SurfaceId) -> Option<String>,
) {
    match item {
        ClosedItem::Surface { surface, .. } => {
            surface.restore_command = lookup(surface.id);
        }
        ClosedItem::Tab(tab) => inject_into_panel(&mut tab.panel, lookup),
        ClosedItem::Pane { pane, .. } => {
            for tab in &mut pane.tabs {
                inject_into_panel(&mut tab.panel, lookup);
            }
        }
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

fn inject_into_surface_layout(
    layout: &mut ClosedSurfaceLayout,
    lookup: &dyn Fn(SurfaceId) -> Option<String>,
) {
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

// ── Scrollback persistence: host-driven post-pass over a ClosedItem ──
//
// `tasty-model` cannot reach the host's disk store (`src/store/scrollback.rs`),
// so the host supplies the I/O via closures and these walkers apply it across
// every `ClosedSurface` in the tree — mirroring `inject_restore_commands`.

/// Visit every [`ClosedSurface`] in a [`ClosedItem`] mutably.
fn visit_surfaces_mut(item: &mut ClosedItem, f: &mut dyn FnMut(&mut ClosedSurface)) {
    match item {
        ClosedItem::Surface { surface, .. } => f(surface),
        ClosedItem::Tab(tab) => visit_panel_mut(&mut tab.panel, f),
        ClosedItem::Pane { pane, .. } => {
            for tab in &mut pane.tabs {
                visit_panel_mut(&mut tab.panel, f);
            }
        }
        ClosedItem::Workspace { pane_layout, .. } => visit_pane_node_mut(pane_layout, f),
    }
}

fn visit_panel_mut(panel: &mut ClosedPanel, f: &mut dyn FnMut(&mut ClosedSurface)) {
    match panel {
        ClosedPanel::Terminal(s) => f(s),
        ClosedPanel::Tab { layout, .. } => visit_surface_layout_mut(layout, f),
        ClosedPanel::Generic { .. } => {}
    }
}

fn visit_surface_layout_mut(
    layout: &mut ClosedSurfaceLayout,
    f: &mut dyn FnMut(&mut ClosedSurface),
) {
    match layout {
        ClosedSurfaceLayout::Single(s) => f(s),
        ClosedSurfaceLayout::Split { first, second, .. } => {
            visit_surface_layout_mut(first, f);
            visit_surface_layout_mut(second, f);
        }
    }
}

fn visit_pane_node_mut(node: &mut ClosedPaneNode, f: &mut dyn FnMut(&mut ClosedSurface)) {
    match node {
        ClosedPaneNode::Leaf(pane) => {
            for tab in &mut pane.tabs {
                visit_panel_mut(&mut tab.panel, f);
            }
        }
        ClosedPaneNode::Split { first, second, .. } => {
            visit_pane_node_mut(first, f);
            visit_pane_node_mut(second, f);
        }
    }
}

/// Visit every [`ClosedSurface`] in a [`ClosedItem`] by shared reference.
fn visit_surfaces(item: &ClosedItem, f: &mut dyn FnMut(&ClosedSurface)) {
    match item {
        ClosedItem::Surface { surface, .. } => f(surface),
        ClosedItem::Tab(tab) => visit_panel(&tab.panel, f),
        ClosedItem::Pane { pane, .. } => {
            for tab in &pane.tabs {
                visit_panel(&tab.panel, f);
            }
        }
        ClosedItem::Workspace { pane_layout, .. } => visit_pane_node(pane_layout, f),
    }
}

fn visit_panel(panel: &ClosedPanel, f: &mut dyn FnMut(&ClosedSurface)) {
    match panel {
        ClosedPanel::Terminal(s) => f(s),
        ClosedPanel::Tab { layout, .. } => visit_surface_layout(layout, f),
        ClosedPanel::Generic { .. } => {}
    }
}

fn visit_surface_layout(layout: &ClosedSurfaceLayout, f: &mut dyn FnMut(&ClosedSurface)) {
    match layout {
        ClosedSurfaceLayout::Single(s) => f(s),
        ClosedSurfaceLayout::Split { first, second, .. } => {
            visit_surface_layout(first, f);
            visit_surface_layout(second, f);
        }
    }
}

fn visit_pane_node(node: &ClosedPaneNode, f: &mut dyn FnMut(&ClosedSurface)) {
    match node {
        ClosedPaneNode::Leaf(pane) => {
            for tab in &pane.tabs {
                visit_panel(&tab.panel, f);
            }
        }
        ClosedPaneNode::Split { first, second, .. } => {
            visit_pane_node(first, f);
            visit_pane_node(second, f);
        }
    }
}

/// Persist every surface's `Inline` scrollback to disk (via `persist`, which
/// returns a `persist_id`) and replace it with a `Persisted` reference,
/// dropping the in-memory copy. Empty entries collapse to `Empty`; if `persist`
/// returns `None` (write failed) the `Inline` copy is kept so restore still
/// works from memory. Called by the host once per close, after capture.
pub fn persist_closed_scrollback(
    item: &mut ClosedItem,
    persist: &mut dyn FnMut(&[ScrollbackLine]) -> Option<String>,
) {
    visit_surfaces_mut(item, &mut |s| {
        let taken = std::mem::replace(&mut s.scrollback, ClosedScrollback::Empty);
        s.scrollback = match taken {
            ClosedScrollback::Inline(mut lines) if !lines.is_empty() => {
                match persist(lines.make_contiguous()) {
                    Some(id) => ClosedScrollback::Persisted(id),
                    None => ClosedScrollback::Inline(lines),
                }
            }
            ClosedScrollback::Inline(_) => ClosedScrollback::Empty,
            other => other,
        };
    });
}

/// Collect every `Persisted` scrollback reference in a [`ClosedItem`] (used by
/// the host to delete the backing files when an item is evicted).
pub fn collect_scrollback_refs(item: &ClosedItem, out: &mut Vec<String>) {
    visit_surfaces(item, &mut |s| {
        if let ClosedScrollback::Persisted(id) = &s.scrollback {
            out.push(id.clone());
        }
    });
}

/// 스냅샷 규모 — close 계측(`tasty::close` C1)이 "ms 가 무엇에 비례하는가" 를
/// 판정하려면 surface 수와 스크롤백 라인 수가 함께 필요하다.
#[derive(Default, Clone, Copy)]
pub struct SnapshotExtent {
    /// 스냅샷에 담긴 surface 수.
    pub surfaces: u64,
    /// 아직 인라인(메모리)으로 들고 있는 스크롤백 라인 총합. 디스크로 내려간
    /// (`Persisted`) 뒤에 세면 0 이 되므로 **`persist_closed_scrollback` 전에**
    /// 호출해야 의미가 있다.
    pub scrollback_lines: u64,
}

/// 캡처 직후의 [`ClosedItem`] 규모를 센다.
pub fn snapshot_extent(item: &ClosedItem) -> SnapshotExtent {
    let mut out = SnapshotExtent::default();
    visit_surfaces(item, &mut |s| {
        out.surfaces += 1;
        if let ClosedScrollback::Inline(lines) = &s.scrollback {
            out.scrollback_lines += lines.len() as u64;
        }
    });
    out
}

/// LIFO store for recently closed items.
pub struct ClosedItemStore {
    items: VecDeque<ClosedItem>,
}

impl Default for ClosedItemStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ClosedItemStore {
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
        }
    }

    /// Push a closed item. Returns the item evicted when the store is already at
    /// `MAX_CLOSED_ITEMS`, so the host can release its backing scrollback files.
    #[must_use = "evicted item may own disk-backed scrollback that needs cleanup"]
    pub fn push(&mut self, item: ClosedItem) -> Option<ClosedItem> {
        let evicted = if self.items.len() >= MAX_CLOSED_ITEMS {
            self.items.pop_front() // Drop oldest
        } else {
            None
        };
        self.items.push_back(item);
        evicted
    }

    pub fn pop(&mut self) -> Option<ClosedItem> {
        self.items.pop_back()
    }

    /// 라이브러리 표준 accessor — "복원 가능 항목 없음" UI 분기 후보.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> ScrollbackLine {
        ScrollbackLine::new(vec![(text.to_string(), CellAttributes::default())], false)
    }

    fn surface_item(id: SurfaceId, scrollback: ClosedScrollback) -> ClosedItem {
        ClosedItem::Surface {
            surface: ClosedSurface {
                id,
                cwd: None,
                restore_command: None,
                screen: Vec::new(),
                scrollback,
            },
            tab_name: String::new(),
        }
    }

    fn scrollback_of(item: &ClosedItem) -> &ClosedScrollback {
        match item {
            ClosedItem::Surface { surface, .. } => &surface.scrollback,
            _ => panic!("expected Surface"),
        }
    }

    #[test]
    fn persist_replaces_inline_with_reference_and_drops_lines() {
        let inline: VecDeque<ScrollbackLine> = [line("a"), line("b")].into();
        let mut item = surface_item(1, ClosedScrollback::Inline(inline));

        let mut captured_len = 0usize;
        persist_closed_scrollback(&mut item, &mut |lines| {
            captured_len = lines.len();
            Some("ref-1".to_string())
        });

        // Inline copy is gone; only a reference remains.
        assert_eq!(captured_len, 2);
        match scrollback_of(&item) {
            ClosedScrollback::Persisted(id) => assert_eq!(id, "ref-1"),
            _ => panic!("expected Persisted"),
        }

        // collect_scrollback_refs surfaces the reference for cleanup.
        let mut refs = Vec::new();
        collect_scrollback_refs(&item, &mut refs);
        assert_eq!(refs, vec!["ref-1".to_string()]);
    }

    #[test]
    fn persist_normalizes_empty_inline_and_keeps_inline_on_write_failure() {
        // Empty inline → Empty (no reference, no persist call).
        let mut empty = surface_item(1, ClosedScrollback::Inline(VecDeque::new()));
        persist_closed_scrollback(&mut empty, &mut |_| panic!("must not persist empty"));
        assert!(matches!(scrollback_of(&empty), ClosedScrollback::Empty));

        // Write failure (None) keeps the Inline copy so restore still works.
        let mut item = surface_item(2, ClosedScrollback::Inline([line("x")].into()));
        persist_closed_scrollback(&mut item, &mut |_| None);
        match scrollback_of(&item) {
            ClosedScrollback::Inline(lines) => assert_eq!(lines.len(), 1),
            _ => panic!("expected Inline kept on failure"),
        }
        // No reference to clean up when nothing was persisted.
        let mut refs = Vec::new();
        collect_scrollback_refs(&item, &mut refs);
        assert!(refs.is_empty());
    }

    #[test]
    fn push_returns_evicted_item_over_capacity() {
        let mut store = ClosedItemStore::new();
        for i in 0..MAX_CLOSED_ITEMS {
            assert!(
                store
                    .push(surface_item(i as u32, ClosedScrollback::Empty))
                    .is_none()
            );
        }
        // The (MAX+1)-th push evicts the oldest (id 0) so its files can be freed.
        let evicted = store
            .push(surface_item(
                999,
                ClosedScrollback::Persisted("ref-evicted".into()),
            ))
            .expect("eviction over capacity");
        match evicted {
            ClosedItem::Surface { surface, .. } => assert_eq!(surface.id, 0),
            _ => panic!("expected Surface"),
        }
        assert_eq!(store.len(), MAX_CLOSED_ITEMS);
    }
}
