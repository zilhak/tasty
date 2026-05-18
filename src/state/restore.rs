use crate::model::closed_item::*;
use crate::model::{Pane, PaneNode, Surface, SurfaceLayout, Tab, TerminalSurface, Workspace};

use super::AppState;

/// Result of rebuilding a closed panel.
enum RebuildResult {
    /// A single surface (Terminal, Markdown, Explorer, etc.)
    Single(Box<dyn Surface>),
    /// A full layout tree with focused_surface id
    Layout(SurfaceLayout, u32),
}

impl RebuildResult {
    /// Convert into a Tab.
    fn into_tab(self, tab_id: u32, name: String) -> Tab {
        match self {
            RebuildResult::Single(surface) => Tab::new_with_surface(tab_id, name, surface),
            RebuildResult::Layout(layout, focused_surface) => Tab {
                id: tab_id,
                name,
                explicit_name: None,
                layout_opt: Some(layout),
                focused_surface,
                cached_display_name: None,
            },
        }
    }
}

impl AppState {
    /// Restore the most recently closed item. Returns true if something was restored.
    /// Focus moves to the restored item.
    pub fn restore_closed_item(&mut self) -> bool {
        let item = match self.engine.closed_items.pop() {
            Some(item) => item,
            None => return false,
        };

        let result = match item {
            ClosedItem::Surface { surface, tab_name } => self.restore_surface(surface, tab_name),
            ClosedItem::Tab(tab) => self.restore_tab(tab),
            ClosedItem::Workspace {
                name,
                subtitle,
                pane_layout,
                focused_pane,
                ..
            } => self.restore_workspace(name, subtitle, pane_layout, focused_pane),
        };
        if result {
            self.engine.mark_layout_dirty();
        }
        result
    }

    fn restore_surface(&mut self, closed: ClosedSurface, tab_name: String) -> bool {
        let node = match self.rebuild_surface_node(closed) {
            Some(n) => n,
            None => return false,
        };
        let tab_id = self.engine.next_ids.next_tab();
        let surface: Box<dyn Surface> = Box::new(node);
        let tab = Tab::new_with_surface(tab_id, tab_name, surface);

        // Add to focused pane
        self.ensure_workspace_exists();
        if let Some(pane) = self.focused_pane_mut() {
            pane.tabs.push(tab);
            pane.active_tab = pane.tabs.len() - 1;
        }
        true
    }

    fn restore_tab(&mut self, closed_tab: ClosedTab) -> bool {
        let result = match self.rebuild_surface(closed_tab.panel) {
            Some(r) => r,
            None => return false,
        };

        let tab_id = self.engine.next_ids.next_tab();
        let name = closed_tab.explicit_name.unwrap_or(closed_tab.name);
        let tab = result.into_tab(tab_id, name);

        self.ensure_workspace_exists();
        if let Some(pane) = self.focused_pane_mut() {
            pane.tabs.push(tab);
            pane.active_tab = pane.tabs.len() - 1;
        }
        true
    }

    fn restore_workspace(
        &mut self,
        name: String,
        subtitle: String,
        closed_layout: ClosedPaneNode,
        focused_pane: u32,
    ) -> bool {
        let ws_id = self.engine.next_ids.next_workspace();
        let pane_node = match self.rebuild_pane_node(closed_layout) {
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
        self.engine.workspaces.push(ws);
        self.active_workspace = self.engine.workspaces.len() - 1;
        true
    }

    // ── Rebuild helpers ──

    fn rebuild_surface(&mut self, closed: ClosedPanel) -> Option<RebuildResult> {
        match closed {
            ClosedPanel::Terminal(surface) => {
                let node = self.rebuild_surface_node(surface)?;
                Some(RebuildResult::Single(Box::new(node)))
            }
            ClosedPanel::Tab {
                layout,
                focused_surface: _,
            } => {
                let rebuilt_layout = self.rebuild_surface_layout(layout)?;
                let first_id = rebuilt_layout.first_surface_id().unwrap_or(0);
                Some(RebuildResult::Layout(rebuilt_layout, first_id))
            }
            ClosedPanel::Generic { kind, snapshot } => {
                let id = self.engine.next_ids.next_surface();
                let def = self.engine.surface_registry.get(&kind)?;
                match (def.restore)(id, &snapshot) {
                    Ok(surface) => Some(RebuildResult::Single(surface)),
                    Err(e) => {
                        tracing::warn!("restore failed for kind '{}': {e}", kind);
                        None
                    }
                }
            }
        }
    }

    fn rebuild_surface_node(&mut self, closed: ClosedSurface) -> Option<TerminalSurface> {
        let surface_id = self.engine.next_ids.next_surface();
        let cols = self.engine.default_cols;
        let rows = self.engine.default_rows;
        let shell = if self.engine.settings.general.shell.is_empty() {
            None
        } else {
            Some(self.engine.settings.general.shell.clone())
        };
        let shell_args_owned = self.engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = self.engine.make_waker(surface_id);

        let mut terminal = tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols,
                rows,
                shell: shell.as_deref(),
                args: &shell_args,
                surface_id,
                working_dir: None,
            },
            waker,
        )
        .ok()?;

        if !closed.scrollback.is_empty() {
            terminal.inject_scrollback(closed.scrollback.into_iter().collect());
        }

        if let Some(dir) = closed.cwd.as_deref() {
            let cd_cmd = format!("cd {}\r", shell_escape(dir));
            terminal.send_key(&cd_cmd);
        }

        self.engine.send_fast_init(surface_id);

        // Queue restore command for TUI session resumption (plugin-provided, e.g. "claude -r <uuid>").
        if let Some(cmd) = closed.restore_command {
            self.engine.pending_restore_commands.push((surface_id, cmd));
        }

        Some(TerminalSurface {
            id: surface_id,
            terminal,
            deferred_spawn: None,
        })
    }

    fn rebuild_surface_layout(
        &mut self,
        closed: ClosedSurfaceLayout,
    ) -> Option<SurfaceLayout> {
        match closed {
            ClosedSurfaceLayout::Single(surface) => {
                let node = self.rebuild_surface_node(surface)?;
                Some(SurfaceLayout::Leaf(Box::new(node)))
            }
            ClosedSurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let first = self.rebuild_surface_layout(*first)?;
                let second = self.rebuild_surface_layout(*second)?;
                Some(SurfaceLayout::Split {
                    direction,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                    focus_second: false,
                })
            }
        }
    }

    fn rebuild_pane_node(&mut self, closed: ClosedPaneNode) -> Option<PaneNode> {
        match closed {
            ClosedPaneNode::Leaf(closed_pane) => {
                let pane = self.rebuild_pane(closed_pane)?;
                Some(PaneNode::Leaf(pane))
            }
            ClosedPaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let first = self.rebuild_pane_node(*first)?;
                let second = self.rebuild_pane_node(*second)?;
                Some(PaneNode::Split {
                    direction,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
        }
    }

    fn rebuild_pane(&mut self, closed: ClosedPane) -> Option<Pane> {
        let pane_id = self.engine.next_ids.next_pane();
        let mut tabs = Vec::new();
        for closed_tab in closed.tabs {
            let result = self.rebuild_surface(closed_tab.panel)?;
            let tab_id = self.engine.next_ids.next_tab();
            let name = closed_tab.explicit_name.unwrap_or(closed_tab.name);
            tabs.push(result.into_tab(tab_id, name));
        }
        if tabs.is_empty() {
            return None;
        }
        let active_tab = closed.active_tab.min(tabs.len() - 1);
        Some(Pane {
            id: pane_id,
            tabs,
            active_tab,
            tab_scroll_offset: 0.0,
        })
    }
}

/// Escape a path for shell use.
fn shell_escape(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    if s.contains(' ') || s.contains('\'') || s.contains('"') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}
