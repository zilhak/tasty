use std::path::PathBuf;

use super::SurfaceId;
use super::surface_trait::Surface;

/// A surface that displays HTML content via a native OS WebView.
/// The actual WebView instance is managed by MainWindow (not stored here),
/// keyed by the surface_id.
pub struct HtmlPanel {
    pub id: u32,
    pub url: String,
}

impl HtmlPanel {
    pub fn new(id: u32, url: String) -> Self {
        Self { id, url }
    }

    /// `file://` URI 또는 로컬 절대경로면 PathBuf, 그 외(http/https, about:, data: 등)는 None.
    fn url_to_local_path(&self) -> Option<PathBuf> {
        let url = self.url.trim();
        if url.is_empty() {
            return None;
        }
        if let Some(rest) = url.strip_prefix("file://") {
            #[cfg(windows)]
            {
                let s = rest.strip_prefix('/').unwrap_or(rest).replace('/', "\\");
                return Some(PathBuf::from(s));
            }
            #[cfg(not(windows))]
            {
                return Some(PathBuf::from(rest));
            }
        }
        let p = PathBuf::from(url);
        if p.is_absolute() { Some(p) } else { None }
    }
}

impl Surface for HtmlPanel {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "html"
    }
    fn type_name(&self) -> &'static str {
        "Html"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    fn display_name(&self) -> String {
        if self.url.is_empty() {
            "HTML".to_string()
        } else {
            self.url.clone()
        }
    }
    fn webview_url(&self) -> Option<&str> {
        Some(&self.url)
    }
    fn source_cwd(&self) -> Option<PathBuf> {
        self.url_to_local_path()
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(|p| p.to_path_buf())
    }
}
