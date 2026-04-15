use super::surface_trait::Surface;
use super::SurfaceId;

/// A surface that displays HTML content via a native OS WebView.
/// The actual WebView instance is managed by TastyWindow (not stored here),
/// keyed by the surface_id.
pub struct HtmlPanel {
    pub id: u32,
    pub url: String,
}

impl HtmlPanel {
    pub fn new(id: u32, url: String) -> Self {
        Self { id, url }
    }
}

impl Surface for HtmlPanel {
    fn type_name(&self) -> &'static str { "Html" }
    fn surface_id(&self) -> Option<SurfaceId> { Some(self.id) }
    fn display_name(&self) -> String {
        if self.url.is_empty() { "HTML".to_string() } else { self.url.clone() }
    }
}
