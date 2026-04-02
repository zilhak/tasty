use tasty_terminal::Terminal;
use super::{ExplorerPanel, MarkdownPanel, Rect, SplitDirection, SurfaceGroupLayout, SurfaceGroupNode, SurfaceId, SurfaceNode};

/// Content type within a Tab.
pub enum Panel {
    /// A single terminal instance.
    Terminal(SurfaceNode),
    /// A split within a tab - appears as ONE tab but renders multiple terminals.
    SurfaceGroup(SurfaceGroupNode),
    /// A Markdown file viewer (rendered with egui, no PTY).
    Markdown(MarkdownPanel),
    /// A file explorer (rendered with egui, no PTY).
    Explorer(ExplorerPanel),
}

impl Panel {
    /// Get the focused terminal.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        match self {
            Panel::Terminal(node) => Some(&node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal(),
            Panel::Markdown(_) | Panel::Explorer(_) => None,
        }
    }

    /// Get the focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        match self {
            Panel::Terminal(node) => Some(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal_mut(),
            Panel::Markdown(_) | Panel::Explorer(_) => None,
        }
    }

    /// Collect all terminals (mutable) in this panel.
    pub fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        match self {
            Panel::Terminal(node) => out.push(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.layout_mut().collect_terminals_mut(out),
            Panel::Markdown(_) | Panel::Explorer(_) => {}
        }
    }

    /// Visit all terminals (mutable) in this panel.
    pub fn for_each_terminal_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(SurfaceId, &mut Terminal),
    {
        match self {
            Panel::Terminal(node) => f(node.id, &mut node.terminal),
            Panel::SurfaceGroup(group) => group.layout_mut().for_each_terminal_mut(f),
            Panel::Markdown(_) | Panel::Explorer(_) => {}
        }
    }

    /// Find a terminal by surface ID (immutable).
    pub fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        match self {
            Panel::Terminal(node) => {
                if node.id == surface_id { Some(&node.terminal) } else { None }
            }
            Panel::SurfaceGroup(group) => group.layout().find_terminal(surface_id),
            Panel::Markdown(_) | Panel::Explorer(_) => None,
        }
    }

    /// Find a terminal by surface ID (mutable).
    pub fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        match self {
            Panel::Terminal(node) => {
                if node.id == surface_id { Some(&mut node.terminal) } else { None }
            }
            Panel::SurfaceGroup(group) => group.layout_mut().find_terminal_mut(surface_id),
            Panel::Markdown(_) | Panel::Explorer(_) => None,
        }
    }

    /// Get render regions for this panel within the given rect.
    /// Markdown and Explorer panels return empty since they are rendered by egui.
    pub fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        match self {
            Panel::Terminal(node) => vec![(node.id, &node.terminal, rect)],
            Panel::SurfaceGroup(group) => group.compute_rects(rect),
            Panel::Markdown(_) | Panel::Explorer(_) => vec![],
        }
    }

    /// Collect all surface IDs in this panel.
    pub fn all_surface_ids(&self) -> Vec<SurfaceId> {
        match self {
            Panel::Terminal(node) => vec![node.id],
            Panel::SurfaceGroup(group) => group.layout().all_surface_ids(),
            Panel::Markdown(_) | Panel::Explorer(_) => vec![],
        }
    }

    /// Returns true if this panel is a non-terminal panel (Markdown or Explorer).
    pub fn is_non_terminal(&self) -> bool {
        matches!(self, Panel::Markdown(_) | Panel::Explorer(_))
    }

    /// Resize all terminals in this panel.
    pub fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        match self {
            Panel::Terminal(node) => {
                let cols = ((rect.width - 4.0) / cell_width).floor().max(1.0) as usize;
                let rows = ((rect.height - 4.0) / cell_height).floor().max(1.0) as usize;
                node.terminal.resize(cols, rows);
            }
            Panel::SurfaceGroup(group) => group.resize_all(rect, cell_width, cell_height),
            Panel::Markdown(_) | Panel::Explorer(_) => {}
        }
    }

    /// Split the focused surface. Takes a pre-created terminal (infallible).
    /// Called from Pane::split_active_surface after pre-creation succeeds.
    pub fn split_surface_with_terminal(
        self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        new_terminal: Terminal,
    ) -> Self {
        match self {
            Panel::Terminal(old_node) => {
                let old_surface_id = old_node.id;
                let group = SurfaceGroupNode {
                    layout_opt: Some(SurfaceGroupLayout::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(SurfaceGroupLayout::Single(old_node)),
                        second: Box::new(SurfaceGroupLayout::Single(SurfaceNode {
                            id: new_surface_id,
                            terminal: new_terminal,
                            deferred_spawn: None,
                        })),
                        focus_second: true,
                    }),
                    focused_surface: new_surface_id,
                    _first_surface: old_surface_id,
                };
                Panel::SurfaceGroup(group)
            }
            Panel::SurfaceGroup(mut group) => {
                // Pre-built terminal: wrap into SurfaceNode and use infallible split.
                let new_node = SurfaceNode { id: new_surface_id, terminal: new_terminal, deferred_spawn: None };
                let target = group.focused_surface;
                let old_layout = group.take_layout();
                let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
                group.put_layout(new_layout);
                group.focused_surface = new_surface_id;
                Panel::SurfaceGroup(group)
            }
            // Non-terminal panels cannot be split (they have no surfaces).
            Panel::Markdown(_) | Panel::Explorer(_) => self,
        }
    }

    /// Split a specific surface by ID. Does NOT change focused_surface.
    /// Used by IPC `split` command where focus must not move.
    pub fn split_surface_by_id_with_terminal(
        self,
        target_surface_id: SurfaceId,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        new_terminal: Terminal,
    ) -> Self {
        match self {
            Panel::Terminal(old_node) if old_node.id == target_surface_id => {
                let old_surface_id = old_node.id;
                let group = SurfaceGroupNode {
                    layout_opt: Some(SurfaceGroupLayout::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(SurfaceGroupLayout::Single(old_node)),
                        second: Box::new(SurfaceGroupLayout::Single(SurfaceNode {
                            id: new_surface_id,
                            terminal: new_terminal,
                            deferred_spawn: None,
                        })),
                        focus_second: false,
                    }),
                    focused_surface: old_surface_id,
                    _first_surface: old_surface_id,
                };
                Panel::SurfaceGroup(group)
            }
            Panel::SurfaceGroup(mut group) => {
                let new_node = SurfaceNode { id: new_surface_id, terminal: new_terminal, deferred_spawn: None };
                let old_layout = group.take_layout();
                let (new_layout, _) = old_layout.split_with_node(target_surface_id, direction, new_node);
                group.put_layout(new_layout);
                // Do NOT change group.focused_surface
                Panel::SurfaceGroup(group)
            }
            other => other,
        }
    }
}
