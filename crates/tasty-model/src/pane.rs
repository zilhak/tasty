use super::tab::Tab;
use super::{PaneId, SplitDirection, SurfaceId, TabId};

/// 탭 전환 결과. 변경 없음과 대상 부재를 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSwitch {
    /// 다른 탭으로 옮겼다.
    Switched,
    /// 이미 그 탭을 보고 있었다 — 정상이고, 바뀐 것이 없을 뿐이다.
    AlreadyActive,
    /// 대상 인덱스가 범위 밖이다. 실제 탭 수를 함께 반환한다.
    OutOfRange { tabs: usize },
    /// 포커스된 pane 이 없어 물음이 성립하지 않는다. View의 탭 전환 은 이 갈래를
    /// 내지 않는다 — pane 을 찾아 주는 바깥 층이 더한다.
    NoPane,
}

/// A screen region with its own independent tab bar.
#[derive(Default)]
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
}

impl Pane {
    /// Create a Pane with a Surface trait object.
    pub fn new_with_surface(
        id: PaneId,
        tab_id: TabId,
        name: String,
        surface: super::SurfaceDescriptor,
    ) -> Self {
        let tab = super::tab::Tab::new_with_surface(tab_id, name, surface);
        Self {
            id,
            tabs: vec![tab],
        }
    }

    /// Create a Pane with a TerminalSurface marker. Caller must have already
    /// `engine.runtime.terminals.insert(surface_id, terminal)` for the spawned Terminal.
    pub fn new_with_terminal_marker(id: PaneId, tab_id: TabId, surface_id: SurfaceId) -> Self {
        let surface: super::SurfaceDescriptor =
            super::SurfaceDescriptor::new(surface_id, "terminal");
        let tab = Tab::new_with_surface(tab_id, "Shell".to_string(), surface);
        Self {
            id,
            tabs: vec![tab],
        }
    }

    /// Add a TerminalSurface-marker tab (active). Caller must have already
    /// inserted the spawned Terminal into the store.
    pub fn add_terminal_marker_tab(&mut self, tab_id: TabId, surface_id: SurfaceId) {
        let surface: super::SurfaceDescriptor =
            super::SurfaceDescriptor::new(surface_id, "terminal");
        let tab = Tab::new_with_surface(tab_id, "Shell".to_string(), surface);
        self.tabs.push(tab);
    }

    /// Same as [`add_terminal_marker_tab`] but does NOT change `active_tab`.
    pub fn add_terminal_marker_tab_background(
        &mut self,
        tab_id: TabId,
        surface_id: SurfaceId,
        explicit_name: Option<String>,
    ) {
        let surface: super::SurfaceDescriptor =
            super::SurfaceDescriptor::new(surface_id, "terminal");
        let tab = Tab::new_named(tab_id, "Shell".to_string(), explicit_name, surface);
        self.tabs.push(tab);
    }

    /// Collect all surface IDs across all tabs in this pane.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        let mut ids = Vec::new();
        for tab in &self.tabs {
            ids.extend(tab.all_surface_ids());
        }
        ids
    }

    /// Split a specific surface by ID with a TerminalSurface marker. Caller must
    /// have already inserted the spawned Terminal into the store.
    pub fn split_surface_by_id_marker(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
    ) -> anyhow::Result<()> {
        for tab in &mut self.tabs {
            if tab.contains_surface(target_surface_id) {
                tab.split_surface_by_id(target_surface_id, direction, new_surface_id);
                return Ok(());
            }
        }
        anyhow::bail!("surface {} not found in this pane", target_surface_id)
    }

    /// Split a specific surface by ID with any surface type (not just terminal).
    pub fn split_surface_by_id_with_surface(
        &mut self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface: super::SurfaceDescriptor,
    ) -> anyhow::Result<()> {
        for tab in &mut self.tabs {
            if tab.contains_surface(target_surface_id) {
                tab.split_surface_by_id_generic(target_surface_id, direction, new_surface);
                return Ok(());
            }
        }
        anyhow::bail!("surface {} not found in this pane", target_surface_id)
    }

    /// Remove a tab by its current position. Selection belongs to the caller.
    pub fn remove_tab(&mut self, tab_index: usize) {
        self.take_tab(tab_index);
    }

    /// Remove and return a tab for a structural move.
    pub fn take_tab(&mut self, tab_index: usize) -> Option<Tab> {
        if tab_index >= self.tabs.len() {
            return None;
        }
        Some(self.tabs.remove(tab_index))
    }

    /// Close the tab at the given index. Returns false if the tab can't be closed
    /// (e.g., it's the last tab).
    pub fn close_tab(&mut self, tab_index: usize) -> bool {
        if self.tabs.len() <= 1 {
            return false; // Can't close last tab
        }
        if tab_index < self.tabs.len() {
            self.remove_tab(tab_index);
            true
        } else {
            false
        }
    }

    /// Close a tab by its ID. Returns false if not found or it's the last tab.
    pub fn close_tab_by_id(&mut self, tab_id: TabId) -> bool {
        if self.tabs.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) {
            self.remove_tab(idx);
            true
        } else {
            false
        }
    }

    /// Check if any tab in this pane contains the given surface ID.
    pub fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        self.tabs.iter().any(|tab| tab.contains_surface(surface_id))
    }

    /// Add a tab with a Surface trait object and switch to it.
    pub fn add_surface_tab(
        &mut self,
        tab_id: TabId,
        name: String,
        explicit_name: Option<String>,
        surface: super::SurfaceDescriptor,
    ) {
        let tab = super::tab::Tab::new_named(tab_id, name, explicit_name, surface);
        self.tabs.push(tab);
    }

    /// Same as [`add_surface_tab`](Self::add_surface_tab) but does NOT change `active_tab`.
    pub fn add_surface_tab_background(
        &mut self,
        tab_id: TabId,
        name: String,
        explicit_name: Option<String>,
        surface: super::SurfaceDescriptor,
    ) {
        let tab = super::tab::Tab::new_named(tab_id, name, explicit_name, surface);
        self.tabs.push(tab);
    }

    /// Move a tab from one index to another, adjusting active_tab accordingly.
    /// Returns false if indices are out of bounds or equal.
    pub fn move_tab(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return false;
        }
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        true
    }

    /// Produce a JSON tree representation of this pane.
    pub fn to_tree_json(
        &self,
        presentation: &(impl crate::StructurePresentation + ?Sized),
        surface_json: &dyn Fn(SurfaceId) -> serde_json::Value,
    ) -> serde_json::Value {
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let mut t = tab.to_tree_json(presentation, surface_json);
                t["active"] = serde_json::json!(i == presentation.tab_index(self));
                t
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "tabs": tabs,
        })
    }

    /// attach 디스크립터용 pane JSON: `{"id", "tabs":[{"id","name","active",
    /// "focused_surface","layout"}, ...]}`. `Workspace::to_attach_tree_json`(평면
    /// "panes")과 `PaneNode::to_tree_json_full`(트리 Leaf)이 이 메서드를 공유해
    /// 두 표현이 서로 다른 pane 직렬화를 갖지 않도록 한다.
    pub fn to_attach_json(
        &self,
        presentation: &(impl crate::StructurePresentation + ?Sized),
    ) -> serde_json::Value {
        let tabs: Vec<_> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let layout = tab
                    .layout_if_initialized()
                    .map(|l| l.to_tree_json_full(presentation))
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "id": tab.id,
                    "name": tab.display_name(presentation.surface_id(tab).and_then(|id|presentation.surface_title(id))),
                    "active": i == presentation.tab_index(self),
                    "focused_surface": presentation.surface_id(tab).unwrap_or(0),
                    "layout": layout,
                })
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "tabs": tabs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_removal_and_reorder_preserve_remaining_tab_ids() {
        let mut pane = Pane::new_with_terminal_marker(1, 10, 100);
        pane.add_terminal_marker_tab(11, 101);
        pane.add_terminal_marker_tab(12, 102);
        assert!(pane.move_tab(0, 2));
        assert_eq!(
            pane.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            [11, 12, 10]
        );
        assert_eq!(pane.take_tab(1).unwrap().id, 12);
        pane.remove_tab(9);
        assert_eq!(pane.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), [11, 10]);
        assert!(pane.close_tab_by_id(10));
        assert!(!pane.close_tab_by_id(11));
    }
}
