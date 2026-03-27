use termwiz::cell::CellAttributes;
use termwiz::surface::Surface;

use crate::font::{FontConfig, GlyphAtlas, GlyphKey};
use crate::model::Rect;

mod palette;
mod shaders;
mod types;

pub use palette::DEFAULT_BG;
use palette::{color_attr_to_rgba, DEFAULT_FG};
use shaders::{BG_SHADER, GLYPH_SHADER};
use types::{BgInstance, GlyphInstance, Uniforms};

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
    /// Reusable buffer to avoid per-frame allocation.
    bg_instances: Vec<BgInstance>,
    /// Reusable buffer to avoid per-frame allocation.
    glyph_instances: Vec<GlyphInstance>,
}

impl CellRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        font_size: f32,
        font_family: &str,
    ) -> Self {
        let font_config = FontConfig::new(font_size, font_family);
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
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 8,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 16,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 3,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 40,
                            shader_location: 4,
                            format: wgpu::VertexFormat::Float32x2,
                        },
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
            bg_instances: Vec::with_capacity(300 * 100),
            glyph_instances: Vec::with_capacity(300 * 100),
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

    /// Build instance data from the terminal surface with a custom default background.
    /// If `cursor` is Some((col, row)), that cell's fg/bg are swapped to show the cursor.
    pub fn prepare_with_bg(&mut self, surface: &Surface, queue: &wgpu::Queue, default_bg: [f32; 4], cursor: Option<(usize, usize)>) {
        let (cols, rows) = surface.dimensions();
        let lines = surface.screen_lines();

        self.bg_instances.clear();
        self.glyph_instances.clear();

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
                let is_cursor = cursor == Some((col_idx, row_idx));
                let (bg_color, fg_color) = if is_cursor {
                    let fg = color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG);
                    let bg = color_attr_to_rgba(&attrs.background(), default_bg);
                    (fg, bg)
                } else {
                    (
                        color_attr_to_rgba(&attrs.background(), default_bg),
                        color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG),
                    )
                };

                self.bg_instances.push(BgInstance {
                    pos: [col_idx as f32, row_idx as f32],
                    bg_color,
                });

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
                        self.glyph_instances.push(GlyphInstance {
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

        let bg_count = self.bg_instances.len().min(self.max_instances);
        let glyph_count = self.glyph_instances.len().min(self.max_instances);

        if bg_count > 0 {
            queue.write_buffer(
                &self.bg_instance_buffer,
                0,
                bytemuck::cast_slice(&self.bg_instances[..bg_count]),
            );
        }
        if glyph_count > 0 {
            queue.write_buffer(
                &self.glyph_instance_buffer,
                0,
                bytemuck::cast_slice(&self.glyph_instances[..glyph_count]),
            );
        }

        self.bg_instance_count = bg_count as u32;
        self.glyph_instance_count = glyph_count as u32;
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

    /// Build instance data from the terminal surface (uses default palette bg).
    pub fn prepare(&mut self, surface: &Surface, queue: &wgpu::Queue) {
        self.prepare_with_bg(surface, queue, DEFAULT_BG, None);
    }

    /// Prepare instance data for a terminal surface to be rendered in a specific viewport rect.
    pub fn prepare_viewport(
        &mut self,
        surface: &Surface,
        queue: &wgpu::Queue,
        viewport: &Rect,
        screen_width: u32,
        screen_height: u32,
    ) {
        self.prepare_viewport_with_bg(surface, queue, viewport, screen_width, screen_height, DEFAULT_BG);
    }

    /// Prepare instance data with a custom default background color.
    pub fn prepare_viewport_with_bg(
        &mut self,
        surface: &Surface,
        queue: &wgpu::Queue,
        viewport: &Rect,
        screen_width: u32,
        screen_height: u32,
        default_bg: [f32; 4],
    ) {
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

        self.prepare_with_bg(surface, queue, default_bg, None);
    }

    /// Prepare instance data for a terminal with scrollback support.
    /// When scroll_offset > 0, mixes scrollback lines with surface lines.
    /// If `show_cursor` is true, the cursor cell's fg/bg colors are swapped.
    pub fn prepare_terminal_viewport(
        &mut self,
        terminal: &tasty_terminal::Terminal,
        queue: &wgpu::Queue,
        viewport: &Rect,
        screen_width: u32,
        screen_height: u32,
        default_bg: [f32; 4],
        show_cursor: bool,
    ) {
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

        let cursor = if show_cursor && terminal.cursor_visible() && terminal.scroll_offset == 0 {
            let (cx, cy) = terminal.surface().cursor_position();
            Some((cx, cy as usize))
        } else {
            None
        };

        if terminal.scroll_offset == 0 {
            // No scrollback active - use normal surface rendering
            self.prepare_with_bg(terminal.surface(), queue, default_bg, cursor);
            return;
        }

        // Scrolled back - mix scrollback buffer + surface lines
        let (cols, rows) = terminal.surface().dimensions();
        let scroll_offset = terminal.scroll_offset;
        let scrollback_len = terminal.scrollback_len();
        let surface_lines = terminal.surface().screen_lines();

        self.bg_instances.clear();
        self.glyph_instances.clear();

        for row_idx in 0..rows {
            // Which source line to show at this display row?
            // source_line indexes into: [scrollback_0 .. scrollback_N-1, surface_0 .. surface_M-1]
            let source_line = scrollback_len as isize - scroll_offset as isize + row_idx as isize;

            if source_line < 0 {
                // Before start of scrollback - render empty row with default bg
                for col_idx in 0..cols {
                    self.bg_instances.push(BgInstance {
                        pos: [col_idx as f32, row_idx as f32],
                        bg_color: default_bg,
                    });
                }
                continue;
            }
            let source_line = source_line as usize;

            if source_line < scrollback_len {
                // From scrollback buffer
                if let Some(line) = terminal.scrollback_line(source_line) {
                    self.render_scrollback_line(line, row_idx, cols, default_bg, queue);
                }
            } else {
                // From current surface
                let surface_row = source_line - scrollback_len;
                if surface_row < surface_lines.len() {
                    self.render_surface_line(&surface_lines[surface_row], row_idx, cols, default_bg, queue);
                }
            }
        }

        let bg_count = self.bg_instances.len().min(self.max_instances);
        let glyph_count = self.glyph_instances.len().min(self.max_instances);

        if bg_count > 0 {
            queue.write_buffer(
                &self.bg_instance_buffer,
                0,
                bytemuck::cast_slice(&self.bg_instances[..bg_count]),
            );
        }
        if glyph_count > 0 {
            queue.write_buffer(
                &self.glyph_instance_buffer,
                0,
                bytemuck::cast_slice(&self.glyph_instances[..glyph_count]),
            );
        }

        self.bg_instance_count = bg_count as u32;
        self.glyph_instance_count = glyph_count as u32;
    }

    /// Render a single scrollback line (stored as Vec<(String, CellAttributes)>).
    fn render_scrollback_line(
        &mut self,
        line: &[(String, CellAttributes)],
        row_idx: usize,
        cols: usize,
        default_bg: [f32; 4],
        queue: &wgpu::Queue,
    ) {
        for (col_idx, (text, attrs)) in line.iter().enumerate() {
            if col_idx >= cols {
                break;
            }
            let bg_color = color_attr_to_rgba(&attrs.background(), default_bg);
            let fg_color = color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG);

            self.bg_instances.push(BgInstance {
                pos: [col_idx as f32, row_idx as f32],
                bg_color,
            });

            if text.is_empty() || text == " " {
                continue;
            }

            let ch = match text.chars().next() {
                Some(c) => c,
                None => continue,
            };
            let bold = attrs.intensity() == termwiz::cell::Intensity::Bold;
            let italic = attrs.italic();

            let key = GlyphKey { ch, bold, italic };

            if let Some(entry) = self.atlas.get_or_insert(key, &mut self.font_config, queue) {
                if entry.width > 0.0 && entry.height > 0.0 {
                    self.glyph_instances.push(GlyphInstance {
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

    /// Render a single surface line (from termwiz screen_lines).
    fn render_surface_line(
        &mut self,
        line: &termwiz::surface::line::Line,
        row_idx: usize,
        cols: usize,
        default_bg: [f32; 4],
        queue: &wgpu::Queue,
    ) {
        for cell_ref in line.visible_cells() {
            let col_idx = cell_ref.cell_index();
            if col_idx >= cols {
                break;
            }

            let attrs = cell_ref.attrs();
            let bg_color = color_attr_to_rgba(&attrs.background(), default_bg);
            let fg_color = color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG);

            self.bg_instances.push(BgInstance {
                pos: [col_idx as f32, row_idx as f32],
                bg_color,
            });

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
                    self.glyph_instances.push(GlyphInstance {
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

    /// Render with a scissor rect applied.
    pub fn render_scissored<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        viewport: &Rect,
        surface_width: u32,
        surface_height: u32,
    ) {
        let x = (viewport.x.max(0.0) as u32).min(surface_width.saturating_sub(1));
        let y = (viewport.y.max(0.0) as u32).min(surface_height.saturating_sub(1));
        let max_w = surface_width.saturating_sub(x);
        let max_h = surface_height.saturating_sub(y);
        let w = (viewport.width.max(1.0) as u32).min(max_w).max(1);
        let h = (viewport.height.max(1.0) as u32).min(max_h).max(1);
        render_pass.set_scissor_rect(x, y, w, h);

        if self.bg_instance_count > 0 {
            render_pass.set_pipeline(&self.bg_pipeline);
            render_pass.set_bind_group(0, &self.bg_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.bg_instance_buffer.slice(..));
            render_pass.draw(0..6, 0..self.bg_instance_count);
        }

        if self.glyph_instance_count > 0 {
            render_pass.set_pipeline(&self.glyph_pipeline);
            render_pass.set_bind_group(0, &self.glyph_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.glyph_instance_buffer.slice(..));
            render_pass.draw(0..6, 0..self.glyph_instance_count);
        }
    }

    /// Update font configuration (font size and/or family changed).
    /// Resets the glyph atlas and re-measures cell metrics.
    pub fn update_font(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, font_size: f32, font_family: &str) {
        self.font_config = FontConfig::new(font_size, font_family);
        self.atlas = GlyphAtlas::new(device);

        // Rebuild the glyph bind group with the new atlas texture
        let glyph_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
        self.glyph_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glyph_bind_group"),
            layout: &glyph_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.atlas.sampler),
                },
            ],
        });

        // Update uniform buffer with new cell size
        let uniforms = Uniforms {
            cell_size: [self.font_config.metrics.cell_width, self.font_config.metrics.cell_height],
            grid_offset: [0.0, 0.0],
            viewport_size: [0.0, 0.0], // will be updated on next resize
            _padding: [0.0, 0.0],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
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
