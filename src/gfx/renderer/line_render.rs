use tasty_terminal::ScrollbackLine;
use tasty_type_appearance::color::{GpuRgb, GpuRgba};
use termwiz::cell::CellAttributes;

use crate::font::GlyphKey;
use crate::selection::{NormalizedSelection, SelectionPoint};
use crate::terminal_link::LinkHighlight;

use super::types::{BgInstance, GlyphInstance};
use super::{CellRenderer, unicode_width};
use crate::cell_palette::compute_cell_colors;

impl CellRenderer {
    /// Render a single cell into instance buffers (shared logic for both line types).
    #[allow(clippy::too_many_arguments)]
    fn render_cell(
        &mut self,
        col_idx: usize,
        row_idx: usize,
        text: &str,
        attrs: &CellAttributes,
        width: usize,
        cols: usize,
        default_bg: GpuRgba,
        default_fg: GpuRgba,
        ansi: &[GpuRgb; 16],
        queue: &wgpu::Queue,
        selection: Option<&(NormalizedSelection, GpuRgba)>,
        vi_cursor: Option<&(SelectionPoint, GpuRgba)>,
        absolute_row: usize,
        link: Option<&LinkHighlight>,
        search: Option<&super::SearchHighlights<'_>>,
    ) {
        let (mut bg_color, mut fg_color) = compute_cell_colors(attrs, default_bg, default_fg, ansi);

        // Selection: override bg color
        if let Some((sel, sel_bg)) = selection
            && crate::selection::is_selected(col_idx, absolute_row, sel)
        {
            bg_color = *sel_bg;
        }
        // vi copy mode cursor cell: selection 보다 우선하여 cursor 위치를 강조.
        if let Some((pt, cursor_bg)) = vi_cursor
            && pt.col == col_idx
            && pt.absolute_row == absolute_row
        {
            bg_color = *cursor_bg;
        }
        // Link highlight: override both bg and fg for hovered link spans
        if let Some(link) = link
            && link.covers(col_idx, absolute_row)
        {
            bg_color = link.bg;
            fg_color = link.fg;
        }
        // Search match highlight
        if let Some(sh) = search {
            for (i, m) in sh.matches.iter().enumerate() {
                if m.row == absolute_row && col_idx >= m.col_start && col_idx < m.col_end {
                    bg_color = if i == sh.active_index {
                        sh.active_bg
                    } else {
                        sh.inactive_bg
                    };
                    break;
                }
            }
        }

        let off = self.current_viewport_offset;

        // Push bg for main cell and continuation cells of wide characters
        for i in 0..width {
            if col_idx + i < cols {
                self.bg_instances.push(BgInstance {
                    pos: [(col_idx + i) as f32, row_idx as f32],
                    viewport_offset: off,
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

        if let Some(entry) = self.atlas.get_or_insert(key, &mut self.font_config, queue)
            && entry.width > 0.0
            && entry.height > 0.0
        {
            self.glyph_instances.push(GlyphInstance {
                pos: [col_idx as f32, row_idx as f32],
                viewport_offset: off,
                uv_offset: [entry.uv_x, entry.uv_y],
                uv_size: [entry.uv_w, entry.uv_h],
                fg_color,
                glyph_offset: [entry.offset_x, entry.offset_y],
                glyph_size: [entry.width, entry.height],
                page: entry.page,
                _pad: 0,
            });
        }
    }

    /// Render a single scrollback line (compact `ScrollbackLine`).
    /// Cells are streamed via the borrowing `cells()` iterator — no per-cell
    /// allocation. Fills remaining columns with default_bg.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_scrollback_line(
        &mut self,
        line: &ScrollbackLine,
        row_idx: usize,
        cols: usize,
        default_bg: GpuRgba,
        default_fg: GpuRgba,
        ansi: &[GpuRgb; 16],
        queue: &wgpu::Queue,
        selection: Option<&(NormalizedSelection, GpuRgba)>,
        vi_cursor: Option<&(SelectionPoint, GpuRgba)>,
        absolute_row: usize,
        link: Option<&LinkHighlight>,
        search: Option<&super::SearchHighlights<'_>>,
    ) {
        let mut col_idx: usize = 0;
        for (text, attrs) in line.cells() {
            if col_idx >= cols {
                break;
            }
            let ch = text.chars().next().unwrap_or(' ');
            let width = unicode_width(ch);
            self.render_cell(
                col_idx,
                row_idx,
                text,
                attrs,
                width,
                cols,
                default_bg,
                default_fg,
                ansi,
                queue,
                selection,
                vi_cursor,
                absolute_row,
                link,
                search,
            );
            col_idx += width;
        }
        // Fill remaining columns with default_bg (vi cursor 가 trailing 영역에 있으면 강조).
        let off = self.current_viewport_offset;
        for c in col_idx..cols {
            let bg_color = vi_cursor
                .filter(|(pt, _)| pt.col == c && pt.absolute_row == absolute_row)
                .map(|(_, cursor_bg)| *cursor_bg)
                .unwrap_or(default_bg);
            self.bg_instances.push(BgInstance {
                pos: [c as f32, row_idx as f32],
                viewport_offset: off,
                bg_color,
            });
        }
    }

    /// Render a single surface line (from termwiz screen_lines).
    /// Fills remaining columns with default_bg.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_surface_line(
        &mut self,
        line: &termwiz::surface::line::Line,
        row_idx: usize,
        cols: usize,
        default_bg: GpuRgba,
        default_fg: GpuRgba,
        ansi: &[GpuRgb; 16],
        queue: &wgpu::Queue,
        selection: Option<&(NormalizedSelection, GpuRgba)>,
        vi_cursor: Option<&(SelectionPoint, GpuRgba)>,
        absolute_row: usize,
        link: Option<&LinkHighlight>,
        search: Option<&super::SearchHighlights<'_>>,
    ) {
        let mut last_col = 0usize;
        for cell_ref in line.visible_cells() {
            let col_idx = cell_ref.cell_index();
            if col_idx >= cols {
                break;
            }
            let text = cell_ref.str();
            let ch = text.chars().next().unwrap_or(' ');
            let width = if !text.is_empty() {
                unicode_width(ch)
            } else {
                1
            };
            self.render_cell(
                col_idx,
                row_idx,
                text,
                cell_ref.attrs(),
                width,
                cols,
                default_bg,
                default_fg,
                ansi,
                queue,
                selection,
                vi_cursor,
                absolute_row,
                link,
                search,
            );
            last_col = col_idx + width;
        }
        // Fill remaining columns with default_bg (vi cursor 가 trailing 영역에 있으면 강조).
        let off = self.current_viewport_offset;
        for c in last_col..cols {
            let bg_color = vi_cursor
                .filter(|(pt, _)| pt.col == c && pt.absolute_row == absolute_row)
                .map(|(_, cursor_bg)| *cursor_bg)
                .unwrap_or(default_bg);
            self.bg_instances.push(BgInstance {
                pos: [c as f32, row_idx as f32],
                viewport_offset: off,
                bg_color,
            });
        }
    }
}
