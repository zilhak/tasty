/// A panel that displays HTML content via a native OS WebView.
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
