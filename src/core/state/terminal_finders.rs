use tasty_terminal::Terminal;

use super::CoreState;

impl CoreState {
    pub fn has_surface(&self, surface_id: u32) -> bool {
        self.workspaces
            .iter()
            .any(|ws| ws.all_surface_ids().contains(&surface_id))
    }

    pub fn has_workspace(&self, workspace_id: u32) -> bool {
        self.workspaces.iter().any(|ws| ws.id == workspace_id)
    }

    pub fn has_pane(&self, pane_id: u32) -> bool {
        self.workspaces
            .iter()
            .any(|ws| ws.pane_layout().all_pane_ids().contains(&pane_id))
    }

    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        self.terminals.get(surface_id)
    }

    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        self.terminals.get_mut(surface_id)
    }

    /// 화면과 같은 Terminal을 사용해야 선택 좌표와 복사 내용이 맞는다.
    /// hard 점유 중에는 readonly 사본만 반환하며 없다고 원본으로 대체하지 않는다.
    #[cfg(feature = "gui")]
    pub fn visible_terminal(&self, surface_id: u32) -> Option<&Terminal> {
        if self.attach.is_hard_occupied(surface_id) {
            self.readonly_view(surface_id)
        } else {
            self.terminals.get(surface_id)
        }
    }
}
