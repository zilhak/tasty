use bytemuck::{Pod, Zeroable};
use termwiz::color::ColorAttribute;
use termwiz::surface::Surface;

use crate::font::{FontConfig, GlyphAtlas, GlyphKey};
use crate::model::Rect;

// ---- GPU data types ----

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct Uniforms {
    cell_size: [f32; 2],
    grid_offset: [f32; 2],
    viewport_size: [f32; 2],
    _padding: [f32; 2],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct BgInstance {
    pos: [f32; 2],
    bg_color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct GlyphInstance {
    pos: [f32; 2],
    uv_offset: [f32; 2],
    uv_size: [f32; 2],
    fg_color: [f32; 4],
    glyph_offset: [f32; 2],
    glyph_size: [f32; 2],
}

// ---- Shaders ----

const BG_SHADER: &str = r#"
struct Uniforms {
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    viewport_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct BgInstance {
    @location(0) pos: vec2<f32>,
    @location(1) bg_color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, instance: BgInstance) -> VertexOutput {
    let quad_pos = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );

    let p = quad_pos[vi];
    let pixel_pos = (instance.pos + p) * uniforms.cell_size + uniforms.grid_offset;
    let ndc = pixel_pos / uniforms.viewport_size * 2.0 - 1.0;

    var out: VertexOutput;
    out.position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    out.color = instance.bg_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

const GLYPH_SHADER: &str = r#"
struct Uniforms {
    cell_size: vec2<f32>,
    grid_offset: vec2<f32>,
    viewport_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var atlas_texture: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct GlyphInstance {
    @location(0) pos: vec2<f32>,
    @location(1) uv_offset: vec2<f32>,
    @location(2) uv_size: vec2<f32>,
    @location(3) fg_color: vec4<f32>,
    @location(4) glyph_offset: vec2<f32>,
    @location(5) glyph_size: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg_color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, instance: GlyphInstance) -> VertexOutput {
    let quad_pos = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );

    let p = quad_pos[vi];
    let pixel_pos = instance.pos * uniforms.cell_size + uniforms.grid_offset
                    + instance.glyph_offset + p * instance.glyph_size;
    let ndc = pixel_pos / uniforms.viewport_size * 2.0 - 1.0;

    var out: VertexOutput;
    out.position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    out.uv = instance.uv_offset + p * instance.uv_size;
    out.fg_color = instance.fg_color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    return vec4(in.fg_color.rgb, in.fg_color.a * alpha);
}
"#;

// ---- Default terminal colors (xterm-256) ----

const DEFAULT_FG: [f32; 4] = [0.8, 0.8, 0.8, 1.0]; // #cccccc
const DEFAULT_BG: [f32; 4] = [0.102, 0.102, 0.118, 1.0]; // #1a1a1e

/// Standard 16-color ANSI palette (sRGB, approximate).
const ANSI_COLORS: [[f32; 3]; 16] = [
    [0.0, 0.0, 0.0],       // 0: black
    [0.8, 0.0, 0.0],       // 1: red
    [0.0, 0.8, 0.0],       // 2: green
    [0.8, 0.8, 0.0],       // 3: yellow
    [0.0, 0.0, 0.8],       // 4: blue
    [0.8, 0.0, 0.8],       // 5: magenta
    [0.0, 0.8, 0.8],       // 6: cyan
    [0.75, 0.75, 0.75],    // 7: white
    [0.5, 0.5, 0.5],       // 8: bright black
    [1.0, 0.0, 0.0],       // 9: bright red
    [0.0, 1.0, 0.0],       // 10: bright green
    [1.0, 1.0, 0.0],       // 11: bright yellow
    [0.0, 0.0, 1.0],       // 12: bright blue
    [1.0, 0.0, 1.0],       // 13: bright magenta
    [0.0, 1.0, 1.0],       // 14: bright cyan
    [1.0, 1.0, 1.0],       // 15: bright white
];

fn palette_index_to_rgb(idx: u8) -> [f32; 3] {
    if idx < 16 {
        ANSI_COLORS[idx as usize]
    } else if idx < 232 {
        // 216-color cube: 6x6x6
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        let to_f = |v: u8| if v == 0 { 0.0 } else { (55.0 + 40.0 * v as f32) / 255.0 };
        [to_f(r), to_f(g), to_f(b)]
    } else {
        // 24 grayscale: 232..=255
        let level = (8 + 10 * (idx - 232) as u16) as f32 / 255.0;
        [level, level, level]
    }
}

fn color_attr_to_rgba(attr: &ColorAttribute, default: [f32; 4]) -> [f32; 4] {
    match attr {
        ColorAttribute::Default => default,
        ColorAttribute::PaletteIndex(idx) => {
            let [r, g, b] = palette_index_to_rgb(*idx);
            [r, g, b, 1.0]
        }
        ColorAttribute::TrueColorWithPaletteFallback(srgba, _) => {
            [srgba.0, srgba.1, srgba.2, srgba.3]
        }
        ColorAttribute::TrueColorWithDefaultFallback(srgba) => {
            [srgba.0, srgba.1, srgba.2, srgba.3]
        }
    }
}

// ---- Cell Renderer ----

pub struct CellRenderer {
    bg_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    _bg_bind_group_layout: wgpu::BindGroupLayout,
    _glyph_bind_group_layout: wgpu::BindGroupLayout,
    bg_bind_group: wgpu::BindGroup,
    glyph_bind_group: wgpu::BindGroup,
    bg_instance_buffer: wgpu::Buffer,
    glyph_instance_buffer: wgpu::Buffer,
    bg_instance_count: u32,
    glyph_instance_count: u32,
    max_instances: usize,
    pub font_config: FontConfig,
    pub atlas: GlyphAtlas,
}

impl CellRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        font_size: f32,
    ) -> Self {
        let font_config = FontConfig::new(font_size);
        let atlas = GlyphAtlas::new(device);

        // Max instances for an 80x24 grid (with room for larger)
        let max_instances = 300 * 100;

        // Uniform buffer
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cell_uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Instance buffers
        let bg_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg_instances"),
            size: (max_instances * std::mem::size_of::<BgInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let glyph_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glyph_instances"),
            size: (max_instances * std::mem::size_of::<GlyphInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Background bind group layout (uniforms only)
        let bg_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Glyph bind group layout (uniforms + texture + sampler)
        let glyph_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("glyph_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // Create bind groups
        let bg_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg_bind_group"),
            layout: &bg_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let glyph_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyph_bind_group"),
            layout: &glyph_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });

        // Background pipeline
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg_shader"),
            source: wgpu::ShaderSource::Wgsl(BG_SHADER.into()),
        });

        let bg_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg_pipeline_layout"),
            bind_group_layouts: &[&bg_bind_group_layout],
            push_constant_ranges: &[],
        });

        let bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg_pipeline"),
            layout: Some(&bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bg_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<BgInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bg_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Glyph pipeline
        let glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(GLYPH_SHADER.into()),
        });

        let glyph_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("glyph_pipeline_layout"),
                bind_group_layouts: &[&glyph_bind_group_layout],
                push_constant_ranges: &[],
            });

        let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&glyph_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &glyph_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GlyphInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[
                        // pos
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        // uv_offset
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        // uv_size
                        wgpu::VertexAttribute {
                            offset: 16,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        // fg_color
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 3,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        // glyph_offset
                        wgpu::VertexAttribute {
                            offset: 40,
                            shader_location: 4,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        // glyph_size
                        wgpu::VertexAttribute {
                            offset: 48,
                            shader_location: 5,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &glyph_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Write initial uniforms
        let uniforms = Uniforms {
            cell_size: [font_config.metrics.cell_width, font_config.metrics.cell_height],
            grid_offset: [4.0, 4.0],
            viewport_size: [1280.0, 720.0],
            _padding: [0.0; 2],
        };
        queue.write_buffer(&uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        Self {
            bg_pipeline,
            glyph_pipeline,
            uniform_buffer,
            _bg_bind_group_layout: bg_bind_group_layout,
            _glyph_bind_group_layout: glyph_bind_group_layout,
            bg_bind_group,
            glyph_bind_group,
            bg_instance_buffer,
            glyph_instance_buffer,
            bg_instance_count: 0,
            glyph_instance_count: 0,
            max_instances,
            font_config,
            atlas,
        }
    }

    /// Update uniforms when viewport is resized.
    pub fn resize(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        let uniforms = Uniforms {
            cell_size: [
                self.font_config.metrics.cell_width,
                self.font_config.metrics.cell_height,
            ],
            grid_offset: [4.0, 4.0],
            viewport_size: [width as f32, height as f32],
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Build instance data from the terminal surface.
    pub fn prepare(&mut self, surface: &Surface, queue: &wgpu::Queue) {
        let (cols, rows) = surface.dimensions();
        let lines = surface.screen_lines();

        let mut bg_instances: Vec<BgInstance> = Vec::with_capacity(cols * rows);
        let mut glyph_instances: Vec<GlyphInstance> = Vec::with_capacity(cols * rows);

        for (row_idx, line) in lines.iter().enumerate() {
            if row_idx >= rows {
                break;
            }
            for cell_ref in line.visible_cells() {
                let col_idx = cell_ref.cell_index();
                if col_idx >= cols {
                    break;
                }

                let attrs = cell_ref.attrs();
                let bg_color = color_attr_to_rgba(&attrs.background(), DEFAULT_BG);
                let fg_color = color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG);

                // Background instance for every cell
                bg_instances.push(BgInstance {
                    pos: [col_idx as f32, row_idx as f32],
                    bg_color,
                });

                // Glyph instance only for non-empty cells
                let text = cell_ref.str();
                if text.is_empty() || text == " " {
                    continue;
                }

                let ch = text.chars().next().unwrap();
                let bold = attrs.intensity() == termwiz::cell::Intensity::Bold;
                let italic = attrs.italic();

                let key = GlyphKey { ch, bold, italic };

                if let Some(entry) = self.atlas.get_or_insert(key, &mut self.font_config, queue) {
                    if entry.width > 0.0 && entry.height > 0.0 {
                        glyph_instances.push(GlyphInstance {
                            pos: [col_idx as f32, row_idx as f32],
                            uv_offset: [entry.uv_x, entry.uv_y],
                            uv_size: [entry.uv_w, entry.uv_h],
                            fg_color,
                            glyph_offset: [entry.offset_x, entry.offset_y],
                            glyph_size: [entry.width, entry.height],
                        });
                    }
                }
            }
        }

        // Clamp to max
        let bg_count = bg_instances.len().min(self.max_instances);
        let glyph_count = glyph_instances.len().min(self.max_instances);

        if bg_count > 0 {
            queue.write_buffer(
                &self.bg_instance_buffer,
                0,
                bytemuck::cast_slice(&bg_instances[..bg_count]),
            );
        }
        if glyph_count > 0 {
            queue.write_buffer(
                &self.glyph_instance_buffer,
                0,
                bytemuck::cast_slice(&glyph_instances[..glyph_count]),
            );
        }

        self.bg_instance_count = bg_count as u32;
        self.glyph_instance_count = glyph_count as u32;
    }

    /// Record render commands into the given encoder.
    pub fn render<'a>(&'a self, render_pass: &mut wgpu::RenderPass<'a>) {
        // Pass 1: backgrounds
        if self.bg_instance_count > 0 {
            render_pass.set_pipeline(&self.bg_pipeline);
            render_pass.set_bind_group(0, &self.bg_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.bg_instance_buffer.slice(..));
            render_pass.draw(0..6, 0..self.bg_instance_count);
        }

        // Pass 2: glyphs
        if self.glyph_instance_count > 0 {
            render_pass.set_pipeline(&self.glyph_pipeline);
            render_pass.set_bind_group(0, &self.glyph_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.glyph_instance_buffer.slice(..));
            render_pass.draw(0..6, 0..self.glyph_instance_count);
        }
    }

    /// Compute terminal grid size from pixel dimensions.
    pub fn grid_size(&self, width: u32, height: u32) -> (usize, usize) {
        let padding = 8.0; // 4px each side
        let cell_w = self.font_config.metrics.cell_width.max(1.0);
        let cell_h = self.font_config.metrics.cell_height.max(1.0);
        let cols = ((width as f32 - padding) / cell_w).floor() as usize;
        let rows = ((height as f32 - padding) / cell_h).floor() as usize;
        (cols.max(1), rows.max(1))
    }

    /// Compute terminal grid size from a viewport rect (physical pixels).
    pub fn grid_size_for_rect(&self, rect: &Rect) -> (usize, usize) {
        let padding = 8.0;
        let cell_w = self.font_config.metrics.cell_width.max(1.0);
        let cell_h = self.font_config.metrics.cell_height.max(1.0);
        let cols = ((rect.width - padding) / cell_w).floor() as usize;
        let rows = ((rect.height - padding) / cell_h).floor() as usize;
        (cols.max(1), rows.max(1))
    }

    /// Prepare instance data for a terminal surface to be rendered in a specific viewport rect.
    /// The viewport rect is in physical pixels. The `screen_width`/`screen_height` are full screen dimensions.
    pub fn prepare_viewport(
        &mut self,
        surface: &Surface,
        queue: &wgpu::Queue,
        viewport: &Rect,
        screen_width: u32,
        screen_height: u32,
    ) {
        // Update uniforms with the viewport's grid offset
        let uniforms = Uniforms {
            cell_size: [
                self.font_config.metrics.cell_width,
                self.font_config.metrics.cell_height,
            ],
            grid_offset: [viewport.x + 4.0, viewport.y + 4.0],
            viewport_size: [screen_width as f32, screen_height as f32],
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        // Build instance data using the standard prepare logic
        self.prepare(surface, queue);
    }

    /// Render with a scissor rect applied. The rect is in physical pixels.
    pub fn render_scissored<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        viewport: &Rect,
        surface_width: u32,
        surface_height: u32,
    ) {
        // Clamp scissor rect to the render target bounds
        let x = (viewport.x.max(0.0) as u32).min(surface_width.saturating_sub(1));
        let y = (viewport.y.max(0.0) as u32).min(surface_height.saturating_sub(1));
        let max_w = surface_width.saturating_sub(x);
        let max_h = surface_height.saturating_sub(y);
        let w = (viewport.width.max(1.0) as u32).min(max_w).max(1);
        let h = (viewport.height.max(1.0) as u32).min(max_h).max(1);
        render_pass.set_scissor_rect(x, y, w, h);

        // Pass 1: backgrounds
        if self.bg_instance_count > 0 {
            render_pass.set_pipeline(&self.bg_pipeline);
            render_pass.set_bind_group(0, &self.bg_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.bg_instance_buffer.slice(..));
            render_pass.draw(0..6, 0..self.bg_instance_count);
        }

        // Pass 2: glyphs
        if self.glyph_instance_count > 0 {
            render_pass.set_pipeline(&self.glyph_pipeline);
            render_pass.set_bind_group(0, &self.glyph_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.glyph_instance_buffer.slice(..));
            render_pass.draw(0..6, 0..self.glyph_instance_count);
        }
    }

    /// Get cell width in pixels.
    pub fn cell_width(&self) -> f32 {
        self.font_config.metrics.cell_width
    }

    /// Get cell height in pixels.
    pub fn cell_height(&self) -> f32 {
        self.font_config.metrics.cell_height
    }
}
