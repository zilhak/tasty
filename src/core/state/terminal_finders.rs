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

    /// **D.3.E.4.e** — 모든 workspace 의 layout 트리 안에서 *terminal 이 Some
    /// 인 모든 TerminalSurface* 의 terminal 을 take 해 store 로 이전. idempotent
    /// (모두 None 이면 no-op). main loop 의 frame begin 에 매번 호출되어 새로
    /// 생성된 Terminal 들을 store 로 모은다.
    pub(crate) fn install_orphan_terminals(&mut self) {
        for ws in &mut self.workspaces {
            Self::install_in_pane_node(ws.pane_layout_mut(), &mut self.terminals);
        }
    }

    fn install_in_pane_node(
        layout: &mut crate::model::PaneNode,
        store: &mut crate::core::terminal_store::TerminalStore,
    ) {
        match layout {
            crate::model::PaneNode::Leaf(pane) => {
                for tab in &mut pane.tabs {
                    if let Some(surface_layout) = tab.layout_opt.as_mut() {
                        surface_layout.drain_terminals_into_store(store);
                    }
                }
            }
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::install_in_pane_node(first, store);
                Self::install_in_pane_node(second, store);
            }
        }
    }

    /// 단일 surface_id 에 대해, layout 트리에서 TerminalSurface 의 terminal 을
    /// take 해 store 로 이전. split / convert 같은 *단일 surface 추가/변경* 경로에
    /// 호출. 트리 전체 순회.
    #[allow(dead_code)]
    pub(crate) fn install_single_terminal(&mut self, surface_id: u32) {
        for ws in &mut self.workspaces {
            let layout = ws.pane_layout_mut();
            if Self::install_single_in_layout(layout, surface_id, &mut self.terminals) {
                return;
            }
        }
    }

    fn install_single_in_layout(
        layout: &mut crate::model::PaneNode,
        surface_id: u32,
        store: &mut crate::core::terminal_store::TerminalStore,
    ) -> bool {
        match layout {
            crate::model::PaneNode::Leaf(pane) => {
                for tab in &mut pane.tabs {
                    if let Some(surface_layout) = tab.layout_opt.as_mut() {
                        if Self::install_single_in_surface_layout(surface_layout, surface_id, store)
                        {
                            return true;
                        }
                    }
                }
                false
            }
            crate::model::PaneNode::Split { first, second, .. } => {
                Self::install_single_in_layout(first, surface_id, store)
                    || Self::install_single_in_layout(second, surface_id, store)
            }
        }
    }

    fn install_single_in_surface_layout(
        layout: &mut crate::model::SurfaceLayout,
        surface_id: u32,
        store: &mut crate::core::terminal_store::TerminalStore,
    ) -> bool {
        match layout {
            crate::model::SurfaceLayout::Leaf(surface) => {
                if let Some(ts) = surface.as_terminal_surface_mut() {
                    if ts.id == surface_id {
                        if let Some(t) = ts.terminal.take() {
                            store.insert(ts.id, t);
                        }
                        if let Some(persist_id) = ts.scrollback_persist_id.take() {
                            store.set_scrollback_persist_id(ts.id, persist_id);
                        }
                        return true;
                    }
                }
                false
            }
            crate::model::SurfaceLayout::Split { first, second, .. } => {
                Self::install_single_in_surface_layout(first, surface_id, store)
                    || Self::install_single_in_surface_layout(second, surface_id, store)
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
