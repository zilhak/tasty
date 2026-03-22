use tasty_terminal::Terminal;
use super::{Rect, SplitDirection, SurfaceGroupLayout, SurfaceGroupNode, SurfaceId, SurfaceNode};

/// Content type within a Tab.
pub enum Panel {
    /// A single terminal instance.
    Terminal(SurfaceNode),
    /// A split within a tab - appears as ONE tab but renders multiple terminals.
    SurfaceGroup(SurfaceGroupNode),
}

impl Panel {
    /// Get the focused terminal.
    pub fn focused_terminal(&self) -> Option<&Terminal> {
        match self {
            Panel::Terminal(node) => Some(&node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal(),
        }
    }

    /// Get the focused terminal (mutable).
    pub fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        match self {
            Panel::Terminal(node) => Some(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal_mut(),
        }
    }

    /// Collect all terminals (mutable) in this panel.
    pub fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        match self {
            Panel::Terminal(node) => out.push(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.layout_mut().collect_terminals_mut(out),
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
        }
    }

    /// Get render regions for this panel within the given rect.
    pub fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        match self {
            Panel::Terminal(node) => vec![(node.id, &node.terminal, rect)],
            Panel::SurfaceGroup(group) => group.compute_rects(rect),
        }
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
                let new_node = SurfaceNode { id: new_surface_id, terminal: new_terminal };
                let target = group.focused_surface;
                let old_layout = group.take_layout();
                let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
                group.put_layout(new_layout);
                group.focused_surface = new_surface_id;
                Panel::SurfaceGroup(group)
            }
        }
    }
}
