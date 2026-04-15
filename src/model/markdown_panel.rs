use super::surface_trait::Surface;
use super::SurfaceId;

/// A surface that displays a Markdown file rendered with egui.
pub struct MarkdownPanel {
    pub id: u32,
    pub file_path: String,
    pub content: String,
    pub scroll_offset: f32,
}

impl MarkdownPanel {
    pub fn new(id: u32, file_path: String) -> Self {
        let content =
            std::fs::read_to_string(&file_path).unwrap_or_else(|e| format!("Error: {}", e));
        Self {
            id,
            file_path,
            content,
            scroll_offset: 0.0,
        }
    }

    pub fn reload(&mut self) {
        self.content = std::fs::read_to_string(&self.file_path)
            .unwrap_or_else(|e| format!("Error: {}", e));
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
}
