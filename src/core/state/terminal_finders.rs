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

    /// Find a terminal by surface ID (immutable).
    ///
    /// **D.3.E.4.c** — dual-source: TerminalStore 우선, 미존재 시 legacy Surface
    /// 트리 순회. E.4.d 에서 Terminal owner 가 store 로 cutover 되면 legacy
    /// fallback 가지가 dead code 가 되고 E.4.f 에서 제거된다.
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        if let Some(t) = self.terminals.get(surface_id) {
            return Some(t);
        }
        for workspace in &self.workspaces {
            let layout = workspace.pane_layout();
            if let Some(t) = Self::find_terminal_in_layout(layout, surface_id) {
                return Some(t);
            }
        }
        None
    }

    /// Find a terminal by surface ID (mutable). Dual-source — TerminalStore 우선.
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        // Borrow checker — store 에 있으면 store 의 &mut 만 반환. legacy fallback
        // 은 별 분기에서 layout tree 순회. 두 분기가 같은 `&mut self` 를 점유
        // 하므로 contains 체크로 branch.
        if self.terminals.contains(surface_id) {
            return self.terminals.get_mut(surface_id);
        }
        for workspace in &mut self.workspaces {
            let layout = workspace.pane_layout_mut();
            if let Some(t) = Self::find_terminal_in_layout_mut(layout, surface_id) {
                return Some(t);
            }
        }
        None
    }

    fn find_terminal_in_layout(
        layout: &crate::model::PaneNode,
        surface_id: u32,
    ) -> Option<&Terminal> {
        match layout {
            crate::model::PaneNode::Leaf(pane) => pane.find_terminal(surface_id),
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::find_terminal_in_layout(first, surface_id)
                    .or_else(|| Self::find_terminal_in_layout(second, surface_id))
            }
        }
    }

    fn find_terminal_in_layout_mut(
        layout: &mut crate::model::PaneNode,
        surface_id: u32,
    ) -> Option<&mut Terminal> {
        match layout {
            crate::model::PaneNode::Leaf(pane) => pane.find_terminal_mut(surface_id),
            crate::model::PaneNode::Split { first, second, .. } => {
                if let Some(t) = Self::find_terminal_in_layout_mut(first, surface_id) {
                    return Some(t);
                }
                Self::find_terminal_in_layout_mut(second, surface_id)
            }
        }
    }
}
