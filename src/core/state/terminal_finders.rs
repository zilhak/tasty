use tasty_terminal::Terminal;

use super::CoreState;

impl CoreState {
    /// Check if this engine owns a surface with the given id (any kind, not just terminal).
    pub fn has_surface(&self, surface_id: u32) -> bool {
        self.workspaces
            .iter()
            .any(|ws| ws.all_surface_ids().contains(&surface_id))
    }

    /// Check if this engine owns a workspace with the given id.
    pub fn has_workspace(&self, workspace_id: u32) -> bool {
        self.workspaces.iter().any(|ws| ws.id == workspace_id)
    }

    /// Check if this engine owns a pane with the given id.
    pub fn has_pane(&self, pane_id: u32) -> bool {
        self.workspaces
            .iter()
            .any(|ws| ws.pane_layout().all_pane_ids().contains(&pane_id))
    }

    /// Find a terminal by surface ID (immutable). `TerminalStore` 가 source of truth.
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        self.terminals.get(surface_id)
    }

    /// Find a terminal by surface ID (mutable). `TerminalStore` 가 source of truth.
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        self.terminals.get_mut(surface_id)
    }

    /// 실제로 화면에 렌더되는 terminal(불변). hard 점유(readonly) surface 는
    /// `render_pass`/`gpu.rs`와 동일하게 3초 cadence mirror(`readonly_view`)를,
    /// 아니면 live terminal을 반환한다. 좌표 변환(`mouse_to_grid`)·복사 텍스트
    /// 추출(`copy_selection_*`)이 live 를 직접 참조하면, 점유 중에도 계속 갱신되는
    /// live 의 스크롤 위치/scrollback 길이가 화면에 보이는(최대 3초 지연) mirror 와
    /// 어긋나 사용자가 드래그한 영역과 실제 복사되는 텍스트가 달라질 수 있다
    /// (ADR-0040). 휠 스크롤백 mutate(`&mut Terminal` 필요)는 hard 점유 시 자체를
    /// 조기 차단하므로 이 헬퍼로 커버할 필요가 없다.
    pub fn visible_terminal(&self, surface_id: u32) -> Option<&Terminal> {
        if self.attach.is_hard_occupied(surface_id) {
            self.readonly_view(surface_id)
        } else {
            self.terminals.get(surface_id)
        }
    }
}
