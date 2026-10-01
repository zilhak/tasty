//! Legacy scrollback references used by slot migration and garbage collection.
use super::schema::{SavedLayout, SavedPaneNode, SavedSurface, SavedSurfaceLayout};

impl SavedLayout {
    /// 이 슬롯의 scrollback 참조만 모은다. GC는 다른 창의 파일을 지우지 않도록 모든 슬롯과 합쳐야 한다.
    pub fn collect_scrollback_refs(&self) -> std::collections::HashSet<String> {
        let mut refs = std::collections::HashSet::new();
        for ws in &self.workspaces {
            Self::collect_scrollback_refs_in_pane(&ws.pane_layout, &mut refs);
        }
        refs
    }

    fn collect_scrollback_refs_in_pane(
        node: &SavedPaneNode,
        out: &mut std::collections::HashSet<String>,
    ) {
        match node {
            SavedPaneNode::Leaf(pane) => {
                for tab in &pane.tabs {
                    Self::collect_scrollback_refs_in_layout(&tab.surface, out);
                }
            }
            SavedPaneNode::Split { first, second, .. } => {
                Self::collect_scrollback_refs_in_pane(first, out);
                Self::collect_scrollback_refs_in_pane(second, out);
            }
        }
    }

    fn collect_scrollback_refs_in_layout(
        layout: &SavedSurfaceLayout,
        out: &mut std::collections::HashSet<String>,
    ) {
        match layout {
            SavedSurfaceLayout::Leaf(SavedSurface::Terminal {
                scrollback_ref: Some(id),
                ..
            }) => {
                out.insert(id.clone());
            }
            SavedSurfaceLayout::Leaf(_) => {}
            SavedSurfaceLayout::Split { first, second, .. } => {
                Self::collect_scrollback_refs_in_layout(first, out);
                Self::collect_scrollback_refs_in_layout(second, out);
            }
        }
    }
}
