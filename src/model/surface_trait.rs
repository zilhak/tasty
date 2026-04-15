use tasty_terminal::Terminal;
use super::{Rect, SurfaceGroupNode, SurfaceId, TerminalSurface};

/// Common behavior for all Surface types.
///
/// Each surface type (TerminalSurface, MarkdownSurface, ExplorerSurface,
/// HtmlSurface, EmptySurface, SurfaceGroup) implements this trait.
/// All methods have default implementations suitable for non-terminal surfaces.
pub trait Surface {
    /// Surface type name (e.g. "Terminal", "Markdown").
    fn type_name(&self) -> &'static str;

    /// Get this surface's ID. Returns None only for SurfaceGroup (multiple IDs).
    fn surface_id(&self) -> Option<SurfaceId>;

    /// All surface IDs contained in this surface.
    fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.surface_id().into_iter().collect()
    }

    /// The focused surface ID.
    fn focused_surface_id(&self) -> Option<SurfaceId> {
        self.surface_id()
    }

    /// Whether this surface contains the given surface ID.
    fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        self.all_surface_ids().contains(&surface_id)
    }

    /// Whether this surface has terminal (PTY-backed) content.
    fn has_terminal(&self) -> bool { false }

    /// Get the focused terminal (immutable).
    fn focused_terminal(&self) -> Option<&Terminal> { None }

    /// Get the focused terminal (mutable).
    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> { None }

    /// Find a terminal by surface ID (immutable).
    fn find_terminal(&self, _surface_id: SurfaceId) -> Option<&Terminal> { None }

    /// Find a TerminalSurface by surface ID.
    fn find_terminal_surface(&self, _surface_id: SurfaceId) -> Option<&TerminalSurface> { None }

    /// Find a terminal by surface ID (mutable).
    fn find_terminal_mut(&mut self, _surface_id: SurfaceId) -> Option<&mut Terminal> { None }

    /// Get render regions for GPU rendering. Non-terminal surfaces return empty.
    fn render_regions(&self, _rect: Rect) -> Vec<(SurfaceId, &Terminal, Rect)> { vec![] }

    /// Resize all terminals to fit the given rect.
    fn resize_all(&mut self, _rect: Rect, _cell_width: f32, _cell_height: f32) {}

    /// Collect all terminals (mutable). Object-safe signature.
    fn collect_terminals_mut<'a>(&'a mut self, _out: &mut Vec<&'a mut Terminal>) {}

    /// Visit all terminals with their surface IDs. Object-safe signature.
    fn for_each_terminal_mut(&mut self, _f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {}

    /// Downcast to SurfaceGroupNode (immutable). Default: None.
    fn as_surface_group(&self) -> Option<&SurfaceGroupNode> { None }

    /// Downcast to SurfaceGroupNode (mutable). Default: None.
    fn as_surface_group_mut(&mut self) -> Option<&mut SurfaceGroupNode> { None }

    /// Display name for tab title. Default: type_name.
    fn display_name(&self) -> String {
        self.type_name().to_string()
    }
}
