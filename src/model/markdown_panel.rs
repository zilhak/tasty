use std::time::{Instant, SystemTime};

use super::surface_trait::Surface;
use super::SurfaceId;

/// A surface that displays a Markdown file rendered with egui.
/// Supports automatic reload when the file changes on disk.
pub struct MarkdownPanel {
    pub id: u32,
    pub file_path: String,
    pub content: String,
    pub scroll_offset: f32,
    /// Last known modification time of the file.
    last_mtime: Option<SystemTime>,
    /// When we last checked the file's mtime (throttle to avoid excessive stat calls).
    last_check: Instant,
}

/// How often to check the file's mtime (in seconds).
const RELOAD_CHECK_INTERVAL_SECS: f64 = 1.0;

impl MarkdownPanel {
    pub fn new(id: u32, file_path: String) -> Self {
        let content =
            std::fs::read_to_string(&file_path).unwrap_or_else(|e| format!("Error: {}", e));
        let mtime = std::fs::metadata(&file_path)
            .and_then(|m| m.modified())
            .ok();
        Self {
            id,
            file_path,
            content,
            scroll_offset: 0.0,
            last_mtime: mtime,
            last_check: Instant::now(),
        }
    }

    pub fn reload(&mut self) {
        self.content = std::fs::read_to_string(&self.file_path)
            .unwrap_or_else(|e| format!("Error: {}", e));
        self.last_mtime = std::fs::metadata(&self.file_path)
            .and_then(|m| m.modified())
            .ok();
    }

    /// Check if the file has been modified since last read, and reload if so.
    /// Throttled to avoid excessive filesystem access.
    pub fn check_reload(&mut self) {
        if self.last_check.elapsed().as_secs_f64() < RELOAD_CHECK_INTERVAL_SECS {
            return;
        }
        self.last_check = Instant::now();

        let current_mtime = match std::fs::metadata(&self.file_path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => return,
        };

        let changed = match self.last_mtime {
            Some(prev) => current_mtime != prev,
            None => true,
        };

        if changed {
            self.reload();
        }
    }
}

impl Surface for MarkdownPanel {
    fn type_name(&self) -> &'static str { "Markdown" }
    fn surface_id(&self) -> Option<SurfaceId> { Some(self.id) }
    fn display_name(&self) -> String {
        std::path::Path::new(&self.file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Markdown".to_string())
    }
    fn as_markdown(&self) -> Option<&MarkdownPanel> { Some(self) }
    fn as_markdown_mut(&mut self) -> Option<&mut MarkdownPanel> { Some(self) }
}
