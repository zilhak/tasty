use std::time::{Instant, SystemTime};

use super::SurfaceId;
use super::surface_trait::Surface;

/// A surface that displays a Markdown file. Holds only identification + reload-tracking
/// state; render content (`String`), scroll offset, and `egui_commonmark` cache live in
/// the host's `MarkdownView` so this model is GUI-free.
pub struct MarkdownPanel {
    pub id: u32,
    pub file_path: String,
    /// Last known modification time of the file.
    last_mtime: Option<SystemTime>,
    /// When we last checked the file's mtime (throttle to avoid excessive stat calls).
    last_check: Instant,
}

/// How often to check the file's mtime (in seconds).
const RELOAD_CHECK_INTERVAL_SECS: f64 = 1.0;

impl MarkdownPanel {
    pub fn new(id: u32, file_path: String) -> Self {
        let mtime = std::fs::metadata(&file_path)
            .and_then(|m| m.modified())
            .ok();
        Self {
            id,
            file_path,
            last_mtime: mtime,
            last_check: Instant::now(),
        }
    }

    /// Throttled mtime poll. Returns the file's new content when it has changed since
    /// the last successful read; the host view is responsible for caching it.
    pub fn poll_reload(&mut self) -> Option<String> {
        if self.last_check.elapsed().as_secs_f64() < RELOAD_CHECK_INTERVAL_SECS {
            return None;
        }
        self.last_check = Instant::now();

        let current = std::fs::metadata(&self.file_path).and_then(|m| m.modified()).ok()?;
        let changed = self.last_mtime.map_or(true, |prev| current != prev);
        if !changed {
            return None;
        }
        self.last_mtime = Some(current);
        std::fs::read_to_string(&self.file_path).ok()
    }

    /// Force a reload regardless of throttle/mtime. Returns new content on success.
    pub fn force_reload(&mut self) -> Option<String> {
        self.last_mtime = std::fs::metadata(&self.file_path)
            .and_then(|m| m.modified())
            .ok();
        self.last_check = Instant::now();
        std::fs::read_to_string(&self.file_path).ok()
    }
}

impl Surface for MarkdownPanel {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "markdown"
    }
    fn type_name(&self) -> &'static str {
        "Markdown"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    fn display_name(&self) -> String {
        std::path::Path::new(&self.file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Markdown".to_string())
    }
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        std::path::Path::new(&self.file_path)
            .parent()
            .map(|p| p.to_path_buf())
    }
}
