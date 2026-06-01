use crate::core::restore_rebuild;
use crate::engine_state::CoreState;
use crate::model::closed_item::*;
use crate::model::{Surface, Tab, Workspace};

use super::AppState;

impl AppState {
    /// Restore the most recently closed item. Returns true if something was restored.
    /// Focus moves to the restored item.
    pub fn restore_closed_item(&mut self, engine: &mut CoreState) -> bool {
        let item = match engine.closed_items.pop() {
            Some(item) => item,
            None => return false,
        };

        let result = match item {
            ClosedItem::Surface { surface, tab_name } => {
                self.restore_surface(engine, surface, tab_name)
            }
            ClosedItem::Tab(tab) => self.restore_tab(engine, tab),
            ClosedItem::Workspace {
                name,
                subtitle,
                pane_layout,
                focused_pane,
                ..
            } => self.restore_workspace(engine, name, subtitle, pane_layout, focused_pane),
        };
        if result {
            engine.mark_layout_dirty();
        }
        result
    }

    fn restore_surface(
        &mut self,
        engine: &mut CoreState,
        closed: ClosedSurface,
        tab_name: String,
    ) -> bool {
        let node = match restore_rebuild::rebuild_surface_node(engine, closed) {
            Some(n) => n,
            None => return false,
        };
        let tab_id = engine.next_ids.next_tab();
        let surface: Box<dyn Surface> = Box::new(node);
        let tab = Tab::new_with_surface(tab_id, tab_name, surface);

        // Add to focused pane
        self.ensure_workspace_exists(engine);
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.tabs.push(tab);
            pane.active_tab = pane.tabs.len() - 1;
        }
        true
    }

    fn restore_tab(&mut self, engine: &mut CoreState, closed_tab: ClosedTab) -> bool {
        let result = match restore_rebuild::rebuild_surface(engine, closed_tab.panel) {
            Some(r) => r,
            None => return false,
        };

        let tab_id = engine.next_ids.next_tab();
        let name = closed_tab.explicit_name.unwrap_or(closed_tab.name);
        let tab = result.into_tab(tab_id, name);

        self.ensure_workspace_exists(engine);
        if let Some(pane) = self.focused_pane_mut(engine) {
            pane.tabs.push(tab);
            pane.active_tab = pane.tabs.len() - 1;
        }
        true
    }

    fn restore_workspace(
        &mut self,
        engine: &mut CoreState,
        name: String,
        subtitle: String,
        closed_layout: ClosedPaneNode,
        focused_pane: u32,
    ) -> bool {
        let ws_id = engine.next_ids.next_workspace();
        let pane_node = match restore_rebuild::rebuild_pane_node(engine, closed_layout) {
            Some(n) => n,
            None => return false,
        };

        // Find the actual focused pane ID (the saved one may not match rebuilt IDs)
        let all_pane_ids = pane_node.all_pane_ids();
        let actual_focused = if all_pane_ids.contains(&focused_pane) {
            focused_pane
        } else {
            *all_pane_ids.first().unwrap_or(&0)
        };

        let ws = Workspace::from_restored(ws_id, name, subtitle, pane_node, actual_focused);
        engine.workspaces.push(ws);
        self.active_workspace = engine.workspaces.len() - 1;
        true
    }
}
