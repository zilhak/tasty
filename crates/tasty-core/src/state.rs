use tasty_model::Workspace;
/// engine별 도메인 상태. GUI에서는 창마다 따로 보유하고 공유 자원은 Arc로 주입한다.
/// 외부 함수의 타입에 쓰이지만 내부 필드는 crate 밖에 노출하지 않는다.
pub struct CoreState {
    /// Revision of this committed live projection, never a command-decision source.
    pub(crate) committed_structure_revision: Option<u64>,
    pub(crate) local_workspaces: Vec<Workspace>,
    pub(crate) mirror_workspaces: Vec<Workspace>,
    /// Composite display projection; local relative order comes from the committed model.
    pub(crate) workspace_display_order: Vec<u32>,
    /// Lifetime token for volatile annotations; replaced by every remote structural projection.
    pub(crate) mirror_projection_tokens: std::collections::HashMap<u32, std::sync::Arc<()>>,
    /// 표시 순서의 카테고리. 생성·복원 뒤 기본 normal 항목을 앞에 두도록 정규화한다.
    pub(crate) categories: Vec<tasty_model::WorkspaceCategory>,
}

impl CoreState {
    pub fn new_base() -> Self {
        Self {
            committed_structure_revision: None,
            local_workspaces: Vec::new(),
            mirror_workspaces: Vec::new(),
            workspace_display_order: Vec::new(),
            mirror_projection_tokens: Default::default(),
            categories: vec![tasty_model::WorkspaceCategory::normal()],
        }
    }
}

impl CoreState {
    pub fn local_workspaces(&self) -> &[tasty_model::Workspace] {
        &self.local_workspaces
    }
    pub fn mirror_workspaces(&self) -> &[tasty_model::Workspace] {
        &self.mirror_workspaces
    }
    pub fn committed_structure_revision(&self) -> Option<u64> {
        self.committed_structure_revision
    }

    /// Remote is the only structural writer for mirror collections. A local ID cannot grant
    /// mutable access to the committed local projection through this entry point.
    pub fn mirror_workspace_mut(&mut self, id: u32) -> Option<&mut tasty_model::Workspace> {
        self.mirror_workspaces
            .iter_mut()
            .find(|workspace| workspace.id == id)
    }
    pub fn remove_mirror_workspace(&mut self, id: u32) -> Option<tasty_model::Workspace> {
        let index = self
            .mirror_workspaces
            .iter()
            .position(|workspace| workspace.id == id)?;
        self.workspace_display_order
            .retain(|candidate| *candidate != id);
        self.mirror_projection_tokens.remove(&id);
        Some(self.mirror_workspaces.remove(index))
    }
}
