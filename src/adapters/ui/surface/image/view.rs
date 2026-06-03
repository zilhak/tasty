//! Host-side per-surface state for `ImagePanel` rendering and editing. Holds the loaded
//! pixel buffer, GPU texture handles, edit-mode state machine (drawing, floating
//! selection), action history (undo/redo), and popup buffers — none of which belong in
//! the GUI-free `tasty-core` model.

use std::collections::HashMap;
use std::time::SystemTime;

use egui::{Color32, ColorImage, Pos2, Rect, TextureHandle, Vec2};

use crate::model::{
    DEFAULT_BLANK_CANVAS_HEIGHT, DEFAULT_BLANK_CANVAS_WIDTH, ImagePanel, SurfaceId,
};

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
        #[allow(dead_code)]
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

/// Edit session state for the image view.
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

/// Per-surface host view state for an `ImagePanel`.
pub struct ImageView {
    // ── Viewer state ──
    pub original_image: Option<ColorImage>,
    pub texture: Option<TextureHandle>,
    pub zoom: f32,
    pub pan_offset: Vec2,
    /// Last known mtime of `panel.file_path` at load time. Used so the explicit
    /// "Reload" button can determine whether the disk file has changed.
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
}

impl Default for ImageView {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageView {
    pub fn new() -> Self {
        Self {
            original_image: None,
            texture: None,
            zoom: 1.0,
            pan_offset: Vec2::ZERO,
            last_mtime: None,
            edit_state: EditState::Inactive,
            draw_layer: None,
            draw_texture: None,
            brush_size: 2.0,
            brush_color: crate::theme::theme().red.into(),
            last_draw_pos: None,
            draw_texture_dirty: false,
            new_image_popup: false,
            new_image_width: DEFAULT_BLANK_CANVAS_WIDTH.to_string(),
            new_image_height: DEFAULT_BLANK_CANVAS_HEIGHT.to_string(),
            save_path_popup: false,
            save_path_buffer: String::new(),
        }
    }

    /// True when an editing session is active (drawing or floating selection).
    pub fn is_editing(&self) -> bool {
        !matches!(self.edit_state, EditState::Inactive)
    }

    /// Lazy-load pixel data on first render. Cheap to call repeatedly. Returns true
    /// if the view freshly initialised pixels (caller may want to invalidate textures).
    pub fn ensure_loaded(&mut self, panel: &ImagePanel) -> bool {
        if self.original_image.is_some() {
            return false;
        }
        if let Some(p) = panel.file_path.as_deref() {
            let (img, mtime) = load_image_from_path(p);
            self.original_image = img;
            self.last_mtime = mtime;
        } else {
            // Blank canvas — start in edit mode so user/agent can draw immediately.
            self.original_image = Some(ColorImage::new(
                [DEFAULT_BLANK_CANVAS_WIDTH, DEFAULT_BLANK_CANVAS_HEIGHT],
                Color32::WHITE,
            ));
            self.enter_edit_mode();
        }
        true
    }

    /// Reload from `panel.file_path` regardless of mtime, exiting any edit session.
    pub fn reload_from_disk(&mut self, panel: &ImagePanel) {
        if let Some(ref path) = panel.file_path {
            let (img, mtime) = load_image_from_path(path);
            self.original_image = img;
            self.last_mtime = mtime;
            self.texture = None;
        }
    }

    /// After a navigation step (`step_prev`/`step_next`) updated `panel.file_path`,
    /// load the new file and reset zoom/pan/edit state.
    pub fn load_after_navigation(&mut self, panel: &ImagePanel) {
        if self.is_editing() {
            return;
        }
        if let Some(ref path) = panel.file_path {
            let (img, mtime) = load_image_from_path(path);
            self.original_image = img;
            self.last_mtime = mtime;
            self.texture = None;
            self.zoom = 1.0;
            self.pan_offset = Vec2::ZERO;
            self.exit_edit_mode();
        }
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

    /// Replace the original image with a fresh blank canvas of the given size and
    /// enter edit mode. The caller (UI button) typically reads width/height from
    /// `new_image_width`/`new_image_height` buffers.
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
        {
            if let Some(stroke) = current_stroke.take() {
                if !stroke.points.is_empty() {
                    history.push(DrawAction::Stroke {
                        points: stroke.points,
                        brush_size: stroke.brush_size,
                        color: stroke.color,
                    });
                }
            }
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

        if let EditState::Drawing { current_stroke, .. } = &mut self.edit_state {
            if let Some(stroke) = current_stroke {
                stroke.points.push((from, to));
            }
        }

        self.draw_texture_dirty = true;
    }

    pub fn undo(&mut self) {
        if matches!(self.edit_state, EditState::FloatingSelection { .. }) {
            self.commit_floating();
        }
        if let EditState::Drawing { history, .. } = &mut self.edit_state {
            if history.undo().is_some() {
                if let Some(ref original) = self.original_image {
                    self.draw_layer = Some(history.replay(original.size));
                    self.draw_texture_dirty = true;
                }
            }
        }
    }

    pub fn redo(&mut self) {
        if matches!(self.edit_state, EditState::FloatingSelection { .. }) {
            self.commit_floating();
        }
        if let EditState::Drawing { history, .. } = &mut self.edit_state {
            if history.redo().is_some() {
                if let Some(ref original) = self.original_image {
                    self.draw_layer = Some(history.replay(original.size));
                    self.draw_texture_dirty = true;
                }
            }
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

#[derive(Default)]
pub struct ImageViewStore {
    views: HashMap<SurfaceId, ImageView>,
}

impl ImageViewStore {
    pub fn get_or_init(&mut self, panel: &mut ImagePanel) -> &mut ImageView {
        let view = self.views.entry(panel.id).or_insert_with(ImageView::new);
        view.ensure_loaded(panel);
        view
    }

    /// Direct mutable access without panel. Returns `None` if no view has been created yet
    /// (i.e. the surface has not been rendered). Used by shortcut handlers that only need
    /// to act on an already-active view (undo/redo, paste).
    pub fn get_mut(&mut self, sid: SurfaceId) -> Option<&mut ImageView> {
        self.views.get_mut(&sid)
    }

    pub fn drop_view(&mut self, sid: SurfaceId) {
        self.views.remove(&sid);
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

/// Draw a thick line using Bresenham's algorithm with circle brush.
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
#[path = "view_tests.rs"]
mod tests;
