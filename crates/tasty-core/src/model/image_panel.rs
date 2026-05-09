use std::path::{Path, PathBuf};

use super::SurfaceId;
use super::surface_trait::Surface;

/// Image file extensions recognized by the viewer.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tiff", "svg",
];

/// Default blank canvas dimensions when an image surface is created without a file.
/// The host's `ImageView` reads these to initialise pixels on first render.
pub const DEFAULT_BLANK_CANVAS_WIDTH: usize = 800;
pub const DEFAULT_BLANK_CANVAS_HEIGHT: usize = 600;

/// A surface backed by an image file (or, when `file_path` is `None`, a blank canvas).
/// Holds only identification + directory navigation state — pixel data, textures, edit
/// history, brush settings, and popup buffers all live in the host's `ImageView`.
pub struct ImagePanel {
    pub id: u32,
    /// `None` = blank canvas not yet saved to disk.
    pub file_path: Option<String>,
    /// Sibling images in the same directory (sorted), used for prev/next navigation.
    pub dir_images: Vec<String>,
    /// Index into `dir_images` for the currently displayed file.
    pub current_index: usize,
}

impl ImagePanel {
    pub fn new(id: u32, file_path: String) -> Self {
        let dir_images = scan_directory_images(&file_path);
        let current_index = dir_images.iter().position(|p| p == &file_path).unwrap_or(0);
        Self {
            id,
            file_path: Some(file_path),
            dir_images,
            current_index,
        }
    }

    /// Create an image surface with no backing file. The host view treats this as
    /// "open new blank canvas in edit mode" on first render.
    pub fn new_blank(id: u32) -> Self {
        Self {
            id,
            file_path: None,
            dir_images: Vec::new(),
            current_index: 0,
        }
    }

    /// True when the panel was created via `new_blank` and the user has not saved a file
    /// path yet (`Save As...` flow has not completed).
    pub fn is_blank(&self) -> bool {
        self.file_path.is_none()
    }

    /// Step one image backward in the directory. Returns the new file path on success;
    /// the host view should reload pixel data from it. No-op if `dir_images` is empty.
    pub fn step_prev(&mut self) -> Option<String> {
        if self.dir_images.is_empty() {
            return None;
        }
        if self.current_index > 0 {
            self.current_index -= 1;
        } else {
            self.current_index = self.dir_images.len() - 1;
        }
        let path = self.dir_images.get(self.current_index)?.clone();
        self.file_path = Some(path.clone());
        Some(path)
    }

    /// Step one image forward in the directory. See `step_prev`.
    pub fn step_next(&mut self) -> Option<String> {
        if self.dir_images.is_empty() {
            return None;
        }
        self.current_index = (self.current_index + 1) % self.dir_images.len();
        let path = self.dir_images.get(self.current_index)?.clone();
        self.file_path = Some(path.clone());
        Some(path)
    }

    /// Default save destination for the current image (always `.png`).
    pub fn save_path(&self) -> Option<String> {
        self.file_path.as_ref().map(|p| {
            Path::new(p)
                .with_extension("png")
                .to_string_lossy()
                .to_string()
        })
    }

    /// Adopt a save path after the host view writes a previously-blank canvas to disk.
    pub fn assign_file_path(&mut self, path: String) {
        self.file_path = Some(path);
    }
}

impl Surface for ImagePanel {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "image"
    }
    fn type_name(&self) -> &'static str {
        "Image"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    fn display_name(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| Path::new(p).file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Image".to_string())
    }
    fn as_image(&self) -> Option<&ImagePanel> {
        Some(self)
    }
    fn as_image_mut(&mut self) -> Option<&mut ImagePanel> {
        Some(self)
    }
    fn source_cwd(&self) -> Option<PathBuf> {
        self.file_path
            .as_ref()
            .and_then(|p| Path::new(p).parent())
            .map(|p| p.to_path_buf())
    }
}

/// Returns true if the path's extension is a recognised image type.
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Scan the directory of the given file path for image files.
fn scan_directory_images(file_path: &str) -> Vec<String> {
    let path = Path::new(file_path);
    let parent = match path.parent() {
        Some(p) => p,
        None => return vec![file_path.to_string()],
    };

    let mut images: Vec<PathBuf> = match std::fs::read_dir(parent) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| is_image_file(p))
            .collect(),
        Err(_) => return vec![file_path.to_string()],
    };

    images.sort();
    images
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}
