use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
};

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
}

impl FontConfig {
    pub fn new(font_size: f32) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        let metrics = Self::measure_cell(&mut font_system, font_size);

        Self {
            font_system,
            swash_cache,
            metrics,
        }
    }

    fn measure_cell(font_system: &mut FontSystem, font_size: f32) -> FontMetrics {
        let line_height = (font_size * 1.2).ceil();
        let cosmic_metrics = Metrics::new(font_size, line_height);

        let mut buffer = Buffer::new(font_system, cosmic_metrics);
        buffer.set_size(font_system, Some(font_size * 10.0), Some(line_height * 2.0));
        buffer.set_text(
            font_system,
            "M",
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(font_system, false);

        // Measure the width of 'M' by looking at layout runs
        let mut cell_width = font_size * 0.6; // fallback
        let mut baseline = line_height * 0.8; // fallback
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                cell_width = glyph.w;
                break;
            }
            // line_y is the baseline position within the layout
            baseline = run.line_y;
            break;
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

    fn rasterize_glyph(
        &mut self,
        key: GlyphKey,
        font_config: &mut FontConfig,
        queue: &wgpu::Queue,
    ) -> Option<AtlasEntry> {
        let font_size = font_config.metrics.font_size;
        let line_height = font_config.metrics.cell_height;
        let cosmic_metrics = Metrics::new(font_size, line_height);

        let mut buffer = Buffer::new(&mut font_config.font_system, cosmic_metrics);
        buffer.set_size(
            &mut font_config.font_system,
            Some(font_size * 4.0),
            Some(line_height * 2.0),
        );

        let mut attrs = Attrs::new().family(Family::Monospace);
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

        // Find the glyph in the layout
        let mut found_glyph = None;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                found_glyph = Some((glyph.physical((0.0, 0.0), 1.0), run.line_y));
                break;
            }
            if found_glyph.is_some() {
                break;
            }
        }

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
                    .map(|pixel| {
                        ((pixel[0] as u16 + pixel[1] as u16 + pixel[2] as u16) / 3) as u8
                    })
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
            tracing::warn!("glyph atlas full, cannot add glyph '{}'", key.ch);
            return None;
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
