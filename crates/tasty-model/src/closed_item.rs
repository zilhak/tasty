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

/// 닫힌 surface의 스크롤백. 캡처 직후 Inline이고 호스트가 저장에 성공하면
/// Persisted ID만 남긴다. 저장 실패 때는 복원을 위해 Inline을 유지한다.
/// Empty는 스크롤백이 없음을 뜻한다.
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
    /// 닫힌 pane과 분할 위치. 복원은 저장한 sibling_pane_id 옆에 다시 삽입하며
    /// 그 pane이 없으면 호출자의 포커스 pane을 대신 사용한다.
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

        // 호스트가 닫기 후 디스크로 옮기기 전의 임시 복사본이다.
        // 줄마다 terminal mutex를 잠그지 않도록 한 번에 읽는다.
        let scrollback: VecDeque<ScrollbackLine> = terminal.scrollback_lines_all().into();
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
        let surface = tab.surface();
        Self::from_surface(surface, snapshot, terminal_lookup)
    }

    /// Terminal은 직접 캡처하고 나머지는 호스트의 snapshot 콜백을 사용한다.
    /// 콜백이 None이면 복원 목록에서 제외한다.
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

// 디스크 I/O는 호스트 콜백으로 받아 모델의 각 ClosedSurface에 적용한다.

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

/// 닫기 시간과 함께 기록할 snapshot 크기.
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

/// 복원 항목과 원래 workspace. 로컬·원격 사용자가 스택을 공유하므로
/// mirror 복원 요청이 다른 workspace의 항목을 가져가지 않도록 출처를 보관한다.
pub struct ClosedEntry {
    /// 닫힐 때의 workspace. workspace 전체를 닫은 항목은 None이라 workspace별 복원에서 제외한다.
    pub origin_workspace: Option<WorkspaceId>,
    pub item: ClosedItem,
}

/// LIFO store for recently closed items.
pub struct ClosedItemStore {
    items: VecDeque<ClosedEntry>,
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

    /// Push a closed item together with the workspace it was closed in (`None`
    /// for a whole-workspace item). Returns the item evicted when the store is
    /// already at `MAX_CLOSED_ITEMS`, so the host can release its backing
    /// scrollback files.
    #[must_use = "evicted item may own disk-backed scrollback that needs cleanup"]
    pub fn push(
        &mut self,
        item: ClosedItem,
        origin_workspace: Option<WorkspaceId>,
    ) -> Option<ClosedItem> {
        let evicted = if self.items.len() >= MAX_CLOSED_ITEMS {
            self.items.pop_front() // Drop oldest
        } else {
            None
        };
        self.items.push_back(ClosedEntry {
            origin_workspace,
            item,
        });
        evicted.map(|e| e.item)
    }

    /// 최근 항목부터 찾아 keep(origin_workspace)가 참인 첫 항목을 꺼낸다.
    /// 호출자가 요청의 workspace 범위를 명시하도록 전역 pop은 제공하지 않는다.
    pub fn pop_matching(
        &mut self,
        keep: impl Fn(Option<WorkspaceId>) -> bool,
    ) -> Option<ClosedItem> {
        let idx = self.items.iter().rposition(|e| keep(e.origin_workspace))?;
        self.items.remove(idx).map(|e| e.item)
    }

    /// 복원 가능한 항목이 없는지 확인한다.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// List items for display (newest first).
    pub fn list(&self) -> impl Iterator<Item = &ClosedItem> {
        self.items.iter().rev().map(|e| &e.item)
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

        assert_eq!(captured_len, 2);
        match scrollback_of(&item) {
            ClosedScrollback::Persisted(id) => assert_eq!(id, "ref-1"),
            _ => panic!("expected Persisted"),
        }

        let mut refs = Vec::new();
        collect_scrollback_refs(&item, &mut refs);
        assert_eq!(refs, vec!["ref-1".to_string()]);
    }

    #[test]
    fn persist_normalizes_empty_inline_and_keeps_inline_on_write_failure() {
        let mut empty = surface_item(1, ClosedScrollback::Inline(VecDeque::new()));
        persist_closed_scrollback(&mut empty, &mut |_| panic!("must not persist empty"));
        assert!(matches!(scrollback_of(&empty), ClosedScrollback::Empty));

        let mut item = surface_item(2, ClosedScrollback::Inline([line("x")].into()));
        persist_closed_scrollback(&mut item, &mut |_| None);
        match scrollback_of(&item) {
            ClosedScrollback::Inline(lines) => assert_eq!(lines.len(), 1),
            _ => panic!("expected Inline kept on failure"),
        }
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
                    .push(surface_item(i as u32, ClosedScrollback::Empty), Some(1))
                    .is_none()
            );
        }
        let evicted = store
            .push(
                surface_item(999, ClosedScrollback::Persisted("ref-evicted".into())),
                Some(1),
            )
            .expect("eviction over capacity");
        match evicted {
            ClosedItem::Surface { surface, .. } => assert_eq!(surface.id, 0),
            _ => panic!("expected Surface"),
        }
        assert_eq!(store.len(), MAX_CLOSED_ITEMS);
    }

    fn ids_of(store: &ClosedItemStore) -> Vec<SurfaceId> {
        store
            .list()
            .map(|i| match i {
                ClosedItem::Surface { surface, .. } => surface.id,
                _ => panic!("expected Surface"),
            })
            .collect()
    }

    #[test]
    fn a_workspace_scoped_pop_skips_other_workspaces_however_recent() {
        let mut store = ClosedItemStore::new();
        for (id, ws) in [(1, 7), (2, 9), (3, 9)] {
            assert!(
                store
                    .push(surface_item(id, ClosedScrollback::Empty), Some(ws))
                    .is_none(),
                "용량 미만이라 evict 는 없다"
            );
        }

        let got = store.pop_matching(|o| o == Some(7)).expect("ws 7 item");
        match got {
            ClosedItem::Surface { surface, .. } => assert_eq!(surface.id, 1),
            _ => panic!("expected Surface"),
        }
        assert_eq!(
            ids_of(&store),
            vec![3, 2],
            "다른 워크스페이스 항목은 그대로"
        );
        assert!(
            store.pop_matching(|o| o == Some(7)).is_none(),
            "ws 7 에는 더 없다 — 다른 워크스페이스로 흘러가지 않는다"
        );
    }

    #[test]
    fn a_whole_workspace_item_never_matches_a_workspace_scope() {
        let mut store = ClosedItemStore::new();
        let evicted = store.push(
            ClosedItem::Workspace {
                id: 5,
                name: "w".into(),
                subtitle: String::new(),
                pane_layout: ClosedPaneNode::Leaf(ClosedPane {
                    id: 1,
                    tabs: Vec::new(),
                    active_tab: 0,
                }),
                focused_pane: 1,
            },
            None,
        );
        assert!(evicted.is_none(), "용량 미만이라 evict 는 없다");
        assert!(
            store.pop_matching(|o| o == Some(5)).is_none(),
            "출처가 None 이라 어떤 워크스페이스 스코프에도 안 걸린다"
        );
        assert_eq!(store.len(), 1, "후보가 아니었으므로 스택에 그대로 남는다");
        assert!(store.pop_matching(|_| true).is_some());
    }
}
