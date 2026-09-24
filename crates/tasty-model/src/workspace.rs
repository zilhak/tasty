use std::path::PathBuf;

use super::{
    EmptySurface, ExplorerPanel, NORMAL_CATEGORY_ID, Pane, PaneId, PaneNode, SurfaceId, TabId,
    WorkspaceAttachMapping, WorkspaceCategoryId, WorkspaceId,
};

/// workspace attach의 surface 분류.
/// - terminals: 실제 터미널 또는 PTY 생성 대기 placeholder.
/// - non_terminals: 전용 mirror 경로가 없어 placeholder로 보낼 surface.
/// - mesh_candidates/content_candidates: trait이 반환한 후보이며 최종 허용 여부는 호스트가 검사한다.
/// - explorers: 활성 내부 탭의 현재 경로로 만든 읽기 전용 원격 탐색 대상.
///
/// 허용되지 않은 후보는 호스트가 placeholder로 처리해야 한다.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AttachSurfaceClass {
    pub terminals: Vec<SurfaceId>,
    pub non_terminals: Vec<SurfaceId>,
    pub mesh_candidates: Vec<(SurfaceId, String, String)>,
    pub explorers: Vec<(SurfaceId, PathBuf)>,
    pub content_candidates: Vec<(SurfaceId, String, String, Option<PathBuf>)>,
}

/// Workspace - one sidebar item. Contains a PaneLayout (binary split tree of Panes).
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    /// Always `Some` during normal operation. Temporarily `None` during structural mutations.
    pane_layout_opt: Option<PaneNode>,
    pub focused_pane: PaneId,
    /// 저장할 원격 attach 매핑. 활성화할 때 호스트가 자동 연결에 사용한다.
    pub attach_mapping: Option<WorkspaceAttachMapping>,
    /// 원격 workspace를 보여 주는 client mirror인지. 영속하지 않으며 UI 구분 표시에 사용한다.
    pub mirror: bool,
    /// 이 워크스페이스가 속한 카테고리(사이드바 폴더) id. 기본값은 예약된
    /// `normal`([`NORMAL_CATEGORY_ID`]). layout.json 으로 영속(`SavedWorkspace.category`).
    pub category: WorkspaceCategoryId,
}

impl Workspace {
    /// Create a workspace with a TerminalSurface marker. Caller must have already
    /// `engine.terminals.insert(surface_id, terminal)` for the spawned Terminal.
    pub fn new_with_terminal_marker(
        id: WorkspaceId,
        name: String,
        pane_id: PaneId,
        tab_id: TabId,
        surface_id: SurfaceId,
    ) -> Self {
        let pane = Pane::new_with_terminal_marker(pane_id, tab_id, surface_id);
        let focused_pane = pane_id;
        Self {
            id,
            name,
            subtitle: String::new(),
            description: String::new(),
            pane_layout_opt: Some(PaneNode::Leaf(pane)),
            focused_pane,
            attach_mapping: None,
            mirror: false,
            category: NORMAL_CATEGORY_ID,
        }
    }

    /// Access the pane layout (always valid during normal operation).
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn pane_layout(&self) -> &PaneNode {
        self.pane_layout_opt
            .as_ref()
            .expect("BUG: pane_layout accessed during structural mutation (between take/put)")
    }

    /// Access the pane layout mutably.
    /// Panics if called during a structural mutation (between take/put).
    #[track_caller]
    pub fn pane_layout_mut(&mut self) -> &mut PaneNode {
        self.pane_layout_opt
            .as_mut()
            .expect("BUG: pane_layout accessed during structural mutation (between take/put)")
    }

    /// pane을 닫고, 그 pane이 포커스 대상이었을 때만 남은 pane으로 포커스를 옮긴다.
    /// 실제로 닫았는지 반환한다.
    pub fn close_pane_preserving_focus(&mut self, pane_id: PaneId) -> bool {
        let was_focused = self.focused_pane == pane_id;
        let removed = self.pane_layout_mut().close_pane(pane_id);
        if removed
            && was_focused
            && let Some(first) = self.pane_layout().first_pane()
        {
            self.focused_pane = first.id;
        }
        removed
    }

    /// Create a workspace from a pre-built Pane (for non-terminal surface types).
    pub fn new_with_pane(id: WorkspaceId, name: String, pane: Pane) -> Self {
        let focused_pane = pane.id;
        Self {
            id,
            name,
            subtitle: String::new(),
            description: String::new(),
            pane_layout_opt: Some(PaneNode::Leaf(pane)),
            focused_pane,
            attach_mapping: None,
            mirror: false,
            category: NORMAL_CATEGORY_ID,
        }
    }

    /// Create a workspace from a restored pane layout (no PTY creation needed).
    pub fn from_restored(
        id: WorkspaceId,
        name: String,
        subtitle: String,
        pane_layout: PaneNode,
        focused_pane: PaneId,
    ) -> Self {
        Self {
            id,
            name,
            subtitle,
            description: String::new(),
            pane_layout_opt: Some(pane_layout),
            focused_pane,
            attach_mapping: None,
            mirror: false,
            category: NORMAL_CATEGORY_ID,
        }
    }

    /// 원격 attach 매핑을 설정하거나 해제한다.
    pub fn set_attach_mapping(&mut self, mapping: Option<WorkspaceAttachMapping>) {
        self.attach_mapping = mapping;
    }

    /// 카테고리 소속을 변경한다.
    pub fn set_category(&mut self, category: WorkspaceCategoryId) {
        self.category = category;
    }

    /// Collect all surface IDs in this workspace.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.pane_layout().all_surface_ids()
    }

    /// leaf를 조회해 attach 후보를 분류한다. Deferred::Terminal만 터미널로 취급하며
    /// Deferred::Plugin은 PTY 생성 대상이 아닌 플러그인 placeholder다.
    pub fn classify_attach_surfaces(&self) -> AttachSurfaceClass {
        let mut class = AttachSurfaceClass::default();
        for pane_id in self.pane_layout().all_pane_ids() {
            if let Some(pane) = self.pane_layout().find_pane(pane_id) {
                for tab in &pane.tabs {
                    tab.for_each_surface(&mut |s| {
                        let Some(id) = s.surface_id() else { return };
                        if let Some((kind, plugin_id)) = s.attach_mesh_info() {
                            class.mesh_candidates.push((
                                id,
                                kind.to_string(),
                                plugin_id.to_string(),
                            ));
                            return;
                        }
                        // 두 신호를 모두 내면 기존 mesh 경로를 우선한다.
                        if let Some((kind, plugin_id, file)) = s.attach_content_info() {
                            class.content_candidates.push((
                                id,
                                kind.to_string(),
                                plugin_id.to_string(),
                                file,
                            ));
                            return;
                        }
                        if let Some(explorer) = s.as_any().downcast_ref::<ExplorerPanel>() {
                            class
                                .explorers
                                .push((id, explorer.current_root().to_path_buf()));
                            return;
                        }
                        let is_terminal = s.kind() == "terminal"
                            || s.as_any()
                                .downcast_ref::<EmptySurface>()
                                .map(|e| e.deferred_spawn().is_some())
                                .unwrap_or(false);
                        if is_terminal {
                            class.terminals.push(id);
                        } else {
                            class.non_terminals.push(id);
                        }
                    });
                }
            }
        }
        class
    }

    /// client mirror 복원용 전체 pane·tab·surface 트리. 분할 방향과 비율을 보존한다.
    pub fn to_attach_tree_json(&self) -> serde_json::Value {
        let panes: Vec<_> = self
            .pane_layout()
            .all_pane_ids()
            .iter()
            .filter_map(|&pid| self.pane_layout().find_pane(pid))
            .map(|pane| pane.to_attach_json())
            .collect();
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "focused_pane": self.focused_pane,
            "panes": panes,
            "pane_layout": self.pane_layout().to_tree_json_full(),
        })
    }

    /// Produce a JSON tree representation of this workspace.
    pub fn to_tree_json(&self) -> serde_json::Value {
        let panes: Vec<_> = self
            .pane_layout()
            .all_pane_ids()
            .iter()
            .filter_map(|&pid| self.pane_layout().find_pane(pid))
            .map(|pane| {
                let mut p = pane.to_tree_json();
                p["focused"] = serde_json::json!(pane.id == self.focused_pane);
                p
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "panes": panes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SplitDirection;

    fn leaf_pane(id: PaneId, tab_id: TabId, surface_id: SurfaceId) -> Pane {
        Pane::new_with_surface(
            id,
            tab_id,
            "Shell".to_string(),
            Box::new(EmptySurface::new(surface_id)),
        )
    }

    #[test]
    fn to_attach_tree_json_includes_pane_layout_with_direction_and_ratio() {
        let pane_layout = PaneNode::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.3,
            first: Box::new(PaneNode::Leaf(leaf_pane(1, 1, 1))),
            second: Box::new(PaneNode::Leaf(leaf_pane(2, 2, 2))),
        };
        let ws = Workspace::from_restored(9, "test".to_string(), String::new(), pane_layout, 2);
        let json = ws.to_attach_tree_json();
        assert_eq!(json["pane_layout"]["type"], "Split");
        assert_eq!(json["pane_layout"]["direction"], "vertical");
        assert!((json["pane_layout"]["ratio"].as_f64().unwrap() - 0.3).abs() < 1e-6);
        // 기존 평면 "panes" 필드도 여전히 존재(하위호환)해야 한다.
        assert_eq!(json["panes"].as_array().unwrap().len(), 2);
    }

    /// pane을 세 개 만들어 비포커스 pane 제거 뒤에도 원래 포커스가 유지되는지 구분한다.
    fn three_pane_ws(focused: PaneId) -> Workspace {
        let pane_layout = PaneNode::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.5,
            first: Box::new(PaneNode::Leaf(leaf_pane(1, 1, 1))),
            second: Box::new(PaneNode::Split {
                direction: SplitDirection::Horizontal,
                ratio: 0.5,
                first: Box::new(PaneNode::Leaf(leaf_pane(2, 2, 2))),
                second: Box::new(PaneNode::Leaf(leaf_pane(3, 3, 3))),
            }),
        };
        Workspace::from_restored(9, "test".to_string(), String::new(), pane_layout, focused)
    }

    #[test]
    fn closing_an_unfocused_pane_leaves_focus_where_it_was() {
        let mut ws = three_pane_ws(3);
        assert!(ws.close_pane_preserving_focus(1));
        assert_eq!(ws.focused_pane, 3);
    }

    #[test]
    fn closing_the_focused_pane_moves_focus_to_the_first_survivor() {
        let mut ws = three_pane_ws(3);
        assert!(ws.close_pane_preserving_focus(3));
        assert_eq!(ws.focused_pane, 1);
    }

    /// 없는 pane은 닫지 않았다고 반환하며 포커스를 유지한다.
    #[test]
    fn a_close_that_removed_nothing_reports_it() {
        let mut ws = three_pane_ws(3);
        assert!(!ws.close_pane_preserving_focus(77));
        assert_eq!(ws.focused_pane, 3);
    }
}
