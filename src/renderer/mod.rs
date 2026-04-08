use termwiz::surface::Surface;

use crate::font::{FontConfig, GlyphAtlas, GlyphKey};
use crate::model::Rect;
use crate::selection::NormalizedSelection;

mod line_render;
mod palette;
mod pipeline;
mod shaders;
mod types;

/// Check if a character is a wide (2-cell) character (CJK, fullwidth, etc.)
pub fn unicode_width(ch: char) -> usize {
    // CJK Unified Ideographs, Hangul, Fullwidth forms, etc.
    let cp = ch as u32;
    if (0x1100..=0x115F).contains(&cp)     // Hangul Jamo
        || (0x2E80..=0x303E).contains(&cp) // CJK Radicals, Kangxi, CJK Symbols
        || (0x3040..=0x33BF).contains(&cp) // Hiragana, Katakana, CJK Compat
        || (0x3400..=0x4DBF).contains(&cp) // CJK Extension A
        || (0x4E00..=0x9FFF).contains(&cp) // CJK Unified Ideographs
        || (0xA000..=0xA4CF).contains(&cp) // Yi
        || (0xAC00..=0xD7AF).contains(&cp) // Hangul Syllables
        || (0xF900..=0xFAFF).contains(&cp) // CJK Compat Ideographs
        || (0xFE30..=0xFE4F).contains(&cp) // CJK Compat Forms
        || (0xFF01..=0xFF60).contains(&cp) // Fullwidth Forms
        || (0xFFE0..=0xFFE6).contains(&cp) // Fullwidth Signs
        || (0x20000..=0x2FA1F).contains(&cp) // CJK Extensions B-F, Compat Supplement
        || (0x30000..=0x3134F).contains(&cp) // CJK Extension G
    {
        2
    } else {
        1
    }
}

pub use palette::DEFAULT_BG;
use palette::{color_attr_to_rgba, DEFAULT_FG};
use types::{BgInstance, GlyphInstance, Uniforms};

pub struct RenderPreedit {
    pub text: String,
    #[allow(dead_code)]
    pub cursor: Option<(usize, usize)>,
    pub anchor_col: usize,
    pub anchor_row: usize,
    pub bg_color: [f32; 4],
    pub fg_color: [f32; 4],
}

impl RenderPreedit {
    /// Returns the exclusive end column of the preedit text.
    fn end_col(&self) -> usize {
        let mut col = self.anchor_col;
        for ch in self.text.chars() {
            col += unicode_width(ch);
        }
        col
    }

    /// Check if a cell at (col, row) is covered by the preedit overlay.
    fn covers(&self, col: usize, row: usize) -> bool {
        row == self.anchor_row && col >= self.anchor_col && col < self.end_col()
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
    /// Reusable buffer to avoid per-frame allocation.
    bg_instances: Vec<BgInstance>,
    /// Reusable buffer to avoid per-frame allocation.
    glyph_instances: Vec<GlyphInstance>,
}

impl CellRenderer {
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
    pub fn prepare_with_bg(
        &mut self,
        surface: &Surface,
        queue: &wgpu::Queue,
        default_bg: [f32; 4],
        cursor: Option<(usize, usize, bool)>,
        selection: Option<&(NormalizedSelection, [f32; 4])>,
        row_offset: usize,
        preedit: Option<&RenderPreedit>,
    ) {
        let (cols, rows) = surface.dimensions();
        let lines = surface.screen_lines();

        self.bg_instances.clear();
        self.glyph_instances.clear();

        for (row_idx, line) in lines.iter().enumerate() {
            if row_idx >= rows {
                break;
            }
            let mut last_col = 0usize;
            for cell_ref in line.visible_cells() {
                let col_idx = cell_ref.cell_index();
                if col_idx >= cols {
                    break;
                }

                let attrs = cell_ref.attrs();
                let is_cursor = match cursor {
                    Some((cx, cy, wide)) if row_idx == cy => {
                        col_idx == cx || (wide && col_idx == cx + 1)
                    }
                    _ => false,
                };
                let (mut bg_color, mut fg_color) = (
                    color_attr_to_rgba(&attrs.background(), default_bg),
                    color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG),
                );
                if attrs.reverse() {
                    std::mem::swap(&mut bg_color, &mut fg_color);
                }
                if is_cursor {
                    std::mem::swap(&mut bg_color, &mut fg_color);
                }

                if let Some((sel, sel_bg)) = selection {
                    let abs_row = row_offset + row_idx;
                    if crate::selection::is_selected(col_idx, abs_row, sel) {
                        bg_color = *sel_bg;
                    }
                }

                self.bg_instances.push(BgInstance {
                    pos: [col_idx as f32, row_idx as f32],
                    bg_color,
                });

                let text = cell_ref.str();

                if !text.is_empty() {
                    let ch = text.chars().next().unwrap();
                    if unicode_width(ch) > 1 && col_idx + 1 < cols {
                        self.bg_instances.push(BgInstance {
                            pos: [(col_idx + 1) as f32, row_idx as f32],
                            bg_color,
                        });
                        last_col = col_idx + 2;
                    } else {
                        last_col = col_idx + 1;
                    }
                } else {
                    last_col = col_idx + 1;
                }

                if text.is_empty() || text == " " {
                    continue;
                }

                // Skip glyph rendering for cells covered by IME preedit overlay.
                // The preedit overlay will draw its own glyphs on top.
                if preedit.is_some_and(|p| p.covers(col_idx, row_idx)) {
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
            // Fill remaining columns with default_bg so the focused surface
            // background covers the full row, not just cells with content.
            // Also render cursor if it falls in this trailing region.
            for col_idx in last_col..cols {
                let is_cursor = match cursor {
                    Some((cx, cy, _)) if row_idx == cy && col_idx == cx => true,
                    _ => false,
                };
                let bg = if is_cursor {
                    // Cursor on empty cell: swap fg/bg (block cursor)
                    DEFAULT_FG
                } else {
                    default_bg
                };
                self.bg_instances.push(BgInstance {
                    pos: [col_idx as f32, row_idx as f32],
                    bg_color: bg,
                });
            }
        }

        // Fill the right and bottom gutter (fractional cell area beyond the grid)
        // with default_bg. The extra bg instances extend beyond the grid boundary;
        // the scissor rect clips them to exactly the remaining pixels.
        for row_idx in 0..rows {
            self.bg_instances.push(BgInstance {
                pos: [cols as f32, row_idx as f32],
                bg_color: default_bg,
            });
        }
        for col_idx in 0..=cols {
            self.bg_instances.push(BgInstance {
                pos: [col_idx as f32, rows as f32],
                bg_color: default_bg,
            });
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
        let cell_w = self.font_config.metrics.cell_width.max(1.0);
        let cell_h = self.font_config.metrics.cell_height.max(1.0);
        let cols = (rect.width / cell_w).floor() as usize;
        let rows = (rect.height / cell_h).floor() as usize;
        (cols.max(1), rows.max(1))
    }

    /// Prepare instance data for a terminal with scrollback support.
    pub fn prepare_terminal_viewport(
        &mut self,
        terminal: &tasty_terminal::Terminal,
        queue: &wgpu::Queue,
        viewport: &Rect,
        screen_width: u32,
        screen_height: u32,
        default_bg: [f32; 4],
        show_cursor: bool,
        selection: Option<&(NormalizedSelection, [f32; 4])>,
        preedit: Option<&RenderPreedit>,
    ) {
        let uniforms = Uniforms {
            cell_size: [
                self.font_config.metrics.cell_width,
                self.font_config.metrics.cell_height,
            ],
            grid_offset: [viewport.x, viewport.y],
            viewport_size: [screen_width as f32, screen_height as f32],
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let cursor = if show_cursor && terminal.cursor_visible() && terminal.scroll_offset == 0 {
            let (cx, cy) = terminal.surface().cursor_position();
            let wide = terminal.surface().screen_lines()
                .get(cy as usize)
                .and_then(|line| {
                    line.visible_cells()
                        .find(|cell| cell.cell_index() == cx)
                        .map(|cell| {
                            let ch = cell.str().chars().next().unwrap_or(' ');
                            unicode_width(ch) > 1
                        })
                })
                .unwrap_or(false);
            Some((cx, cy as usize, wide))
        } else {
            None
        };

        let (cols, rows) = terminal.surface().dimensions();

        if terminal.scroll_offset == 0 {
            let row_offset = terminal.scrollback_len();
            self.prepare_with_bg(terminal.surface(), queue, default_bg, cursor, selection, row_offset, preedit);
            self.append_preedit_overlay(preedit, queue, cols, rows);
            return;
        }

        // Scrolled back - mix scrollback buffer + surface lines
        let scroll_offset = terminal.scroll_offset;
        let scrollback_len = terminal.scrollback_len();
        let surface_lines = terminal.surface().screen_lines();

        self.bg_instances.clear();
        self.glyph_instances.clear();

        for row_idx in 0..rows {
            let source_line = scrollback_len as isize - scroll_offset as isize + row_idx as isize;

            if source_line < 0 {
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
                if let Some(line) = terminal.scrollback_line(source_line) {
                    self.render_scrollback_line(line, row_idx, cols, default_bg, queue, selection, source_line);
                }
            } else {
                let surface_row = source_line - scrollback_len;
                if surface_row < surface_lines.len() {
                    self.render_surface_line(&surface_lines[surface_row], row_idx, cols, default_bg, queue, selection, source_line);
                }
            }
        }

        // Fill right and bottom gutter (same as prepare_with_bg)
        for row_idx in 0..rows {
            self.bg_instances.push(BgInstance {
                pos: [cols as f32, row_idx as f32],
                bg_color: default_bg,
            });
        }
        for col_idx in 0..=cols {
            self.bg_instances.push(BgInstance {
                pos: [col_idx as f32, rows as f32],
                bg_color: default_bg,
            });
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
        self.append_preedit_overlay(preedit, queue, cols, rows);
    }

    fn append_preedit_overlay(
        &mut self,
        preedit: Option<&RenderPreedit>,
        queue: &wgpu::Queue,
        cols: usize,
        rows: usize,
    ) {
        let Some(preedit) = preedit else {
            return;
        };
        if preedit.text.is_empty() || preedit.anchor_row >= rows || preedit.anchor_col >= cols {
            return;
        }

        let mut col_idx = preedit.anchor_col;
        for ch in preedit.text.chars() {
            if col_idx >= cols {
                break;
            }

            let width = unicode_width(ch);
            for i in 0..width {
                if col_idx + i < cols {
                    self.bg_instances.push(BgInstance {
                        pos: [(col_idx + i) as f32, preedit.anchor_row as f32],
                        bg_color: preedit.bg_color,
                    });
                }
            }

            let key = GlyphKey {
                ch,
                bold: false,
                italic: false,
            };
            if let Some(entry) = self.atlas.get_or_insert(key, &mut self.font_config, queue) {
                if entry.width > 0.0 && entry.height > 0.0 {
                    self.glyph_instances.push(GlyphInstance {
                        pos: [col_idx as f32, preedit.anchor_row as f32],
                        uv_offset: [entry.uv_x, entry.uv_y],
                        uv_size: [entry.uv_w, entry.uv_h],
                        fg_color: preedit.fg_color,
                        glyph_offset: [entry.offset_x, entry.offset_y],
                        glyph_size: [entry.width, entry.height],
                    });
                }
            }

            col_idx += width;
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

    /// Get cell width in pixels.
    pub fn cell_width(&self) -> f32 {
        self.font_config.metrics.cell_width
    }

    /// Get cell height in pixels.
    pub fn cell_height(&self) -> f32 {
        self.font_config.metrics.cell_height
    }
}
