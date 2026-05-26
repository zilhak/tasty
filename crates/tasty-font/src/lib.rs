use std::collections::{BTreeSet, HashMap};

use cosmic_text::{
    Attrs, Buffer, FamilyOwned, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};

/// Family name of the bundled D2Coding ligature font.
/// Must match the `family` entry in the ttf `name` table exactly
/// (lowercase `l`, single space — verified against NAVER Ver 1.3.2).
pub const D2CODING_FAMILY: &str = "D2Coding ligature";

/// D2Coding ligature Regular ttf bytes embedded at compile time (OFL 1.1).
pub const D2CODING_REGULAR_TTF: &[u8] = include_bytes!("../assets/D2Coding-ligature-Regular.ttf");

/// D2Coding ligature Bold ttf bytes embedded at compile time (OFL 1.1).
pub const D2CODING_BOLD_TTF: &[u8] = include_bytes!("../assets/D2Coding-ligature-Bold.ttf");

/// Font metrics for monospace grid layout.
pub struct FontMetrics {
    pub cell_width: f32,
    pub cell_height: f32,
    pub font_size: f32,
    /// Baseline position within a cell (distance from cell top to text baseline)
    pub baseline: f32,
}

/// Font configuration holding cosmic-text state.
pub struct FontConfig {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub metrics: FontMetrics,
    /// The font family used for rendering glyphs.
    pub font_family: FamilyOwned,
}

impl FontConfig {
    /// Create a new FontConfig with the given font size, family name, and optional custom font file.
    /// If `font_family` is empty or "monospace", the bundled D2Coding ligature font is used.
    pub fn new(font_size: f32, font_family: &str) -> Self {
        Self::with_options(font_size, font_family, "", 1.0)
    }

    /// Create a new FontConfig with all options.
    pub fn with_options(
        font_size: f32,
        font_family: &str,
        custom_font_path: &str,
        line_height_mult: f32,
    ) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        font_system
            .db_mut()
            .load_font_data(D2CODING_REGULAR_TTF.to_vec());
        font_system
            .db_mut()
            .load_font_data(D2CODING_BOLD_TTF.to_vec());

        if !custom_font_path.is_empty() {
            if let Ok(data) = std::fs::read(custom_font_path) {
                font_system.db_mut().load_font_data(data);
                tracing::info!("Loaded custom font file: {}", custom_font_path);
            } else {
                tracing::warn!("Failed to load custom font file: {}", custom_font_path);
            }
        }

        let family = if font_family.is_empty() || font_family.eq_ignore_ascii_case("monospace") {
            FamilyOwned::Name(D2CODING_FAMILY.to_string().into())
        } else {
            FamilyOwned::Name(font_family.to_string().into())
        };

        let metrics = Self::measure_cell(&mut font_system, font_size, &family, line_height_mult);

        Self {
            font_system,
            swash_cache,
            metrics,
            font_family: family,
        }
    }

    /// Reconfigure font settings without rebuilding the FontSystem from scratch.
    /// Reuses the existing system font database to avoid the ~180ms FontSystem::new() scan.
    /// Only reloads the custom font if the path changed.
    pub fn reconfigure(
        &mut self,
        font_size: f32,
        font_family: &str,
        custom_font_path: &str,
        line_height_mult: f32,
    ) {
        // Load custom font if path is non-empty (additive; duplicates are harmless)
        if !custom_font_path.is_empty() {
            if let Ok(data) = std::fs::read(custom_font_path) {
                self.font_system.db_mut().load_font_data(data);
                tracing::info!("Loaded custom font file: {}", custom_font_path);
            } else {
                tracing::warn!("Failed to load custom font file: {}", custom_font_path);
            }
        }

        let family = if font_family.is_empty() || font_family.eq_ignore_ascii_case("monospace") {
            FamilyOwned::Name(D2CODING_FAMILY.to_string().into())
        } else {
            FamilyOwned::Name(font_family.to_string().into())
        };

        self.metrics =
            Self::measure_cell(&mut self.font_system, font_size, &family, line_height_mult);
        self.font_family = family;
        self.swash_cache = SwashCache::new();
    }

    /// Load raw font data bytes for a given family name.
    /// Returns the font data if found in the system font database.
    pub fn load_family_data(&self, family: &str) -> Option<Vec<u8>> {
        for face in self.font_system.db().faces() {
            for (name, _) in &face.families {
                if name.eq_ignore_ascii_case(family) {
                    let mut result = None;
                    self.font_system.db().with_face_data(face.id, |data, _| {
                        result = Some(data.to_vec());
                    });
                    if result.is_some() {
                        return result;
                    }
                }
            }
        }
        None
    }

    /// List all available font family names from the system, sorted alphabetically.
    pub fn list_families(&self) -> Vec<String> {
        let mut families = BTreeSet::new();
        for face in self.font_system.db().faces() {
            for (name, _) in &face.families {
                families.insert(name.clone());
            }
        }
        families.into_iter().collect()
    }

    fn measure_cell(
        font_system: &mut FontSystem,
        font_size: f32,
        family: &FamilyOwned,
        line_height_mult: f32,
    ) -> FontMetrics {
        let line_height = (font_size * line_height_mult).ceil();
        let cosmic_metrics = Metrics::new(font_size, line_height);

        let mut buffer = Buffer::new(font_system, cosmic_metrics);
        buffer.set_size(font_system, Some(font_size * 10.0), Some(line_height * 2.0));
        buffer.set_text(
            font_system,
            "M",
            &Attrs::new().family(family.as_family()),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(font_system, false);

        // Measure the width of 'M' by looking at layout runs
        let mut cell_width = font_size * 0.6; // fallback
        let mut baseline = line_height * 0.8; // fallback
        if let Some(run) = buffer.layout_runs().next() {
            if let Some(glyph) = run.glyphs.first() {
                cell_width = glyph.w;
            }
            // line_y is the baseline position within the layout
            baseline = run.line_y;
        }

        FontMetrics {
            cell_width: cell_width.ceil(),
            cell_height: line_height,
            font_size,
            baseline,
        }
    }
}

/// Key for glyph cache lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub ch: char,
    pub bold: bool,
    pub italic: bool,
}

/// Location of a glyph within the atlas texture.
#[derive(Debug, Clone, Copy)]
pub struct AtlasEntry {
    /// UV coordinates in 0..1 range
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    /// Pixel offset from cell origin to place the glyph bitmap
    pub offset_x: f32,
    pub offset_y: f32,
    /// Pixel size of the glyph bitmap
    pub width: f32,
    pub height: f32,
}

/// GPU texture atlas for glyph bitmaps.
/// Uses a simple shelf-based row packer.
pub struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    atlas_size: u32,
    cache: HashMap<GlyphKey, AtlasEntry>,
    /// Current shelf (row) packing state
    shelf_x: u32,
    shelf_y: u32,
    shelf_height: u32,
}

impl GlyphAtlas {
    pub const ATLAS_SIZE: u32 = 2048;

    pub fn new(device: &wgpu::Device) -> Self {
        let atlas_size = Self::ATLAS_SIZE;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
            atlas_size,
            cache: HashMap::new(),
            shelf_x: 0,
            shelf_y: 0,
            shelf_height: 0,
        }
    }

    /// Get or rasterize a glyph, returning its atlas entry.
    pub fn get_or_insert(
        &mut self,
        key: GlyphKey,
        font_config: &mut FontConfig,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        if let Some(entry) = self.cache.get(&key) {
            return Some(*entry);
        }

        self.rasterize_glyph(key, font_config, queue)
    }

    /// Try to rasterize a builtin block/box character programmatically.
    fn rasterize_builtin_glyph(
        &mut self,
        ch: char,
        font_config: &FontConfig,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        let cw = font_config.metrics.cell_width.round() as u32;
        let ch_px = font_config.metrics.cell_height.round() as u32;
        if cw == 0 || ch_px == 0 {
            return None;
        }

        let mut bitmap = vec![0u8; (cw * ch_px) as usize];

        if !draw_builtin_char(ch, &mut bitmap, cw, ch_px) {
            return None;
        }

        self.upload_builtin_bitmap(&bitmap, cw, ch_px, queue)
    }

    /// Upload a programmatically-generated bitmap to the atlas.
    fn upload_builtin_bitmap(
        &mut self,
        bitmap: &[u8],
        glyph_width: u32,
        glyph_height: u32,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        // Pack into atlas using shelf algorithm
        if self.shelf_x + glyph_width > self.atlas_size {
            self.shelf_y += self.shelf_height + 1;
            self.shelf_x = 0;
            self.shelf_height = 0;
        }

        if self.shelf_y + glyph_height > self.atlas_size {
            // Atlas full - fall back to swash
            return None;
        }

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.shelf_x,
                    y: self.shelf_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bitmap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(glyph_width),
                rows_per_image: Some(glyph_height),
            },
            wgpu::Extent3d {
                width: glyph_width,
                height: glyph_height,
                depth_or_array_layers: 1,
            },
        );

        let atlas_f = self.atlas_size as f32;
        let entry = AtlasEntry {
            uv_x: self.shelf_x as f32 / atlas_f,
            uv_y: self.shelf_y as f32 / atlas_f,
            uv_w: glyph_width as f32 / atlas_f,
            uv_h: glyph_height as f32 / atlas_f,
            offset_x: 0.0,
            offset_y: 0.0,
            width: glyph_width as f32,
            height: glyph_height as f32,
        };

        self.shelf_x += glyph_width + 1;
        self.shelf_height = self.shelf_height.max(glyph_height);

        Some(entry)
    }

    fn rasterize_glyph(
        &mut self,
        key: GlyphKey,
        font_config: &mut FontConfig,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        // Try builtin rendering for block elements and box drawing characters
        if !key.bold
            && !key.italic
            && let Some(entry) = self.rasterize_builtin_glyph(key.ch, font_config, queue)
        {
            self.cache.insert(key, entry);
            return Some(entry);
        }

        let font_size = font_config.metrics.font_size;
        let line_height = font_config.metrics.cell_height;
        let cosmic_metrics = Metrics::new(font_size, line_height);

        let mut buffer = Buffer::new(&mut font_config.font_system, cosmic_metrics);
        buffer.set_size(
            &mut font_config.font_system,
            Some(font_size * 4.0),
            Some(line_height * 2.0),
        );

        let mut attrs = Attrs::new().family(font_config.font_family.as_family());
        if key.bold {
            attrs = attrs.weight(cosmic_text::Weight::BOLD);
        }
        if key.italic {
            attrs = attrs.style(cosmic_text::Style::Italic);
        }

        let text = key.ch.to_string();
        buffer.set_text(
            &mut font_config.font_system,
            &text,
            &attrs,
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut font_config.font_system, false);

        // Find the first glyph in the first layout run.
        let found_glyph = buffer.layout_runs().find_map(|run| {
            run.glyphs
                .first()
                .map(|g| (g.physical((0.0, 0.0), 1.0), run.line_y))
        });

        let (physical_glyph, _line_y) = found_glyph?;

        // Rasterize the glyph using swash
        let image = font_config
            .swash_cache
            .get_image(&mut font_config.font_system, physical_glyph.cache_key)
            .as_ref()?;

        let glyph_width = image.placement.width;
        let glyph_height = image.placement.height;

        if glyph_width == 0 || glyph_height == 0 {
            // Space or invisible character - cache an empty entry
            let entry = AtlasEntry {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
                width: 0.0,
                height: 0.0,
            };
            self.cache.insert(key, entry);
            return Some(entry);
        }

        // Convert to grayscale if needed
        let grayscale_data: Vec<u8> = match image.content {
            SwashContent::Mask => image.data.clone(),
            SwashContent::Color => {
                // RGBA -> take alpha channel
                image.data.chunks_exact(4).map(|pixel| pixel[3]).collect()
            }
            SwashContent::SubpixelMask => {
                // RGB subpixel -> average to grayscale
                image
                    .data
                    .chunks_exact(3)
                    .map(|pixel| ((pixel[0] as u16 + pixel[1] as u16 + pixel[2] as u16) / 3) as u8)
                    .collect()
            }
        };

        // Pack into atlas using shelf algorithm
        if self.shelf_x + glyph_width > self.atlas_size {
            // Move to next shelf
            self.shelf_y += self.shelf_height + 1;
            self.shelf_x = 0;
            self.shelf_height = 0;
        }

        if self.shelf_y + glyph_height > self.atlas_size {
            // Atlas full - reset and rebuild. Existing glyphs will be re-rasterized on demand.
            tracing::warn!(
                "glyph atlas full, resetting ({} cached glyphs cleared)",
                self.cache.len()
            );
            self.cache.clear();
            self.shelf_x = 0;
            self.shelf_y = 0;
            self.shelf_height = 0;

            // Clear the texture by uploading zeroes
            let empty = vec![0u8; (self.atlas_size * self.atlas_size) as usize];
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &empty,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.atlas_size),
                    rows_per_image: Some(self.atlas_size),
                },
                wgpu::Extent3d {
                    width: self.atlas_size,
                    height: self.atlas_size,
                    depth_or_array_layers: 1,
                },
            );

            // Try again - if single glyph is too large, give up
            if glyph_height > self.atlas_size || glyph_width > self.atlas_size {
                tracing::warn!("glyph '{}' too large for atlas", key.ch);
                return None;
            }
        }

        // Upload glyph bitmap to texture
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.shelf_x,
                    y: self.shelf_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &grayscale_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(glyph_width),
                rows_per_image: Some(glyph_height),
            },
            wgpu::Extent3d {
                width: glyph_width,
                height: glyph_height,
                depth_or_array_layers: 1,
            },
        );

        // Glyph offset relative to cell origin:
        // placement.left is the horizontal bearing (distance from cell left to glyph left)
        // placement.top is the vertical bearing (distance from baseline to glyph top)
        // We need: offset from cell top-left to glyph top-left
        let offset_x = image.placement.left as f32;
        let offset_y = font_config.metrics.baseline - image.placement.top as f32;

        let atlas_f = self.atlas_size as f32;
        let entry = AtlasEntry {
            uv_x: self.shelf_x as f32 / atlas_f,
            uv_y: self.shelf_y as f32 / atlas_f,
            uv_w: glyph_width as f32 / atlas_f,
            uv_h: glyph_height as f32 / atlas_f,
            offset_x,
            offset_y,
            width: glyph_width as f32,
            height: glyph_height as f32,
        };

        self.shelf_x += glyph_width + 1;
        self.shelf_height = self.shelf_height.max(glyph_height);

        self.cache.insert(key, entry);
        Some(entry)
    }
}

// ---------------------------------------------------------------------------
// Builtin rendering for block elements (U+2580–U+259F) and box drawing (U+2500–U+257F)
// ---------------------------------------------------------------------------

/// Returns `true` if the character was handled (bitmap filled).
fn draw_builtin_char(ch: char, bitmap: &mut [u8], w: u32, h: u32) -> bool {
    let cp = ch as u32;
    match cp {
        0x2580..=0x259F => draw_block_element(cp, bitmap, w, h),
        0x2500..=0x257F => draw_box_drawing(cp, bitmap, w, h),
        _ => false,
    }
}

// ---- helpers ---------------------------------------------------------------

/// Fill a rectangle [x0, x1) × [y0, y1) with the given alpha value.
fn fill_rect(bitmap: &mut [u8], w: u32, _h: u32, x0: u32, y0: u32, x1: u32, y1: u32, alpha: u8) {
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (y * w + x) as usize;
            if idx < bitmap.len() {
                // Composite: max of existing and new so overlapping fills work
                let cur = bitmap[idx];
                bitmap[idx] = cur.max(alpha);
            }
        }
    }
}

/// Fill a horizontal line of given thickness centred at cy, from x0 to x1.
pub(crate) fn fill_hline(
    bitmap: &mut [u8],
    w: u32,
    h: u32,
    x0: u32,
    x1: u32,
    cy: u32,
    thickness: u32,
) {
    let half = thickness / 2;
    let y0 = cy.saturating_sub(half);
    let y1 = (cy + thickness - half).min(h);
    fill_rect(bitmap, w, h, x0, y0, x1, y1, 255);
}

/// Fill a vertical line of given thickness centred at cx, from y0 to y1.
pub(crate) fn fill_vline(
    bitmap: &mut [u8],
    w: u32,
    h: u32,
    y0: u32,
    y1: u32,
    cx: u32,
    thickness: u32,
) {
    let half = thickness / 2;
    let x0 = cx.saturating_sub(half);
    let x1 = (cx + thickness - half).min(w);
    fill_rect(bitmap, w, h, x0, y0, x1, y1, 255);
}

// ---- block elements --------------------------------------------------------

fn draw_block_element(cp: u32, bitmap: &mut [u8], w: u32, h: u32) -> bool {
    match cp {
        // U+2580  ▀  Upper half block
        0x2580 => fill_rect(bitmap, w, h, 0, 0, w, h / 2, 255),
        // U+2581–U+2587  Lower 1/8 … 7/8 blocks
        0x2581 => {
            let t = h / 8;
            fill_rect(bitmap, w, h, 0, h - t.max(1), w, h, 255);
        }
        0x2582 => {
            let t = h / 4;
            fill_rect(bitmap, w, h, 0, h - t.max(1), w, h, 255);
        }
        0x2583 => {
            let t = h * 3 / 8;
            fill_rect(bitmap, w, h, 0, h - t.max(1), w, h, 255);
        }
        0x2584 => fill_rect(bitmap, w, h, 0, h / 2, w, h, 255),
        0x2585 => {
            let t = h * 5 / 8;
            fill_rect(bitmap, w, h, 0, h - t.max(1), w, h, 255);
        }
        0x2586 => {
            let t = h * 3 / 4;
            fill_rect(bitmap, w, h, 0, h - t.max(1), w, h, 255);
        }
        0x2587 => {
            let t = h * 7 / 8;
            fill_rect(bitmap, w, h, 0, h - t.max(1), w, h, 255);
        }
        // U+2588  █  Full block
        0x2588 => fill_rect(bitmap, w, h, 0, 0, w, h, 255),
        // U+2589–U+258F  Left 7/8 … 1/8 blocks
        0x2589 => {
            let t = w * 7 / 8;
            fill_rect(bitmap, w, h, 0, 0, t.max(1), h, 255);
        }
        0x258A => {
            let t = w * 3 / 4;
            fill_rect(bitmap, w, h, 0, 0, t.max(1), h, 255);
        }
        0x258B => {
            let t = w * 5 / 8;
            fill_rect(bitmap, w, h, 0, 0, t.max(1), h, 255);
        }
        0x258C => fill_rect(bitmap, w, h, 0, 0, w / 2, h, 255),
        0x258D => {
            let t = w * 3 / 8;
            fill_rect(bitmap, w, h, 0, 0, t.max(1), h, 255);
        }
        0x258E => {
            let t = w / 4;
            fill_rect(bitmap, w, h, 0, 0, t.max(1), h, 255);
        }
        0x258F => {
            let t = w / 8;
            fill_rect(bitmap, w, h, 0, 0, t.max(1), h, 255);
        }
        // U+2590  ▐  Right half block
        0x2590 => fill_rect(bitmap, w, h, w / 2, 0, w, h, 255),
        // U+2591–U+2593  Shade characters
        0x2591 => fill_shade(bitmap, w, h, 64),
        0x2592 => fill_shade(bitmap, w, h, 128),
        0x2593 => fill_shade(bitmap, w, h, 191),
        // U+2594  ▔  Upper one eighth block
        0x2594 => {
            let t = (h / 8).max(1);
            fill_rect(bitmap, w, h, 0, 0, w, t, 255);
        }
        // U+2595  ▕  Right one eighth block
        0x2595 => {
            let t = (w / 8).max(1);
            fill_rect(bitmap, w, h, w - t, 0, w, h, 255);
        }
        // U+2596–U+259F  Quadrants
        0x2596 => fill_rect(bitmap, w, h, 0, h / 2, w / 2, h, 255), // lower left
        0x2597 => fill_rect(bitmap, w, h, w / 2, h / 2, w, h, 255), // lower right
        0x2598 => fill_rect(bitmap, w, h, 0, 0, w / 2, h / 2, 255), // upper left
        0x2599 => {
            // UL + LL + LR
            fill_rect(bitmap, w, h, 0, 0, w / 2, h / 2, 255);
            fill_rect(bitmap, w, h, 0, h / 2, w / 2, h, 255);
            fill_rect(bitmap, w, h, w / 2, h / 2, w, h, 255);
        }
        0x259A => {
            // UL + LR
            fill_rect(bitmap, w, h, 0, 0, w / 2, h / 2, 255);
            fill_rect(bitmap, w, h, w / 2, h / 2, w, h, 255);
        }
        0x259B => {
            // UL + UR + LL
            fill_rect(bitmap, w, h, 0, 0, w / 2, h / 2, 255);
            fill_rect(bitmap, w, h, w / 2, 0, w, h / 2, 255);
            fill_rect(bitmap, w, h, 0, h / 2, w / 2, h, 255);
        }
        0x259C => {
            // UL + UR + LR
            fill_rect(bitmap, w, h, 0, 0, w / 2, h / 2, 255);
            fill_rect(bitmap, w, h, w / 2, 0, w, h / 2, 255);
            fill_rect(bitmap, w, h, w / 2, h / 2, w, h, 255);
        }
        0x259D => fill_rect(bitmap, w, h, w / 2, 0, w, h / 2, 255), // upper right
        0x259E => {
            // UR + LL
            fill_rect(bitmap, w, h, w / 2, 0, w, h / 2, 255);
            fill_rect(bitmap, w, h, 0, h / 2, w / 2, h, 255);
        }
        0x259F => {
            // UR + LL + LR
            fill_rect(bitmap, w, h, w / 2, 0, w, h / 2, 255);
            fill_rect(bitmap, w, h, 0, h / 2, w / 2, h, 255);
            fill_rect(bitmap, w, h, w / 2, h / 2, w, h, 255);
        }
        _ => return false,
    }
    true
}

fn fill_shade(bitmap: &mut [u8], w: u32, h: u32, alpha: u8) {
    fill_rect(bitmap, w, h, 0, 0, w, h, alpha);
}
mod box_drawing;
use box_drawing::draw_box_drawing;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_config_default_monospace() {
        let config = FontConfig::new(14.0, "");
        match &config.font_family {
            FamilyOwned::Name(name) => assert_eq!(&**name, D2CODING_FAMILY),
            other => panic!("expected bundled D2Coding family, got {other:?}"),
        }
        assert_eq!(config.metrics.font_size, 14.0);
        assert!(config.metrics.cell_width > 0.0);
        assert!(config.metrics.cell_height > 0.0);
    }

    #[test]
    fn font_config_explicit_monospace() {
        let config = FontConfig::new(14.0, "monospace");
        match &config.font_family {
            FamilyOwned::Name(name) => assert_eq!(&**name, D2CODING_FAMILY),
            other => panic!("expected bundled D2Coding family, got {other:?}"),
        }
    }

    #[test]
    fn font_config_named_family() {
        let config = FontConfig::new(16.0, "JetBrains Mono");
        assert!(matches!(config.font_family, FamilyOwned::Name(_)));
        assert_eq!(config.metrics.font_size, 16.0);
        // Cell dimensions should be positive regardless of whether the font exists
        assert!(config.metrics.cell_width > 0.0);
        assert!(config.metrics.cell_height > 0.0);
    }

    #[test]
    fn font_config_different_sizes() {
        let small = FontConfig::new(10.0, "");
        let large = FontConfig::new(24.0, "");
        assert!(large.metrics.cell_height > small.metrics.cell_height);
    }
}
