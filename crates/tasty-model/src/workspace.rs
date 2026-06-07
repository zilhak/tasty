use super::{
    EmptySurface, Pane, PaneId, PaneNode, SurfaceId, TabId, WorkspaceAttachMapping, WorkspaceId,
};

/// Workspace attach(단계 6) 의 surface 분류 결과.
///
/// - `terminals`: kind=="terminal" 또는 deferred `EmptySurface`(아직 PTY 안 뜬 터미널
///   자리). attach 시 각각 mirror 되고 surface_locks 점유 대상.
/// - `non_terminals`: markdown/image/explorer 등. mirror 불가 → client placeholder,
///   서버에서도 숨김(decision 3).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AttachSurfaceClass {
    pub terminals: Vec<SurfaceId>,
    pub non_terminals: Vec<SurfaceId>,
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
        }
    }

    /// attach/detach 단계 7 — 이 워크스페이스의 원격 attach 매핑을 설정/해제한다.
    /// 생성/복원 경로가 생성자 churn 없이 매핑을 얹기 위한 setter.
    pub fn set_attach_mapping(&mut self, mapping: Option<WorkspaceAttachMapping>) {
        self.attach_mapping = mapping;
    }

    /// Collect all surface IDs in this workspace.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.pane_layout().all_surface_ids()
    }

    /// attach 단계 6: 이 workspace 의 surface 를 터미널/비-터미널로 분류한다.
    /// engine 없이 leaf 를 직접 downcast 해 deferred `EmptySurface` 도 터미널로 본다
    /// (surface/list 핸들러가 deferred 를 `type:"Terminal"` 로 보고하는 정책과 동형).
    pub fn classify_attach_surfaces(&self) -> AttachSurfaceClass {
        let mut class = AttachSurfaceClass::default();
        for pane_id in self.pane_layout().all_pane_ids() {
            if let Some(pane) = self.pane_layout().find_pane(pane_id) {
                for tab in &pane.tabs {
                    tab.for_each_surface(&mut |s| {
                        let Some(id) = s.surface_id() else { return };
                        let is_terminal = s.kind() == "terminal"
                            || s.as_any()
                                .downcast_ref::<EmptySurface>()
                                .map(|e| e.is_deferred())
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
            .map(|pane| {
                let tabs: Vec<_> = pane
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, tab)| {
                        let layout = tab
                            .layout_if_initialized()
                            .map(|l| l.to_tree_json_full())
                            .unwrap_or(serde_json::Value::Null);
                        serde_json::json!({
                            "id": tab.id,
                            "name": tab.display_name(),
                            "active": i == pane.active_tab,
                            "focused_surface": tab.focused_surface,
                            "layout": layout,
                        })
                    })
                    .collect();
                serde_json::json!({
                    "id": pane.id,
                    "tabs": tabs,
                })
            })
            .collect();
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "focused_pane": self.focused_pane,
            "panes": panes,
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
