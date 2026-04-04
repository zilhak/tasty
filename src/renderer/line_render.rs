use termwiz::cell::CellAttributes;

use crate::font::GlyphKey;
use crate::selection::NormalizedSelection;

use super::{CellRenderer, unicode_width};
use super::palette::{color_attr_to_rgba, DEFAULT_FG};
use super::types::{BgInstance, GlyphInstance};

impl CellRenderer {
    /// Render a single cell into instance buffers (shared logic for both line types).
    fn render_cell(
        &mut self,
        col_idx: usize,
        row_idx: usize,
        text: &str,
        attrs: &CellAttributes,
        width: usize,
        cols: usize,
        default_bg: [f32; 4],
        queue: &wgpu::Queue,
        selection: Option<&(NormalizedSelection, [f32; 4])>,
        absolute_row: usize,
    ) {
        let (mut bg_color, mut fg_color) = (
            color_attr_to_rgba(&attrs.background(), default_bg),
            color_attr_to_rgba(&attrs.foreground(), DEFAULT_FG),
        );
        if attrs.reverse() {
            std::mem::swap(&mut bg_color, &mut fg_color);
        }

        // Selection: override bg color
        if let Some((sel, sel_bg)) = selection {
            if crate::selection::is_selected(col_idx, absolute_row, sel) {
                bg_color = *sel_bg;
            }
        }

        // Push bg for main cell and continuation cells of wide characters
        for i in 0..width {
            if col_idx + i < cols {
                self.bg_instances.push(BgInstance {
                    pos: [(col_idx + i) as f32, row_idx as f32],
                    bg_color,
                });
            }
        }

        if text.is_empty() || text == " " {
            return;
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

    /// Render a single scrollback line (stored as Vec<(String, CellAttributes)>).
    pub(super) fn render_scrollback_line(
        &mut self,
        line: &[(String, CellAttributes)],
        row_idx: usize,
        cols: usize,
        default_bg: [f32; 4],
        queue: &wgpu::Queue,
        selection: Option<&(NormalizedSelection, [f32; 4])>,
        absolute_row: usize,
    ) {
        let mut col_idx: usize = 0;
        for (text, attrs) in line.iter() {
            if col_idx >= cols {
                break;
            }
            let ch = text.chars().next().unwrap_or(' ');
            let width = unicode_width(ch);
            self.render_cell(col_idx, row_idx, text.as_str(), attrs, width, cols, default_bg, queue, selection, absolute_row);
            col_idx += width;
        }
    }

    /// Render a single surface line (from termwiz screen_lines).
    pub(super) fn render_surface_line(
        &mut self,
        line: &termwiz::surface::line::Line,
        row_idx: usize,
        cols: usize,
        default_bg: [f32; 4],
        queue: &wgpu::Queue,
        selection: Option<&(NormalizedSelection, [f32; 4])>,
        absolute_row: usize,
    ) {
        for cell_ref in line.visible_cells() {
            let col_idx = cell_ref.cell_index();
            if col_idx >= cols {
                break;
            }
            let text = cell_ref.str();
            let ch = text.chars().next().unwrap_or(' ');
            let width = if !text.is_empty() { unicode_width(ch) } else { 1 };
            self.render_cell(col_idx, row_idx, text, cell_ref.attrs(), width, cols, default_bg, queue, selection, absolute_row);
        }
    }
}
