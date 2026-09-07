#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use cosmic_text::{
    Attrs, Buffer, FamilyOwned, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};
use rustc_hash::FxHashMap;

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

    /// On-disk path of a font file providing `family`, when the matching face is
    /// backed by a file on disk. Used to resolve a language pack's
    /// `[font] family = …` to a concrete path so the same file can be handed to
    /// plugin processes via `TASTY_LOCALE_FONT` (they must not re-search the
    /// system DB). Returns `None` for in-memory faces or unknown families.
    pub fn family_source_path(&self, family: &str) -> Option<std::path::PathBuf> {
        for face in self.font_system.db().faces() {
            for (name, _) in &face.families {
                if name.eq_ignore_ascii_case(family) {
                    if let cosmic_text::fontdb::Source::File(path) = &face.source {
                        return Some(path.clone());
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
    /// Atlas page (D2Array layer) the glyph lives on.
    pub page: u32,
}

/// Per-page shelf-packing state. Pure-Rust — no wgpu dependency, so it
/// can be unit-tested without a device.
#[derive(Debug, Clone, Copy, Default)]
pub struct AtlasPage {
    pub shelf_x: u32,
    pub shelf_y: u32,
    pub shelf_height: u32,
    /// Number of cache entries currently allocated on this page (diagnostic).
    pub entry_count: u32,
    /// Frame index when this page was last touched (cache hit OR insertion).
    /// Used as the LRU tiebreaker when picking an eviction victim.
    pub last_access_frame: u64,
}

impl AtlasPage {
    /// Try to reserve a (w × h) box on this page.
    /// Returns the (x, y) origin if there is room; mutates the shelf cursor.
    pub fn try_allocate(&mut self, w: u32, h: u32, atlas_size: u32) -> Option<(u32, u32)> {
        if w == 0 || h == 0 || w > atlas_size || h > atlas_size {
            return None;
        }
        // Advance shelf if the current row can't fit horizontally.
        if self.shelf_x + w > atlas_size {
            self.shelf_y = self.shelf_y.saturating_add(self.shelf_height + 1);
            self.shelf_x = 0;
            self.shelf_height = 0;
        }
        if self.shelf_y + h > atlas_size {
            return None;
        }
        let origin = (self.shelf_x, self.shelf_y);
        self.shelf_x += w + 1;
        self.shelf_height = self.shelf_height.max(h);
        self.entry_count += 1;
        Some(origin)
    }

    /// Reset this page's packing state (used after eviction).
    pub fn reset(&mut self) {
        self.shelf_x = 0;
        self.shelf_y = 0;
        self.shelf_height = 0;
        self.entry_count = 0;
    }
}

/// Pick the eviction victim across `pages`, excluding the currently active
/// page. The victim is the page with the smallest `last_access_frame` (true
/// per-page LRU). Returns `None` if there is no non-active page.
pub fn pick_lru_victim(pages: &[AtlasPage], active_page: u32) -> Option<u32> {
    pages
        .iter()
        .enumerate()
        .filter(|(i, _)| (*i as u32) != active_page)
        .min_by_key(|(_, p)| p.last_access_frame)
        .map(|(i, _)| i as u32)
}

/// ASCII fast-path glyph cache: direct array index for printable ASCII
/// (code points 0..128) × bold × italic = 512 slots. Avoids hash lookup
/// on the hot path where ~95% of cell glyphs land.
struct AsciiCache {
    slots: Box<[Option<AtlasEntry>; 512]>,
}

impl AsciiCache {
    fn new() -> Self {
        Self {
            slots: Box::new([None; 512]),
        }
    }

    #[inline]
    fn index_of(key: &GlyphKey) -> Option<usize> {
        let cp = key.ch as u32;
        if cp < 128 {
            let base = (cp as usize) << 2;
            let off = (key.bold as usize) | ((key.italic as usize) << 1);
            Some(base | off)
        } else {
            None
        }
    }

    #[inline]
    fn get(&self, key: &GlyphKey) -> Option<AtlasEntry> {
        Self::index_of(key).and_then(|i| self.slots[i])
    }

    #[inline]
    fn insert(&mut self, key: &GlyphKey, entry: AtlasEntry) -> bool {
        if let Some(i) = Self::index_of(key) {
            self.slots[i] = Some(entry);
            true
        } else {
            false
        }
    }

    fn retain_pages_except(&mut self, victim_page: u32) {
        for slot in self.slots.iter_mut() {
            if let Some(e) = slot
                && e.page == victim_page
            {
                *slot = None;
            }
        }
    }
}

/// GPU texture atlas for glyph bitmaps, backed by a `D2Array` texture with
/// `MAX_PAGES` layers. Uses a simple shelf-based row packer per page. Once
/// all pages are full, the least-recently-used non-active page is evicted
/// (cache entries dropped + layer zero-cleared) and reused.
pub struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    atlas_size: u32,
    ascii_cache: AsciiCache,
    overflow_cache: FxHashMap<GlyphKey, AtlasEntry>,
    pages: Vec<AtlasPage>,
    /// Page where new glyphs are currently being packed.
    active_page: u32,
    /// Monotonic frame counter; bumped by `begin_frame()`. Used to stamp
    /// `last_access_frame` on each page touch.
    current_frame: u64,
    /// Set to `current_frame` when a page eviction happens; ensures we
    /// never evict more than once per frame to avoid thrashing.
    last_evict_frame: Option<u64>,
    /// Monotonic count of page evictions since atlas construction. Surfaced
    /// via `eviction_count()` for perf logging; never read by atlas logic.
    eviction_count: u64,
}

impl GlyphAtlas {
    pub const ATLAS_SIZE: u32 = 2048;
    /// Maximum number of atlas pages (D2Array layers). Fixed at construction:
    /// 4 × 2048² R8Unorm = 32 MiB resident memory.
    pub const MAX_PAGES: u32 = 4;

    pub fn new(device: &wgpu::Device) -> Self {
        let atlas_size = Self::ATLAS_SIZE;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: atlas_size,
                height: atlas_size,
                depth_or_array_layers: Self::MAX_PAGES,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("glyph_atlas_view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });

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
            ascii_cache: AsciiCache::new(),
            overflow_cache: FxHashMap::default(),
            pages: vec![AtlasPage::default(); Self::MAX_PAGES as usize],
            active_page: 0,
            current_frame: 0,
            last_evict_frame: None,
            eviction_count: 0,
        }
    }

    /// Bump the frame counter. Call once per render frame, before any
    /// `get_or_insert` calls, so LRU stamps are coherent.
    pub fn begin_frame(&mut self) {
        self.current_frame = self.current_frame.wrapping_add(1);
    }

    /// Monotonic eviction count since construction. Diagnostic only.
    pub fn eviction_count(&self) -> u64 {
        self.eviction_count
    }

    /// Number of pages that currently hold at least one glyph entry.
    pub fn active_page_count(&self) -> u32 {
        self.pages.iter().filter(|p| p.entry_count > 0).count() as u32
    }

    /// Sum of `entry_count` across all pages. Approximate live cache size.
    pub fn entry_count_sum(&self) -> u32 {
        self.pages.iter().map(|p| p.entry_count).sum()
    }

    /// Get or rasterize a glyph, returning its atlas entry.
    pub fn get_or_insert(
        &mut self,
        key: GlyphKey,
        font_config: &mut FontConfig,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        if let Some(entry) = self.ascii_cache.get(&key) {
            self.pages[entry.page as usize].last_access_frame = self.current_frame;
            return Some(entry);
        }
        if let Some(entry) = self.overflow_cache.get(&key).copied() {
            self.pages[entry.page as usize].last_access_frame = self.current_frame;
            return Some(entry);
        }
        self.rasterize_glyph(key, font_config, queue)
    }

    #[inline]
    fn cache_insert(&mut self, key: GlyphKey, entry: AtlasEntry) {
        if !self.ascii_cache.insert(&key, entry) {
            self.overflow_cache.insert(key, entry);
        }
    }

    /// Try to reserve a glyph box on some page, possibly evicting one if all
    /// pages are full. Returns `(page_index, x, y)` on success.
    fn allocate_box(&mut self, w: u32, h: u32, queue: &wgpu::Queue) -> Option<(u32, u32, u32)> {
        if w > self.atlas_size || h > self.atlas_size {
            return None;
        }
        let max_pages = self.pages.len() as u32;
        // Walk from active page through all pages once.
        for step in 0..max_pages {
            let idx = (self.active_page + step) % max_pages;
            if let Some((x, y)) = self.pages[idx as usize].try_allocate(w, h, self.atlas_size) {
                self.active_page = idx;
                self.pages[idx as usize].last_access_frame = self.current_frame;
                return Some((idx, x, y));
            }
        }
        // All pages full — evict the LRU non-active page (at most once per frame).
        if self.last_evict_frame == Some(self.current_frame) {
            tracing::warn!(
                "glyph atlas: second eviction skipped in frame {}; glyph deferred",
                self.current_frame
            );
            return None;
        }
        let victim = pick_lru_victim(&self.pages, self.active_page)?;
        tracing::warn!(
            "evicting atlas page {} ({} entries, last_access_frame={})",
            victim,
            self.pages[victim as usize].entry_count,
            self.pages[victim as usize].last_access_frame
        );
        // Drop cache entries that lived on this page.
        self.ascii_cache.retain_pages_except(victim);
        self.overflow_cache.retain(|_, e| e.page != victim);
        self.pages[victim as usize].reset();
        // Zero-clear the layer so stale glyph pixels don't bleed into new UVs.
        let empty = vec![0u8; (self.atlas_size * self.atlas_size) as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: victim,
                },
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
        self.last_evict_frame = Some(self.current_frame);
        self.eviction_count = self.eviction_count.saturating_add(1);
        let (x, y) = self.pages[victim as usize].try_allocate(w, h, self.atlas_size)?;
        self.active_page = victim;
        self.pages[victim as usize].last_access_frame = self.current_frame;
        Some((victim, x, y))
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

        self.upload_bitmap(&bitmap, cw, ch_px, 0.0, 0.0, queue)
    }

    /// Pack a bitmap into the next available page and upload it.
    fn upload_bitmap(
        &mut self,
        bitmap: &[u8],
        glyph_width: u32,
        glyph_height: u32,
        offset_x: f32,
        offset_y: f32,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        let (page, x, y) = self.allocate_box(glyph_width, glyph_height, queue)?;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: page },
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
        Some(AtlasEntry {
            uv_x: x as f32 / atlas_f,
            uv_y: y as f32 / atlas_f,
            uv_w: glyph_width as f32 / atlas_f,
            uv_h: glyph_height as f32 / atlas_f,
            offset_x,
            offset_y,
            width: glyph_width as f32,
            height: glyph_height as f32,
            page,
        })
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
            self.cache_insert(key, entry);
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
            // Space or invisible character - cache an empty entry on the active page.
            let entry = AtlasEntry {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
                offset_x: 0.0,
                offset_y: 0.0,
                width: 0.0,
                height: 0.0,
                page: self.active_page,
            };
            self.cache_insert(key, entry);
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

        // Glyph offset relative to cell origin:
        // placement.left is the horizontal bearing (distance from cell left to glyph left)
        // placement.top is the vertical bearing (distance from baseline to glyph top)
        // We need: offset from cell top-left to glyph top-left
        let offset_x = image.placement.left as f32;
        let offset_y = font_config.metrics.baseline - image.placement.top as f32;

        let entry = self.upload_bitmap(
            &grayscale_data,
            glyph_width,
            glyph_height,
            offset_x,
            offset_y,
            queue,
        )?;
        self.cache_insert(key, entry);
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
///
/// 같은 파일 내 box-drawing match arm 에서 62회 호출되는 private helper.
/// `(bitmap, w, h)` wrapper 도입 시 호출자 + 그 호출자의 내부 코드까지 도미노 변경이라
/// (A2) Target wrapper 대신 (C) `#[allow]` 채택. 좌표 4개 + alpha 의미는 graphics primitive 관습.
#[allow(clippy::too_many_arguments)] // reason: 62회 호출되는 내부 helper, wrapper 도입 시 호출자 도미노 변경 — graphics primitive 좌표 인자 관습
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

    // --- Atlas shelf packer + LRU page selection (device-free) ---

    #[test]
    fn atlas_page_first_alloc_lands_at_origin() {
        let mut page = AtlasPage::default();
        let (x, y) = page.try_allocate(32, 32, 2048).unwrap();
        assert_eq!((x, y), (0, 0));
        assert_eq!(page.shelf_x, 33);
        assert_eq!(page.shelf_height, 32);
        assert_eq!(page.entry_count, 1);
    }

    #[test]
    fn atlas_page_wraps_to_next_shelf_when_row_full() {
        let mut page = AtlasPage::default();
        // 2048 / 100 = 20 glyphs per shelf row; the 21st must wrap.
        for _ in 0..20 {
            page.try_allocate(100, 50, 2048).unwrap();
        }
        let before_y = page.shelf_y;
        let (_, y) = page.try_allocate(100, 50, 2048).unwrap();
        assert!(y > before_y, "expected wrap to next shelf, got y={y}");
        assert_eq!(page.shelf_x, 101);
    }

    #[test]
    fn atlas_page_returns_none_when_full() {
        let mut page = AtlasPage::default();
        // First shelf occupies y=0..1024. Anything taller than ~1023 in the
        // next shelf no longer fits within the 2048 atlas height.
        page.try_allocate(2000, 1024, 2048).unwrap();
        assert!(page.try_allocate(100, 1100, 2048).is_none());
    }

    #[test]
    fn atlas_page_rejects_oversized_glyph() {
        let mut page = AtlasPage::default();
        assert!(page.try_allocate(3000, 32, 2048).is_none());
        assert!(page.try_allocate(32, 3000, 2048).is_none());
        // State must not have advanced.
        assert_eq!(page.shelf_x, 0);
        assert_eq!(page.entry_count, 0);
    }

    #[test]
    fn lru_victim_picks_oldest_non_active_page() {
        let mut pages = vec![AtlasPage::default(); 4];
        pages[0].last_access_frame = 100;
        pages[1].last_access_frame = 50; // oldest
        pages[2].last_access_frame = 200;
        pages[3].last_access_frame = 75;
        let victim = pick_lru_victim(&pages, 2).unwrap();
        assert_eq!(victim, 1);
    }

    #[test]
    fn lru_victim_excludes_active_page() {
        let mut pages = vec![AtlasPage::default(); 4];
        pages[0].last_access_frame = 10; // would be oldest…
        pages[1].last_access_frame = 50;
        pages[2].last_access_frame = 200;
        pages[3].last_access_frame = 75;
        // …but 0 is the active page, so pick the next oldest.
        let victim = pick_lru_victim(&pages, 0).unwrap();
        assert_eq!(victim, 1);
    }

    #[test]
    fn lru_victim_returns_none_when_only_active_page_exists() {
        let pages = vec![AtlasPage::default(); 1];
        assert!(pick_lru_victim(&pages, 0).is_none());
    }

    #[test]
    fn atlas_page_reset_clears_state() {
        let mut page = AtlasPage::default();
        page.try_allocate(100, 100, 2048).unwrap();
        page.last_access_frame = 42;
        page.reset();
        assert_eq!(page.shelf_x, 0);
        assert_eq!(page.shelf_y, 0);
        assert_eq!(page.shelf_height, 0);
        assert_eq!(page.entry_count, 0);
        // last_access_frame intentionally preserved so the just-evicted page
        // doesn't immediately re-evict itself in the same frame.
        assert_eq!(page.last_access_frame, 42);
    }
}
