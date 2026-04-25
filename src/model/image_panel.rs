use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::ColorImage;

use super::SurfaceId;
use super::surface_trait::Surface;

/// Image file extensions recognized by the viewer.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tiff", "svg",
];

/// Default blank canvas dimensions (width × height) when an image surface starts empty.
pub const DEFAULT_BLANK_CANVAS_WIDTH: usize = 800;
pub const DEFAULT_BLANK_CANVAS_HEIGHT: usize = 600;

/// A surface that displays an image with viewer and drawing capabilities.
pub struct ImagePanel {
    pub id: u32,
    // ── Viewer state ──
    pub file_path: Option<String>,
    pub dir_images: Vec<String>,
    pub current_index: usize,
    pub original_image: Option<ColorImage>,
    pub texture: Option<egui::TextureHandle>,
    pub zoom: f32,
    pub pan_offset: egui::Vec2,
    pub last_mtime: Option<SystemTime>,

    // ── Drawing state ──
    pub edit_mode: bool,
    pub draw_layer: Option<ColorImage>,
    pub draw_texture: Option<egui::TextureHandle>,
    pub brush_size: f32,
    pub brush_color: egui::Color32,
    pub last_draw_pos: Option<egui::Pos2>,
    pub is_dirty: bool,
    pub draw_texture_dirty: bool,

    // ── New image popup ──
    pub new_image_popup: bool,
    pub new_image_width: String,
    pub new_image_height: String,

    // ── Save path popup ──
    pub save_path_popup: bool,
    pub save_path_buffer: String,
}

impl ImagePanel {
    /// Create an image panel from a file path.
    pub fn new(id: u32, file_path: String) -> Self {
        let dir_images = scan_directory_images(&file_path);
        let current_index = dir_images.iter().position(|p| p == &file_path).unwrap_or(0);
        let (original_image, last_mtime) = load_image_from_path(&file_path);

        Self {
            id,
            file_path: Some(file_path),
            dir_images,
            current_index,
            original_image,
            texture: None,
            zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            last_mtime,
            edit_mode: false,
            draw_layer: None,
            draw_texture: None,
            brush_size: 2.0,
            brush_color: egui::Color32::RED,
            last_draw_pos: None,
            is_dirty: false,
            draw_texture_dirty: false,
            new_image_popup: false,
            new_image_width: DEFAULT_BLANK_CANVAS_WIDTH.to_string(),
            new_image_height: DEFAULT_BLANK_CANVAS_HEIGHT.to_string(),
            save_path_popup: false,
            save_path_buffer: String::new(),
        }
    }

    /// Create an image panel for a new blank canvas.
    ///
    /// Starts with an 800×600 white canvas already filled in and edit mode active,
    /// so the user (or AI agent) can begin drawing immediately. The "new image"
    /// popup is reserved for the explicit `+` button, which lets users pick a
    /// different size on demand.
    pub fn new_blank(id: u32) -> Self {
        let mut panel = Self {
            id,
            file_path: None,
            dir_images: Vec::new(),
            current_index: 0,
            original_image: None,
            texture: None,
            zoom: 1.0,
            pan_offset: egui::Vec2::ZERO,
            last_mtime: None,
            edit_mode: false,
            draw_layer: None,
            draw_texture: None,
            brush_size: 2.0,
            brush_color: egui::Color32::RED,
            last_draw_pos: None,
            is_dirty: false,
            draw_texture_dirty: false,
            new_image_popup: false,
            new_image_width: DEFAULT_BLANK_CANVAS_WIDTH.to_string(),
            new_image_height: DEFAULT_BLANK_CANVAS_HEIGHT.to_string(),
            save_path_popup: false,
            save_path_buffer: String::new(),
        };
        panel.create_blank_canvas(DEFAULT_BLANK_CANVAS_WIDTH, DEFAULT_BLANK_CANVAS_HEIGHT);
        panel
    }

    /// Navigate to the previous image in the directory.
    pub fn prev_image(&mut self) {
        if self.dir_images.is_empty() || self.edit_mode {
            return;
        }
        if self.current_index > 0 {
            self.current_index -= 1;
        } else {
            self.current_index = self.dir_images.len() - 1;
        }
        self.load_current();
    }

    /// Navigate to the next image in the directory.
    pub fn next_image(&mut self) {
        if self.dir_images.is_empty() || self.edit_mode {
            return;
        }
        self.current_index = (self.current_index + 1) % self.dir_images.len();
        self.load_current();
    }

    /// Reload the current image from disk.
    pub fn reload(&mut self) {
        if let Some(ref path) = self.file_path {
            let (img, mtime) = load_image_from_path(path);
            self.original_image = img;
            self.last_mtime = mtime;
            self.texture = None;
        }
    }

    /// Load the image at current_index.
    fn load_current(&mut self) {
        if let Some(path) = self.dir_images.get(self.current_index).cloned() {
            let (img, mtime) = load_image_from_path(&path);
            self.original_image = img;
            self.last_mtime = mtime;
            self.file_path = Some(path);
            self.texture = None;
            self.zoom = 1.0;
            self.pan_offset = egui::Vec2::ZERO;
            self.exit_edit_mode();
        }
    }

    /// Enter edit mode, creating the draw layer overlay.
    pub fn enter_edit_mode(&mut self) {
        if let Some(ref img) = self.original_image {
            let [w, h] = img.size;
            let transparent = egui::Color32::TRANSPARENT;
            let draw_layer = ColorImage::new([w, h], transparent);
            self.draw_layer = Some(draw_layer);
            self.draw_texture = None;
            self.edit_mode = true;
            self.is_dirty = false;
            self.last_draw_pos = None;
            self.draw_texture_dirty = true;
        }
    }

    /// Exit edit mode, discarding the draw layer.
    pub fn exit_edit_mode(&mut self) {
        self.edit_mode = false;
        self.draw_layer = None;
        self.draw_texture = None;
        self.is_dirty = false;
        self.last_draw_pos = None;
        self.draw_texture_dirty = false;
    }

    /// Create a new blank canvas with the given dimensions.
    pub fn create_blank_canvas(&mut self, width: usize, height: usize) {
        let white = egui::Color32::WHITE;
        self.original_image = Some(ColorImage::new([width, height], white));
        self.texture = None;
        self.zoom = 1.0;
        self.pan_offset = egui::Vec2::ZERO;
        self.dir_images.clear();
        self.current_index = 0;
        self.enter_edit_mode();
        self.new_image_popup = false;
    }

    /// Draw a line segment on the draw layer using Bresenham's algorithm.
    pub fn draw_line(&mut self, from: egui::Pos2, to: egui::Pos2) {
        let layer = match self.draw_layer.as_mut() {
            Some(l) => l,
            None => return,
        };
        let [w, h] = layer.size;
        let radius = (self.brush_size / 2.0).max(0.5);
        let color = self.brush_color;

        bresenham_thick_line(layer, from, to, radius, color, w, h);
        self.is_dirty = true;
        self.draw_texture_dirty = true;
    }

    /// Save the composited image (original + overlay) as PNG.
    pub fn save_png(&self, path: &str) -> Result<(), String> {
        let original = self.original_image.as_ref().ok_or("No image to save")?;
        let [w, h] = original.size;

        // Composite original + draw layer
        let mut composited = original.clone();
        if let Some(ref layer) = self.draw_layer {
            for i in 0..(w * h) {
                let bg = composited.pixels[i];
                let fg = layer.pixels[i];
                composited.pixels[i] = alpha_blend(bg, fg);
            }
        }

        // Encode as PNG using the image crate
        let mut rgba_data = Vec::with_capacity(w * h * 4);
        for pixel in &composited.pixels {
            rgba_data.push(pixel.r());
            rgba_data.push(pixel.g());
            rgba_data.push(pixel.b());
            rgba_data.push(pixel.a());
        }

        let img_buf: image::RgbaImage = image::RgbaImage::from_raw(w as u32, h as u32, rgba_data)
            .ok_or("Failed to create image buffer")?;

        img_buf
            .save(path)
            .map_err(|e| format!("Failed to save PNG: {}", e))
    }

    /// Get the save path for the current image (with .png extension).
    pub fn save_path(&self) -> Option<String> {
        self.file_path.as_ref().map(|p| {
            let path = Path::new(p);
            path.with_extension("png").to_string_lossy().to_string()
        })
    }
}

impl Surface for ImagePanel {
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
}

// ── Helper functions ──

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
            .filter(|p| {
                p.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
                    .unwrap_or(false)
            })
            .collect(),
        Err(_) => return vec![file_path.to_string()],
    };

    images.sort();
    images
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// Load an image from a file path, returning the ColorImage and modification time.
fn load_image_from_path(path: &str) -> (Option<ColorImage>, Option<SystemTime>) {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

    let img = match image::open(path) {
        Ok(img) => img,
        Err(_) => return (None, mtime),
    };

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let pixels: Vec<egui::Color32> = rgba
        .pixels()
        .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();

    (
        Some(ColorImage {
            size: [w as usize, h as usize],
            pixels,
        }),
        mtime,
    )
}

/// Alpha blend foreground over background.
fn alpha_blend(bg: egui::Color32, fg: egui::Color32) -> egui::Color32 {
    let fa = fg.a() as f32 / 255.0;
    if fa < 0.001 {
        return bg;
    }
    let ba = bg.a() as f32 / 255.0;
    let out_a = fa + ba * (1.0 - fa);
    if out_a < 0.001 {
        return egui::Color32::TRANSPARENT;
    }
    let r = (fg.r() as f32 * fa + bg.r() as f32 * ba * (1.0 - fa)) / out_a;
    let g = (fg.g() as f32 * fa + bg.g() as f32 * ba * (1.0 - fa)) / out_a;
    let b = (fg.b() as f32 * fa + bg.b() as f32 * ba * (1.0 - fa)) / out_a;
    egui::Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, (out_a * 255.0) as u8)
}

/// Draw a thick line using Bresenham's algorithm with circle brush.
fn bresenham_thick_line(
    layer: &mut ColorImage,
    from: egui::Pos2,
    to: egui::Pos2,
    radius: f32,
    color: egui::Color32,
    w: usize,
    h: usize,
) {
    let dx = (to.x - from.x).abs();
    let dy = (to.y - from.y).abs();
    let steps = dx.max(dy).ceil() as i32;
    let steps = steps.max(1);

    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = from.x + (to.x - from.x) * t;
        let y = from.y + (to.y - from.y) * t;
        fill_circle(layer, x, y, radius, color, w, h);
    }
}

/// Fill a circle of pixels at (cx, cy) with the given radius.
fn fill_circle(
    layer: &mut ColorImage,
    cx: f32,
    cy: f32,
    radius: f32,
    color: egui::Color32,
    w: usize,
    h: usize,
) {
    let r = radius.ceil() as i32;
    let cx_i = cx as i32;
    let cy_i = cy as i32;
    let r_sq = radius * radius;

    for dy in -r..=r {
        for dx in -r..=r {
            if (dx * dx + dy * dy) as f32 <= r_sq {
                let px = cx_i + dx;
                let py = cy_i + dy;
                if px >= 0 && px < w as i32 && py >= 0 && py < h as i32 {
                    layer.pixels[py as usize * w + px as usize] = color;
                }
            }
        }
    }
}
