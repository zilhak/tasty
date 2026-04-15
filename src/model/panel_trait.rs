use tasty_terminal::Terminal;
use super::{Rect, SurfaceId, TerminalSurface};

/// Common behavior for all Panel types.
///
/// Panel enum implements this trait, centralizing the match dispatch.
/// External code should prefer calling these trait methods over
/// matching on Panel variants directly.
pub trait PanelBehavior {
    /// Get the type name of this panel (e.g. "Terminal", "Markdown").
    fn type_name(&self) -> &'static str;

    /// Get the single surface ID for non-group panels.
    /// Returns None for SurfaceGroup (use `all_surface_ids()` instead).
    fn surface_id(&self) -> Option<SurfaceId>;

    /// Collect all surface IDs in this panel.
    fn all_surface_ids(&self) -> Vec<SurfaceId>;

    /// Get the focused surface ID.
    fn focused_surface_id(&self) -> Option<SurfaceId>;

    /// Check if this panel contains the given surface ID.
    fn contains_surface(&self, surface_id: SurfaceId) -> bool;

    /// Returns true if this panel contains terminal surfaces (PTY-backed).
    fn has_terminal(&self) -> bool;

    /// Get the focused terminal (immutable).
    fn focused_terminal(&self) -> Option<&Terminal>;

    /// Get the focused terminal (mutable).
    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal>;

    /// Find a terminal by surface ID (immutable).
    fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal>;

    /// Find a TerminalSurface by surface ID.
    fn find_terminal_node(&self, surface_id: SurfaceId) -> Option<&TerminalSurface>;

    /// Find a terminal by surface ID (mutable).
    fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal>;

    /// Get render regions for this panel within the given rect.
    /// Terminal panels return regions; non-terminal panels return empty Vec.
    fn render_regions(&self, rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)>;

    /// Resize all terminals in this panel to fit the given rect.
    fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32);

    /// Collect all terminals (mutable) in this panel.
    fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>);

    /// Visit all terminals (mutable) with their surface IDs.
    fn for_each_terminal_mut<F>(&mut self, f: &mut F)
    where
        F: FnMut(SurfaceId, &mut Terminal);
}
