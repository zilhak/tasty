use tasty_terminal::Terminal;
use super::{ExplorerPanel, HtmlPanel, MarkdownPanel, Rect, SplitDirection, SurfaceGroupLayout, SurfaceGroupNode, SurfaceId, SurfaceNode};
use super::panel_trait::PanelBehavior;

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
    /// An HTML viewer (rendered by native OS WebView, no PTY).
    Html(HtmlPanel),
    /// An empty surface placeholder (no content, shows convert button).
    Empty { id: SurfaceId },
}

impl PanelBehavior for Panel {
    fn type_name(&self) -> &'static str {
        match self {
            Panel::Terminal(_) => "Terminal",
            Panel::SurfaceGroup(_) => "SurfaceGroup",
            Panel::Markdown(_) => "Markdown",
            Panel::Explorer(_) => "Explorer",
            Panel::Html(_) => "Html",
            Panel::Empty { .. } => "Empty",
        }
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        match self {
            Panel::Terminal(node) => Some(node.id),
            Panel::SurfaceGroup(_) => None,
            Panel::Markdown(md) => Some(md.id),
            Panel::Explorer(ex) => Some(ex.id),
            Panel::Html(html) => Some(html.id),
            Panel::Empty { id } => Some(*id),
        }
    }

    fn all_surface_ids(&self) -> Vec<SurfaceId> {
        match self {
            Panel::SurfaceGroup(group) => group.layout().all_surface_ids(),
            other => other.surface_id().into_iter().collect(),
        }
    }

    fn focused_surface_id(&self) -> Option<SurfaceId> {
        match self {
            Panel::SurfaceGroup(group) => Some(group.focused_surface),
            other => other.surface_id(),
        }
    }

    fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        self.all_surface_ids().contains(&surface_id)
    }

    fn has_terminal(&self) -> bool {
        matches!(self, Panel::Terminal(_) | Panel::SurfaceGroup(_))
    }

    fn focused_terminal(&self) -> Option<&Terminal> {
        match self {
            Panel::Terminal(node) => Some(&node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal(),
            _ => None,
        }
    }

    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        match self {
            Panel::Terminal(node) => Some(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.focused_terminal_mut(),
            _ => None,
        }
    }

    fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        match self {
            Panel::Terminal(node) if node.id == surface_id => Some(&node.terminal),
            Panel::SurfaceGroup(group) => group.layout().find_terminal(surface_id),
            _ => None,
        }
    }

    fn find_terminal_node(&self, surface_id: SurfaceId) -> Option<&SurfaceNode> {
        match self {
            Panel::Terminal(node) if node.id == surface_id => Some(node),
            Panel::SurfaceGroup(group) => group.layout().find_surface_node(surface_id),
            _ => None,
        }
    }

    fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        match self {
            Panel::Terminal(node) if node.id == surface_id => Some(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.layout_mut().find_terminal_mut(surface_id),
            _ => None,
        }
    }

    fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        match self {
            Panel::Terminal(node) => vec![(node.id, &node.terminal, rect)],
            Panel::SurfaceGroup(group) => group.compute_rects(rect),
            _ => vec![],
        }
    }

    fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        match self {
            Panel::Terminal(node) => {
                let cols = (rect.width / cell_width).floor().max(1.0) as usize;
                let rows = (rect.height / cell_height).floor().max(1.0) as usize;
                node.terminal.resize(cols, rows);
            }
            Panel::SurfaceGroup(group) => group.resize_all(rect, cell_width, cell_height),
            _ => {}
        }
    }

    fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        match self {
            Panel::Terminal(node) => out.push(&mut node.terminal),
            Panel::SurfaceGroup(group) => group.layout_mut().collect_terminals_mut(out),
            _ => {}
        }
    }

    fn for_each_terminal_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(SurfaceId, &mut Terminal),
    {
        match self {
            Panel::Terminal(node) => f(node.id, &mut node.terminal),
            Panel::SurfaceGroup(group) => group.layout_mut().for_each_terminal_mut(f),
            _ => {}
        }
    }
}

/// Downcasting helpers for Panel variants.
impl Panel {
    /// Get the inner SurfaceGroupNode if this is a SurfaceGroup.
    pub fn as_surface_group(&self) -> Option<&SurfaceGroupNode> {
        match self {
            Panel::SurfaceGroup(group) => Some(group),
            _ => None,
        }
    }

    /// Get the inner SurfaceGroupNode mutably if this is a SurfaceGroup.
    pub fn as_surface_group_mut(&mut self) -> Option<&mut SurfaceGroupNode> {
        match self {
            Panel::SurfaceGroup(group) => Some(group),
            _ => None,
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
                let new_node = SurfaceNode { id: new_surface_id, terminal: new_terminal, deferred_spawn: None };
                let target = group.focused_surface;
                let old_layout = group.take_layout();
                let (new_layout, _) = old_layout.split_with_node(target, direction, new_node);
                group.put_layout(new_layout);
                group.focused_surface = new_surface_id;
                Panel::SurfaceGroup(group)
            }
            other => other, // Only terminal panels support surface-level split
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
                Panel::SurfaceGroup(group)
            }
            other => other,
        }
    }
}
