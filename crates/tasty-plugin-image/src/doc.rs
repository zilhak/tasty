//! Per-surface image document state, owned by the plugin process (ADR-0028 / B2).
//!
//! Mirrors the former host `ImageView` + `ImagePanel` navigation fields, now living in the
//! plugin: the loaded pixel buffer, edit-mode state machine (drawing / floating selection),
//! undo/redo history, brush settings, zoom/pan, directory navigation, and popup buffers.
//! The bitmap is uploaded to the plugin's own egui `Context` as a texture and composited by
//! the host over the surface region (mesh textures_delta channel, same path as the font
//! atlas) — no separate host `CanvasTextureCache` layer is involved.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use egui::{Color32, ColorImage, Pos2, Rect, TextureHandle, Vec2};

/// Default blank-canvas dimensions when an image surface is created without a file.
pub const DEFAULT_BLANK_CANVAS_WIDTH: usize = 800;
pub const DEFAULT_BLANK_CANVAS_HEIGHT: usize = 600;

/// Image file extensions recognized for directory navigation.
pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tiff", "tif", "svg",
];

// ── Drawing action / history types ──

/// A single undoable drawing action.
#[derive(Clone)]
pub enum DrawAction {
    Stroke {
        points: Vec<(Pos2, Pos2)>,
        brush_size: f32,
        color: Color32,
    },
    PasteImage {
        image: ColorImage,
        position: Vec2,
        size: [usize; 2],
    },
}

/// Tracks drawing actions for undo/redo.
pub struct ActionHistory {
    actions: Vec<DrawAction>,
    redo_stack: Vec<DrawAction>,
}

impl ActionHistory {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn push(&mut self, action: DrawAction) {
        self.actions.push(action);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> Option<DrawAction> {
        let a = self.actions.pop()?;
        self.redo_stack.push(a.clone());
        Some(a)
    }

    pub fn redo(&mut self) -> Option<DrawAction> {
        let a = self.redo_stack.pop()?;
        self.actions.push(a.clone());
        Some(a)
    }

    pub fn can_undo(&self) -> bool {
        !self.actions.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Replay all actions onto a fresh transparent layer.
    pub fn replay(&self, base_size: [usize; 2]) -> ColorImage {
        let mut layer = ColorImage::new(base_size, Color32::TRANSPARENT);
        let [w, h] = base_size;
        for action in &self.actions {
            match action {
                DrawAction::Stroke {
                    points,
                    brush_size,
                    color,
                } => {
                    let radius = (*brush_size / 2.0).max(0.5);
                    for &(from, to) in points {
                        bresenham_thick_line(&mut layer, from, to, radius, *color, w, h);
                    }
                }
                DrawAction::PasteImage {
                    image: paste_img,
                    position,
                    size,
                } => {
                    blit_image(&mut layer, paste_img, *position, *size, w, h);
                }
            }
        }
        layer
    }
}

/// In-progress stroke being built during a mouse drag.
pub struct StrokeBuilder {
    pub points: Vec<(Pos2, Pos2)>,
    pub brush_size: f32,
    pub color: Color32,
}

/// Resize handle position on the floating selection border.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

/// Drag interaction state for a floating selection.
#[derive(Debug, Clone)]
pub enum DragState {
    Idle,
    Moving {
        drag_start_pos: Pos2,
        initial_position: Vec2,
    },
    Resizing {
        /// 향후 hit-area 별 resize 동작 분기 시 사용 — debug 인식용.
        #[allow(dead_code)] // variant 필드 — 현재 미read, 향후 resize 분기용 보존
        handle: ResizeHandle,
        drag_start_pos: Pos2,
        initial_rect: Rect,
    },
}

/// A pasted image floating over the canvas, waiting to be committed.
pub struct FloatingSelection {
    pub image: ColorImage,
    pub texture: Option<TextureHandle>,
    pub position: Vec2,
    pub size: [usize; 2],
    pub drag_state: DragState,
}

/// Edit session state for the image document.
pub enum EditState {
    Inactive,
    Drawing {
        history: ActionHistory,
        current_stroke: Option<StrokeBuilder>,
    },
    FloatingSelection {
        selection: FloatingSelection,
        history: ActionHistory,
    },
}

/// Per-surface image document state owned by the plugin.
pub struct ImageDoc {
    // ── Navigation / identity (former ImagePanel) ──
    /// `None` = blank canvas not yet saved to disk.
    pub file_path: Option<String>,
    /// Sibling images in the same directory (sorted), used for prev/next navigation.
    pub dir_images: Vec<String>,
    /// Index into `dir_images` for the currently displayed file.
    pub current_index: usize,

    // ── Viewer state ──
    pub original_image: Option<ColorImage>,
    pub texture: Option<TextureHandle>,
    pub zoom: f32,
    pub pan_offset: Vec2,
    /// Last known mtime of `file_path` at load time.
    pub last_mtime: Option<SystemTime>,

    // ── Drawing state ──
    pub edit_state: EditState,
    pub draw_layer: Option<ColorImage>,
    pub draw_texture: Option<TextureHandle>,
    pub brush_size: f32,
    pub brush_color: Color32,
    pub last_draw_pos: Option<Pos2>,
    pub draw_texture_dirty: bool,

    // ── New-image popup buffers ──
    pub new_image_popup: bool,
    pub new_image_width: String,
    pub new_image_height: String,

    // ── Save-path popup buffers ──
    pub save_path_popup: bool,
    pub save_path_buffer: String,

    /// True until pixels are first loaded — the plugin lazily loads on first paint.
    loaded: bool,
    /// True once the brush color has been seeded from the theme (accent-danger). The
    /// default lives in the `Theme` (delivered via set_context), not hardcoded here.
    themed_brush: bool,
}

impl ImageDoc {
    /// Create a document from an optional file path (None = blank canvas surface).
    pub fn new(file: Option<String>) -> Self {
        let (dir_images, current_index) = match &file {
            Some(f) => {
                let dir = scan_directory_images(f);
                let idx = dir.iter().position(|p| p == f).unwrap_or(0);
                (dir, idx)
            }
            None => (Vec::new(), 0),
        };
        Self {
            file_path: file,
            dir_images,
            current_index,
            original_image: None,
            texture: None,
            zoom: 1.0,
            pan_offset: Vec2::ZERO,
            last_mtime: None,
            edit_state: EditState::Inactive,
            draw_layer: None,
            draw_texture: None,
            brush_size: 2.0,
            brush_color: Color32::TRANSPARENT,
            last_draw_pos: None,
            draw_texture_dirty: false,
            new_image_popup: false,
            new_image_width: DEFAULT_BLANK_CANVAS_WIDTH.to_string(),
            new_image_height: DEFAULT_BLANK_CANVAS_HEIGHT.to_string(),
            save_path_popup: false,
            save_path_buffer: String::new(),
            loaded: false,
            themed_brush: false,
        }
    }

    /// Seed the brush color from the theme (accent-danger) on the first themed frame.
    /// Keeps the paint default in the `Theme` rather than hardcoded.
    pub fn ensure_brush_themed(&mut self, accent_danger: Color32) {
        if !self.themed_brush {
            self.brush_color = accent_danger;
            self.themed_brush = true;
        }
    }

    /// True when an editing session is active (drawing or floating selection).
    pub fn is_editing(&self) -> bool {
        !matches!(self.edit_state, EditState::Inactive)
    }

    /// Lazy-load pixel data on first paint. Cheap to call repeatedly.
    pub fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(p) = self.file_path.clone() {
            let (img, mtime) = load_image_from_path(&p);
            self.original_image = img;
            self.last_mtime = mtime;
        } else {
            // Blank canvas — start in edit mode so the user/agent can draw immediately.
            self.original_image = Some(ColorImage::new(
                [DEFAULT_BLANK_CANVAS_WIDTH, DEFAULT_BLANK_CANVAS_HEIGHT],
                Color32::WHITE,
            ));
            self.enter_edit_mode();
        }
    }

    /// Reload from `file_path` regardless of mtime, keeping any edit session cleared.
    pub fn reload_from_disk(&mut self) {
        if let Some(path) = self.file_path.clone() {
            let (img, mtime) = load_image_from_path(&path);
            self.original_image = img;
            self.last_mtime = mtime;
            self.texture = None;
        }
    }

    /// Step one image backward in the directory. Returns the new path on success.
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

    /// Step one image forward in the directory. Returns the new path on success.
    pub fn step_next(&mut self) -> Option<String> {
        if self.dir_images.is_empty() {
            return None;
        }
        self.current_index = (self.current_index + 1) % self.dir_images.len();
        let path = self.dir_images.get(self.current_index)?.clone();
        self.file_path = Some(path.clone());
        Some(path)
    }

    /// After navigation updated `file_path`, load the new file and reset zoom/pan/edit.
    pub fn load_after_navigation(&mut self) {
        if self.is_editing() {
            return;
        }
        if let Some(path) = self.file_path.clone() {
            let (img, mtime) = load_image_from_path(&path);
            self.original_image = img;
            self.last_mtime = mtime;
            self.texture = None;
            self.zoom = 1.0;
            self.pan_offset = Vec2::ZERO;
            self.exit_edit_mode();
        }
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

    pub fn is_blank(&self) -> bool {
        self.file_path.is_none()
    }

    /// Enter edit mode by allocating a transparent overlay matching the original size.
    pub fn enter_edit_mode(&mut self) {
        if let Some(ref img) = self.original_image {
            let [w, h] = img.size;
            self.draw_layer = Some(ColorImage::new([w, h], Color32::TRANSPARENT));
            self.draw_texture = None;
            self.edit_state = EditState::Drawing {
                history: ActionHistory::new(),
                current_stroke: None,
            };
            self.last_draw_pos = None;
            self.draw_texture_dirty = true;
        }
    }

    /// Exit edit mode, discarding the draw layer.
    pub fn exit_edit_mode(&mut self) {
        self.edit_state = EditState::Inactive;
        self.draw_layer = None;
        self.draw_texture = None;
        self.last_draw_pos = None;
        self.draw_texture_dirty = false;
    }

    /// Replace the original image with a fresh blank canvas and enter edit mode.
    pub fn create_blank_canvas(&mut self, width: usize, height: usize) {
        self.original_image = Some(ColorImage::new([width, height], Color32::WHITE));
        self.texture = None;
        self.zoom = 1.0;
        self.pan_offset = Vec2::ZERO;
        self.enter_edit_mode();
        self.new_image_popup = false;
    }

    /// Paste an image as a floating selection.
    pub fn paste_image(&mut self, image: ColorImage) {
        let size = image.size;

        let make_selection = |img: ColorImage, sz: [usize; 2]| FloatingSelection {
            image: img,
            texture: None,
            position: Vec2::ZERO,
            size: sz,
            drag_state: DragState::Idle,
        };

        match std::mem::replace(&mut self.edit_state, EditState::Inactive) {
            EditState::Inactive => {
                if let Some(ref orig) = self.original_image {
                    let [w, h] = orig.size;
                    self.draw_layer = Some(ColorImage::new([w, h], Color32::TRANSPARENT));
                    self.draw_texture = None;
                    self.draw_texture_dirty = true;
                }
                self.edit_state = EditState::FloatingSelection {
                    selection: make_selection(image, size),
                    history: ActionHistory::new(),
                };
            }
            EditState::Drawing {
                history,
                current_stroke: _,
            } => {
                self.edit_state = EditState::FloatingSelection {
                    selection: make_selection(image, size),
                    history,
                };
            }
            EditState::FloatingSelection {
                selection: old_sel,
                mut history,
            } => {
                Self::do_commit(
                    &mut self.draw_layer,
                    &mut self.draw_texture_dirty,
                    &old_sel,
                    &mut history,
                );
                self.edit_state = EditState::FloatingSelection {
                    selection: make_selection(image, size),
                    history,
                };
            }
        }
    }

    pub fn commit_floating(&mut self) {
        if let EditState::FloatingSelection {
            selection,
            mut history,
        } = std::mem::replace(&mut self.edit_state, EditState::Inactive)
        {
            Self::do_commit(
                &mut self.draw_layer,
                &mut self.draw_texture_dirty,
                &selection,
                &mut history,
            );
            self.edit_state = EditState::Drawing {
                history,
                current_stroke: None,
            };
        }
    }

    pub fn cancel_floating(&mut self) {
        if let EditState::FloatingSelection { history, .. } =
            std::mem::replace(&mut self.edit_state, EditState::Inactive)
        {
            self.edit_state = EditState::Drawing {
                history,
                current_stroke: None,
            };
        }
    }

    fn do_commit(
        draw_layer: &mut Option<ColorImage>,
        dirty: &mut bool,
        selection: &FloatingSelection,
        history: &mut ActionHistory,
    ) {
        if let Some(layer) = draw_layer {
            let [w, h] = layer.size;
            blit_image(
                layer,
                &selection.image,
                selection.position,
                selection.size,
                w,
                h,
            );
            *dirty = true;
        }
        history.push(DrawAction::PasteImage {
            image: selection.image.clone(),
            position: selection.position,
            size: selection.size,
        });
    }

    pub fn start_stroke(&mut self) {
        if let EditState::Drawing { current_stroke, .. } = &mut self.edit_state {
            *current_stroke = Some(StrokeBuilder {
                points: Vec::new(),
                brush_size: self.brush_size,
                color: self.brush_color,
            });
        }
    }

    pub fn finish_stroke(&mut self) {
        if let EditState::Drawing {
            history,
            current_stroke,
        } = &mut self.edit_state
            && let Some(stroke) = current_stroke.take()
            && !stroke.points.is_empty()
        {
            history.push(DrawAction::Stroke {
                points: stroke.points,
                brush_size: stroke.brush_size,
                color: stroke.color,
            });
        }
    }

    pub fn draw_line(&mut self, from: Pos2, to: Pos2) {
        let layer = match self.draw_layer.as_mut() {
            Some(l) => l,
            None => return,
        };
        let [w, h] = layer.size;
        let radius = (self.brush_size / 2.0).max(0.5);
        let color = self.brush_color;

        bresenham_thick_line(layer, from, to, radius, color, w, h);

        if let EditState::Drawing { current_stroke, .. } = &mut self.edit_state
            && let Some(stroke) = current_stroke
        {
            stroke.points.push((from, to));
        }

        self.draw_texture_dirty = true;
    }

    pub fn undo(&mut self) {
        if matches!(self.edit_state, EditState::FloatingSelection { .. }) {
            self.commit_floating();
        }
        if let EditState::Drawing { history, .. } = &mut self.edit_state
            && history.undo().is_some()
            && let Some(ref original) = self.original_image
        {
            self.draw_layer = Some(history.replay(original.size));
            self.draw_texture_dirty = true;
        }
    }

    pub fn redo(&mut self) {
        if matches!(self.edit_state, EditState::FloatingSelection { .. }) {
            self.commit_floating();
        }
        if let EditState::Drawing { history, .. } = &mut self.edit_state
            && history.redo().is_some()
            && let Some(ref original) = self.original_image
        {
            self.draw_layer = Some(history.replay(original.size));
            self.draw_texture_dirty = true;
        }
    }

    pub fn can_undo(&self) -> bool {
        match &self.edit_state {
            EditState::Drawing { history, .. } => history.can_undo(),
            EditState::FloatingSelection { .. } => true, // commit + undo
            _ => false,
        }
    }

    pub fn can_redo(&self) -> bool {
        match &self.edit_state {
            EditState::Drawing { history, .. } => history.can_redo(),
            EditState::FloatingSelection { .. } => false,
            _ => false,
        }
    }

    /// Save the composited image (original + overlay + active floating selection) as PNG.
    pub fn save_png(&self, path: &str) -> Result<(), String> {
        let original = self.original_image.as_ref().ok_or("No image to save")?;
        let [w, h] = original.size;

        let mut composited = original.clone();
        if let Some(ref layer) = self.draw_layer {
            for i in 0..(w * h) {
                let bg = composited.pixels[i];
                let fg = layer.pixels[i];
                composited.pixels[i] = alpha_blend(bg, fg);
            }
        }

        if let EditState::FloatingSelection { ref selection, .. } = self.edit_state {
            blit_image(
                &mut composited,
                &selection.image,
                selection.position,
                selection.size,
                w,
                h,
            );
        }

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
}

// ── Free helper functions ──

/// Load an image from a file path, returning the ColorImage and modification time.
pub fn load_image_from_path(path: &str) -> (Option<ColorImage>, Option<SystemTime>) {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();

    let img = match image::open(path) {
        Ok(img) => img,
        Err(_) => return (None, mtime),
    };

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    // 외부 입력 (이미지 파일 픽셀) → Color32. 정당한 dangerously 사용처.
    #[allow(clippy::disallowed_methods)]
    let pixels: Vec<Color32> = rgba
        .pixels()
        .map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
        .collect();

    (
        Some(ColorImage {
            size: [w as usize, h as usize],
            pixels,
        }),
        mtime,
    )
}

/// Returns true if the path's extension is a recognised image type.
pub fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Scan the directory of the given file path for image files (sorted).
pub fn scan_directory_images(file_path: &str) -> Vec<String> {
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

/// Alpha blend foreground over background.
pub fn alpha_blend(bg: Color32, fg: Color32) -> Color32 {
    let fa = fg.a() as f32 / 255.0;
    if fa < 0.001 {
        return bg;
    }
    let ba = bg.a() as f32 / 255.0;
    let out_a = fa + ba * (1.0 - fa);
    if out_a < 0.001 {
        return Color32::TRANSPARENT;
    }
    let r = (fg.r() as f32 * fa + bg.r() as f32 * ba * (1.0 - fa)) / out_a;
    let g = (fg.g() as f32 * fa + bg.g() as f32 * ba * (1.0 - fa)) / out_a;
    let b = (fg.b() as f32 * fa + bg.b() as f32 * ba * (1.0 - fa)) / out_a;
    // alpha 합성 결과 — 입력 두 색의 변형. 정당한 dangerously 사용처.
    #[allow(clippy::disallowed_methods)]
    {
        Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, (out_a * 255.0) as u8)
    }
}

/// Draw a thick line using Bresenham's algorithm with a circle brush.
pub fn bresenham_thick_line(
    layer: &mut ColorImage,
    from: Pos2,
    to: Pos2,
    radius: f32,
    color: Color32,
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

/// Blit a source image onto a target layer at the given position, scaled to `dest_size`.
pub fn blit_image(
    layer: &mut ColorImage,
    src: &ColorImage,
    position: Vec2,
    dest_size: [usize; 2],
    layer_w: usize,
    layer_h: usize,
) {
    let [src_w, src_h] = src.size;
    let [dst_w, dst_h] = dest_size;
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return;
    }
    let ox = position.x as i32;
    let oy = position.y as i32;
    for dy in 0..dst_h as i32 {
        let py = oy + dy;
        if py < 0 || py >= layer_h as i32 {
            continue;
        }
        for dx in 0..dst_w as i32 {
            let px = ox + dx;
            if px < 0 || px >= layer_w as i32 {
                continue;
            }
            let sx = (dx as usize * src_w) / dst_w;
            let sy = (dy as usize * src_h) / dst_h;
            let fg = src.pixels[sy * src_w + sx];
            if fg.a() == 0 {
                continue;
            }
            let idx = py as usize * layer_w + px as usize;
            let bg = layer.pixels[idx];
            layer.pixels[idx] = alpha_blend(bg, fg);
        }
    }
}

/// Fill a circle of pixels at (cx, cy) with the given radius.
pub fn fill_circle(
    layer: &mut ColorImage,
    cx: f32,
    cy: f32,
    radius: f32,
    color: Color32,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_doc_starts_in_edit_mode_after_load() {
        let mut doc = ImageDoc::new(None);
        doc.ensure_loaded();
        assert!(doc.is_editing());
        assert!(doc.original_image.is_some());
    }

    #[test]
    fn missing_file_loads_as_none() {
        let mut doc = ImageDoc::new(Some("\0nonexistent".into()));
        doc.ensure_loaded();
        assert!(doc.original_image.is_none());
        assert!(!doc.is_editing());
    }

    #[test]
    fn undo_redo_roundtrip_on_blank() {
        let mut doc = ImageDoc::new(None);
        doc.ensure_loaded();
        doc.start_stroke();
        doc.draw_line(Pos2::new(1.0, 1.0), Pos2::new(5.0, 5.0));
        doc.finish_stroke();
        assert!(doc.can_undo());
        doc.undo();
        assert!(doc.can_redo());
        doc.redo();
        assert!(doc.can_undo());
    }
}
