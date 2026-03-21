use crate::terminal::Terminal;

pub type WorkspaceId = u32;
pub type PaneId = u32;
pub type SurfaceId = u32;

/// A pixel rectangle used for viewport/scissor calculations.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn split(self, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
        match direction {
            SplitDirection::Vertical => {
                let first_w = (self.width * ratio).floor();
                let second_w = self.width - first_w;
                (
                    Rect {
                        x: self.x,
                        y: self.y,
                        width: first_w,
                        height: self.height,
                    },
                    Rect {
                        x: self.x + first_w,
                        y: self.y,
                        width: second_w,
                        height: self.height,
                    },
                )
            }
            SplitDirection::Horizontal => {
                let first_h = (self.height * ratio).floor();
                let second_h = self.height - first_h;
                (
                    Rect {
                        x: self.x,
                        y: self.y,
                        width: self.width,
                        height: first_h,
                    },
                    Rect {
                        x: self.x,
                        y: self.y + first_h,
                        width: self.width,
                        height: second_h,
                    },
                )
            }
        }
    }
}

pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub panes: Vec<Pane>,
    pub active_pane: usize,
}

impl Workspace {
    /// Create a workspace with one default pane containing a single terminal.
    pub fn new(
        id: WorkspaceId,
        name: String,
        cols: usize,
        rows: usize,
        surface_id: SurfaceId,
        pane_id: PaneId,
    ) -> anyhow::Result<Self> {
        let pane = Pane::new(pane_id, "Shell".to_string(), cols, rows, surface_id)?;
        Ok(Self {
            id,
            name,
            panes: vec![pane],
            active_pane: 0,
        })
    }

    pub fn add_pane(&mut self, pane: Pane) {
        self.panes.push(pane);
        self.active_pane = self.panes.len() - 1;
    }

    pub fn active_pane(&self) -> &Pane {
        &self.panes[self.active_pane]
    }

    pub fn active_pane_mut(&mut self) -> &mut Pane {
        &mut self.panes[self.active_pane]
    }
}

pub struct Pane {
    pub id: PaneId,
    pub name: String,
    pub root: SurfaceGroup,
}

impl Pane {
    pub fn new(
        id: PaneId,
        name: String,
        cols: usize,
        rows: usize,
        surface_id: SurfaceId,
    ) -> anyhow::Result<Self> {
        let terminal = Terminal::new(cols, rows)?;
        let root = SurfaceGroup::Single(SurfaceNode {
            id: surface_id,
            terminal,
        });
        Ok(Self { id, name, root })
    }
}

pub enum SurfaceGroup {
    Single(SurfaceNode),
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<SurfaceGroup>,
        second: Box<SurfaceGroup>,
        /// Which branch has focus: false = first, true = second
        focus_second: bool,
    },
}

impl SurfaceGroup {
    /// Split the currently focused surface into two, spawning a new terminal.
    pub fn split(
        &mut self,
        direction: SplitDirection,
        new_surface_id: SurfaceId,
        cols: usize,
        rows: usize,
    ) -> anyhow::Result<()> {
        match self {
            SurfaceGroup::Single(_) => {
                // Replace self with a split, moving old content to `first`
                let old = std::mem::replace(
                    self,
                    // temporary placeholder, will be overwritten
                    SurfaceGroup::Single(SurfaceNode {
                        id: 0,
                        terminal: Terminal::new(1, 1)?,
                    }),
                );
                let new_terminal = Terminal::new(cols, rows)?;
                *self = SurfaceGroup::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(old),
                    second: Box::new(SurfaceGroup::Single(SurfaceNode {
                        id: new_surface_id,
                        terminal: new_terminal,
                    })),
                    focus_second: true,
                };
                Ok(())
            }
            SurfaceGroup::Split {
                first,
                second,
                focus_second,
                ..
            } => {
                if *focus_second {
                    second.split(direction, new_surface_id, cols, rows)
                } else {
                    first.split(direction, new_surface_id, cols, rows)
                }
            }
        }
    }

    /// Get a reference to the focused terminal.
    pub fn focused_terminal(&self) -> &Terminal {
        match self {
            SurfaceGroup::Single(node) => &node.terminal,
            SurfaceGroup::Split {
                first,
                second,
                focus_second,
                ..
            } => {
                if *focus_second {
                    second.focused_terminal()
                } else {
                    first.focused_terminal()
                }
            }
        }
    }

    /// Get a mutable reference to the focused terminal.
    pub fn focused_terminal_mut(&mut self) -> &mut Terminal {
        match self {
            SurfaceGroup::Single(node) => &mut node.terminal,
            SurfaceGroup::Split {
                first,
                second,
                focus_second,
                ..
            } => {
                if *focus_second {
                    second.focused_terminal_mut()
                } else {
                    first.focused_terminal_mut()
                }
            }
        }
    }

    /// Process all terminals in this group. Returns true if any changed.
    pub fn process_all(&mut self) -> bool {
        match self {
            SurfaceGroup::Single(node) => node.terminal.process(),
            SurfaceGroup::Split { first, second, .. } => {
                let a = first.process_all();
                let b = second.process_all();
                a || b
            }
        }
    }

    /// Compute pixel rectangles for each surface given a total rect.
    pub fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> {
        match self {
            SurfaceGroup::Single(node) => vec![(node.id, &node.terminal, rect)],
            SurfaceGroup::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let (r1, r2) = rect.split(*direction, *ratio);
                let mut result = first.render_regions(r1);
                result.extend(second.render_regions(r2));
                result
            }
        }
    }

    /// Move focus to the next surface in the given direction.
    /// Returns true if focus changed.
    pub fn move_focus_forward(&mut self) -> bool {
        match self {
            SurfaceGroup::Single(_) => false,
            SurfaceGroup::Split {
                first,
                second,
                focus_second,
                ..
            } => {
                // Try to move focus within the focused branch first
                let child = if *focus_second {
                    second.as_mut()
                } else {
                    first.as_mut()
                };
                if child.move_focus_forward() {
                    return true;
                }
                // If focused branch is first, switch to second
                if !*focus_second {
                    *focus_second = true;
                    return true;
                }
                false
            }
        }
    }

    /// Move focus to the previous surface.
    pub fn move_focus_backward(&mut self) -> bool {
        match self {
            SurfaceGroup::Single(_) => false,
            SurfaceGroup::Split {
                first,
                second,
                focus_second,
                ..
            } => {
                let child = if *focus_second {
                    second.as_mut()
                } else {
                    first.as_mut()
                };
                if child.move_focus_backward() {
                    return true;
                }
                if *focus_second {
                    *focus_second = false;
                    return true;
                }
                false
            }
        }
    }

    /// Get the surface ID of the focused terminal.
    pub fn focused_surface_id(&self) -> SurfaceId {
        match self {
            SurfaceGroup::Single(node) => node.id,
            SurfaceGroup::Split {
                first,
                second,
                focus_second,
                ..
            } => {
                if *focus_second {
                    second.focused_surface_id()
                } else {
                    first.focused_surface_id()
                }
            }
        }
    }

    /// Resize all terminals in this group according to the given rect and cell dimensions.
    pub fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        match self {
            SurfaceGroup::Single(node) => {
                let cols = ((rect.width - 4.0) / cell_width).floor().max(1.0) as usize;
                let rows = ((rect.height - 4.0) / cell_height).floor().max(1.0) as usize;
                node.terminal.resize(cols, rows);
            }
            SurfaceGroup::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => {
                let (r1, r2) = rect.split(*direction, *ratio);
                first.resize_all(r1, cell_width, cell_height);
                second.resize_all(r2, cell_width, cell_height);
            }
        }
    }
}

pub struct SurfaceNode {
    pub id: SurfaceId,
    pub terminal: Terminal,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}
