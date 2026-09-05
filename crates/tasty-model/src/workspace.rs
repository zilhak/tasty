use std::path::PathBuf;

use super::{
    EmptySurface, ExplorerPanel, NORMAL_CATEGORY_ID, Pane, PaneId, PaneNode, SurfaceId, TabId,
    WorkspaceAttachMapping, WorkspaceCategoryId, WorkspaceId,
};

/// Workspace attach(단계 6) 의 surface 분류 결과.
///
/// - `terminals`: kind=="terminal" 또는 deferred `EmptySurface`(아직 PTY 안 뜬 터미널
///   자리). attach 시 각각 mirror 되고 surface_locks 점유 대상.
/// - `non_terminals`: markdown/image 등 중 mesh 후보가 **아니고** explorer 도 아닌
///   surface. mirror 불가 → client placeholder, 서버에서도 숨김(decision 3).
/// - `mesh_candidates`: `Surface::attach_mesh_info()` 가 `Some` 을 반환한 surface
///   (surface_id, kind, plugin_id). **최종 화이트리스트 판정은 여기 포함되지
///   않는다** — 앱 계층(`src/core/attach_runtime.rs`)이 bundled 화이트리스트로
///   재검증한 뒤에야 실제 mesh mirror 대상인지 확정한다(`docs/dev-guide/attach-behavior.md`
///   참고). 판정에서
///   떨어진 후보는 호출자가 `non_terminals` 와 동일하게(placeholder) 취급해야
///   한다 — `tasty-model` 은 화이트리스트를 모르므로 스스로 최종 분류하지 않는다.
/// - `explorers`: `(surface_id, root)` — explorer surface 와 **활성 탭의 현재
///   디렉토리**(ADR-0059 Decision 1, 전체 탭이 아니라 활성 탭만). browse-only 원격
///   mirror 대상(ADR-0059).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AttachSurfaceClass {
    pub terminals: Vec<SurfaceId>,
    pub non_terminals: Vec<SurfaceId>,
    pub mesh_candidates: Vec<(SurfaceId, String, String)>,
    pub explorers: Vec<(SurfaceId, PathBuf)>,
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
    /// attach/detach 단계 7 — 이 워크스페이스가 attach 할 원격 컴퓨터(SSH) 매핑.
    /// `Some` 이면 활성화 시 호스트가 자동 attach(SSH 터널 + workspace mirror) 한다.
    /// layout.json 으로 영속(`SavedWorkspace.attach_mapping`).
    pub attach_mapping: Option<WorkspaceAttachMapping>,
    /// attach/detach 작업 J — 이 워크스페이스가 원격을 attach 한 **client mirror** 인지.
    /// `true` 면 사이드바에서 이름 앞 하늘색 `>_→` glyph(collapsed 레일은 아바타 우하단
    /// corner chip)로 로컬 워크스페이스와 구분한다(status dot 은 실행상태 전용, mirror
    /// 색 미포함). 런타임 전용 상태(영속하지 않음 — 재시작 시 재attach).
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

    /// pane 을 닫고, **닫힌 pane 이 포커스였을 때만** 포커스를 옮긴다.
    ///
    /// 사용자가 보고 있지 않은 pane 을 닫았는데 시야가 움직이면 불가침 원칙 1
    /// (사용자 행동 ↔ 에이전트 행동 분리) 위반이다. pane 을 닫는 경로는 여럿인데
    /// (pane 닫기 · 마지막 surface 닫기 · 이동으로 비워진 pane) 규칙은 하나라,
    /// 경로마다 옮겨 적는 대신 여기 하나로 둔다.
    ///
    /// 반환값은 [`PaneNode::close_pane`] 의 반환값 — **실제로 닫혔는지**다.
    ///
    /// `removed` 접합항은 "안 닫았으면 아무것도 안 한다" 는 계약을 코드에 적은 것이지
    /// **지지항이 아니다**: 지워도 이 크레이트의 어떤 단정도 안 죽는다. 두 조건이
    /// 독립이 아니기 때문이다 — 닫기가 실패하는 두 경우(없는 pane · 마지막 leaf)에
    /// `was_focused` 가 참이면서 첫 생존자가 포커스와 다른 상황은 `focused_pane` 이
    /// 이미 트리에 없는 pane 을 가리킬 때뿐이고, 호출자는 전부 조회로 얻은 id 를
    /// 넘긴다. 그 상황을 단정으로 박지 않는 이유는 그때 무엇이 옳은지가 분명하지
    /// 않아서다(매달린 포커스를 유지하는 쪽이 옳다고 말할 수 없다).
    ///
    /// 재는 법: `if removed` 를 `if true` 로 바꾸고 `cargo test -p tasty-model`.
    /// 빨개지면 그때부터 지지항이니 이 문단을 지워라.
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

    /// attach/detach 단계 7 — 이 워크스페이스의 원격 attach 매핑을 설정/해제한다.
    /// 생성/복원 경로가 생성자 churn 없이 매핑을 얹기 위한 setter.
    pub fn set_attach_mapping(&mut self, mapping: Option<WorkspaceAttachMapping>) {
        self.attach_mapping = mapping;
    }

    /// 이 워크스페이스가 속한 카테고리를 변경한다. 생성/복원/이동 경로가 생성자
    /// churn 없이 소속을 얹기 위한 setter(`set_attach_mapping` 과 동형).
    pub fn set_category(&mut self, category: WorkspaceCategoryId) {
        self.category = category;
    }

    /// Collect all surface IDs in this workspace.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.pane_layout().all_surface_ids()
    }

    /// attach 단계 6: 이 workspace 의 surface 를 터미널/비-터미널로 분류한다.
    /// engine 없이 leaf 를 직접 downcast 한다. deferred `EmptySurface` 중
    /// **Terminal deferred(`deferred_spawn`, PTY 를 나중에 spawn 할 자리)만** 터미널로
    /// 보고, **Plugin deferred(`deferred_plugin`, hello 전 placeholder)는 non-terminal
    /// placeholder** 로 보낸다. 이는 `to_tree_json`(surface/list)이 Terminal deferred 는
    /// `pty_ready:false` 로, Plugin deferred 는 `type:"Pending"` 으로 갈라 보고하는 것과
    /// 동형이다. Plugin deferred 를 터미널로 넣으면 attach 가 `tap_surface_for_stream`
    /// 으로 터미널 tap 을 걸려다 `engine.terminals` 에 없어 조용히 실패해, client mirror
    /// 에 "데이터 안 오는 빈 터미널" 로 나타난다(placeholder 로도 표시 안 됨).
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

    /// attach 단계 6 (D4/R2): client mirror 트리 재구성용 전체 트리 디스크립터.
    /// pane → tab → surface 레이아웃(분할 방향/비율 포함)을 JSON 으로 보존한다.
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

    /// pane **셋**이어야 한다. 둘이면 하나를 닫은 뒤 유일한 생존자가 곧 포커스라
    /// `was_focused` 가드를 지워도 값이 안 움직인다 — 그 픽스처로는 아래 첫 시험이
    /// 아무것도 재지 못한다(실측으로 확인했다).
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

    /// 불가침 원칙 1 — 보고 있지 않은 pane 을 닫았다고 시야가 움직이면 안 된다.
    /// 포커스는 3 인데 1 을 닫으면 첫 생존자는 2 다. 가드가 없으면 2 로 끌려간다.
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

    /// 없는 pane 을 닫으라 하면 **닫지 않았다고 답한다.** 이 시험이 재는 것은
    /// 반환값이다 — 뒤의 `focused_pane` 단정은 `removed` 가드가 아니라 `was_focused`
    /// 가드가 통과시킨다(포커스가 3 이고 닫으라는 것은 77 이라 애초에 같지 않다).
    #[test]
    fn a_close_that_removed_nothing_reports_it() {
        let mut ws = three_pane_ws(3);
        assert!(!ws.close_pane_preserving_focus(77));
        assert_eq!(ws.focused_pane, 3);
    }
}
