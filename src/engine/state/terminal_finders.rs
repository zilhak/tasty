use tasty_terminal::Terminal;

use super::EngineState;

impl EngineState {
    /// Find a terminal by surface ID (immutable).
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        for workspace in &self.workspaces {
            let layout = workspace.pane_layout();
            if let Some(t) = Self::find_terminal_in_layout(layout, surface_id) {
                return Some(t);
            }
        }
        None
    }

    /// Find a terminal by surface ID (mutable).
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
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
