mod line_render;
mod pipeline;
mod shaders;
mod types;

use std::cell::Cell;
use std::ops::Range;

use tasty_type_appearance::color::{GpuRgb, GpuRgba};
use termwiz::surface::Surface;

use crate::font::{FontConfig, GlyphAtlas, GlyphKey};
use crate::model::PhysicalRect;
use crate::selection::{NormalizedSelection, SelectionPoint};
use crate::terminal_link::LinkHighlight;

/// Search match highlights to pass into the renderer.
pub struct SearchHighlights<'a> {
    pub matches: &'a [tasty_terminal::search::SearchMatch],
    pub active_index: usize,
    pub inactive_bg: GpuRgba,
    pub active_bg: GpuRgba,
}

/// How the text cursor cell is painted, derived from DECSCUSR (`cursor_shape()`).
/// Blink variants collapse to their steady shape — terminal content does not
/// animate (theme policy), so the cursor is drawn statically.
#[derive(Copy, Clone, PartialEq, Eq)]
enum CursorPaint {
    /// Full-cell reverse (default fg/bg swap).
    Block,
    /// Thin vertical bar at the cell's left edge.
    Bar,
    /// Thin horizontal bar at the cell's bottom.
    Underline,
}

impl CursorPaint {
    fn from_shape(shape: tasty_terminal::CursorShape) -> Self {
        use tasty_terminal::CursorShape as S;
        match shape {
            S::Default | S::SteadyBlock | S::BlinkingBlock => CursorPaint::Block,
            S::SteadyUnderline | S::BlinkingUnderline => CursorPaint::Underline,
            S::SteadyBar | S::BlinkingBar => CursorPaint::Bar,
        }
    }

    /// Block-element glyph that draws this shape as a sub-cell overlay, or `None`
    /// for Block (which is drawn by the fg/bg swap, not a glyph).
    fn overlay_glyph(self) -> Option<char> {
        match self {
            CursorPaint::Block => None,
            CursorPaint::Bar => Some('\u{258f}'), // ▏ LEFT ONE EIGHTH BLOCK
            CursorPaint::Underline => Some('\u{2581}'), // ▁ LOWER ONE EIGHTH BLOCK
        }
    }
}

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
        || (0x30000..=0x3134F).contains(&cp)
    // CJK Extension G
    {
        2
    } else {
        1
    }
}

#[cfg(debug_assertions)]
pub use crate::cell_palette::compute_cell_colors as resolve_cell_colors;
use crate::cell_palette::compute_cell_colors;
use types::{BgInstance, GlyphInstance, Uniforms};

pub struct RenderPreedit {
    pub text: String,
    pub anchor_col: usize,
    pub anchor_row: usize,
    pub bg_color: GpuRgba,
    pub fg_color: GpuRgba,
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
    /// Current GPU buffer capacity in instances. Grows dynamically when
    /// the accumulated frame exceeds it (R1: avoid silent clamp regression).
    max_instances: usize,
    pub font_config: FontConfig,
    pub atlas: GlyphAtlas,
    /// Per-frame accumulator (cleared on `begin_frame`).
    pub(crate) bg_instances: Vec<BgInstance>,
    pub(crate) glyph_instances: Vec<GlyphInstance>,
    /// Per-surface instance ranges recorded during accumulation:
    /// (scissor rect, bg range, glyph range).
    surface_ranges: Vec<(PhysicalRect, Range<u32>, Range<u32>)>,
    /// viewport_offset baked into instances pushed during the current
    /// `append_terminal_viewport` call. Set at the start of accumulation
    /// for a surface and read by per-cell push helpers.
    pub(crate) current_viewport_offset: [f32; 2],
    /// Per-frame draw call counters (set inside `render_all`). `Cell` 으로
    /// interior mutability 를 부여해 `&self` 시그니처를 유지한 채
    /// `wgpu::RenderPass<'a>` 와 묶인 lifetime 충돌을 회피한다.
    last_frame_bg_draws: Cell<u32>,
    last_frame_glyph_draws: Cell<u32>,
}

impl CellRenderer {
    /// Update viewport-size uniforms when the window resizes. Per-surface
    /// offset lives on each instance, so we only refresh the global size here.
    pub fn resize(&self, queue: &wgpu::Queue, width: u32, height: u32) {
        let uniforms = Uniforms {
            cell_size: [
                self.font_config.metrics.cell_width,
                self.font_config.metrics.cell_height,
            ],
            viewport_size: [width as f32, height as f32],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    /// Reset per-frame accumulators. Call once at the start of `render_terminals`.
    /// Bumps the atlas frame counter so per-page LRU stamps stay coherent.
    pub fn begin_frame(&mut self) {
        self.bg_instances.clear();
        self.glyph_instances.clear();
        self.surface_ranges.clear();
        self.last_frame_bg_draws.set(0);
        self.last_frame_glyph_draws.set(0);
        self.atlas.begin_frame();
    }

    /// Append a terminal viewport's instances to the frame accumulator.
    /// `ansi` is hoisted to the caller so the theme lock is taken once per
    /// frame, not per surface.
    #[allow(clippy::too_many_arguments)]
    pub fn append_terminal_viewport(
        &mut self,
        terminal: &tasty_terminal::Terminal,
        queue: &wgpu::Queue,
        viewport: &PhysicalRect,
        ansi: &[GpuRgb; 16],
        default_bg: GpuRgba,
        default_fg: GpuRgba,
        show_cursor: bool,
        selection: Option<&(NormalizedSelection, GpuRgba)>,
        vi_cursor: Option<&(SelectionPoint, GpuRgba)>,
        preedit: Option<&RenderPreedit>,
        link: Option<&LinkHighlight>,
        search: Option<&SearchHighlights<'_>>,
        // DECSCNM 렌더 허용 여부(설정 `general.reverse_screen_enabled`). false 면
        // 터미널이 reverse-screen 모드여도 화면 반전을 적용하지 않는다(플래시 억제).
        reverse_screen_enabled: bool,
    ) {
        let bg_start = self.bg_instances.len() as u32;
        let glyph_start = self.glyph_instances.len() as u32;
        self.current_viewport_offset = [viewport.x.value(), viewport.y.value()];

        // Lock the shared terminal state once for the whole viewport render
        // (surface + scrollback + cursor/modes). The parser thread's per-chunk
        // lock window is the only contention; a visible terminal is idle enough
        // that this is uncontended in practice (ADR-0002).
        terminal.with_render_view(|view| {
            // DECSCNM (reverse screen): swap the default fg/bg for the whole
            // viewport. Cell-level attributes are unaffected; only the default
            // (unstyled) colors invert. Gated by `reverse_screen_enabled`: when
            // the user turns the setting off, the mode flag stays tracked (so
            // program queries still answer correctly) but we skip the visual
            // swap, suppressing the full-screen flash some shells emit as a
            // visible bell.
            let (default_bg, default_fg) = if reverse_screen_enabled && view.screen_reverse() {
                (default_fg, default_bg)
            } else {
                (default_bg, default_fg)
            };
            let cursor = if show_cursor && view.cursor_visible() && view.scroll_offset() == 0 {
                let (cx, cy) = view.surface().cursor_position();
                let wide = view
                    .surface()
                    .screen_lines()
                    .get(cy)
                    .and_then(|line| {
                        line.visible_cells()
                            .find(|cell| cell.cell_index() == cx)
                            .map(|cell| {
                                let ch = cell.str().chars().next().unwrap_or(' ');
                                unicode_width(ch) > 1
                            })
                    })
                    .unwrap_or(false);
                Some((cx, cy, wide, CursorPaint::from_shape(view.cursor_shape())))
            } else {
                None
            };

            let (cols, rows) = view.dimensions();

            if view.scroll_offset() == 0 {
                let row_offset = view.scrollback_len();
                self.fill_surface(
                    view.surface(),
                    queue,
                    default_bg,
                    default_fg,
                    ansi,
                    cursor,
                    selection,
                    vi_cursor,
                    row_offset,
                    preedit,
                    link,
                    search,
                );
                self.append_preedit_overlay(preedit, queue, cols, rows, 0);
            } else {
                let scroll_offset = view.scroll_offset();
                let scrollback_len = view.scrollback_len();
                let surface_lines = view.surface().screen_lines();

                for row_idx in 0..rows {
                    let source_line =
                        scrollback_len as isize - scroll_offset as isize + row_idx as isize;

                    if source_line < 0 {
                        let off = self.current_viewport_offset;
                        for col_idx in 0..cols {
                            self.bg_instances.push(BgInstance {
                                pos: [col_idx as f32, row_idx as f32],
                                viewport_offset: off,
                                bg_color: default_bg,
                            });
                        }
                        continue;
                    }
                    let source_line = source_line as usize;

                    if source_line < scrollback_len {
                        if let Some(line) = view.scrollback_line(source_line) {
                            self.render_scrollback_line(
                                line,
                                row_idx,
                                cols,
                                default_bg,
                                default_fg,
                                ansi,
                                queue,
                                selection,
                                vi_cursor,
                                source_line,
                                link,
                                search,
                            );
                        }
                    } else {
                        let surface_row = source_line - scrollback_len;
                        if surface_row < surface_lines.len() {
                            self.render_surface_line(
                                &surface_lines[surface_row],
                                row_idx,
                                cols,
                                default_bg,
                                default_fg,
                                ansi,
                                queue,
                                selection,
                                vi_cursor,
                                source_line,
                                link,
                                search,
                            );
                        }
                    }
                }

                // Right + bottom gutter.
                let off = self.current_viewport_offset;
                for row_idx in 0..rows {
                    self.bg_instances.push(BgInstance {
                        pos: [cols as f32, row_idx as f32],
                        viewport_offset: off,
                        bg_color: default_bg,
                    });
                }
                for col_idx in 0..=cols {
                    self.bg_instances.push(BgInstance {
                        pos: [col_idx as f32, rows as f32],
                        viewport_offset: off,
                        bg_color: default_bg,
                    });
                }

                self.append_preedit_overlay(preedit, queue, cols, rows, scroll_offset);
            }
        });

        let bg_range = bg_start..self.bg_instances.len() as u32;
        let glyph_range = glyph_start..self.glyph_instances.len() as u32;
        self.surface_ranges.push((*viewport, bg_range, glyph_range));
    }

    /// Append instances for the current-screen path (no scrollback).
    /// Equivalent to the previous `prepare_with_bg`, but operates in append mode.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: 리팩터 후보 — GPU 셀 인스턴스 append(스타일/커서/셀 분기 밀집). hot path 라 분해 신중
    fn fill_surface(
        &mut self,
        surface: &Surface,
        queue: &wgpu::Queue,
        default_bg: GpuRgba,
        default_fg: GpuRgba,
        ansi: &[GpuRgb; 16],
        cursor: Option<(usize, usize, bool, CursorPaint)>,
        selection: Option<&(NormalizedSelection, GpuRgba)>,
        vi_cursor: Option<&(SelectionPoint, GpuRgba)>,
        row_offset: usize,
        preedit: Option<&RenderPreedit>,
        link: Option<&LinkHighlight>,
        search: Option<&SearchHighlights<'_>>,
    ) {
        let (cols, rows) = surface.dimensions();
        let lines = surface.screen_lines();
        let off = self.current_viewport_offset;

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
                // Cursor paint for this cell, if any. Only the Block shape swaps
                // fg/bg; Bar/Underline render the cell normally and get a sub-cell
                // overlay glyph after the loop.
                let cursor_paint = match cursor {
                    Some((cx, cy, wide, shape))
                        if row_idx == cy && (col_idx == cx || (wide && col_idx == cx + 1)) =>
                    {
                        Some(shape)
                    }
                    _ => None,
                };
                let (mut bg_color, mut fg_color) =
                    compute_cell_colors(attrs, default_bg, default_fg, ansi);
                if cursor_paint == Some(CursorPaint::Block) {
                    std::mem::swap(&mut bg_color, &mut fg_color);
                }

                let abs_row = row_offset + row_idx;
                if let Some((sel, sel_bg)) = selection
                    && crate::selection::is_selected(col_idx, abs_row, sel)
                {
                    bg_color = *sel_bg;
                }
                // vi copy mode cursor cell: selection 보다 우선.
                if let Some((pt, cursor_bg)) = vi_cursor
                    && pt.col == col_idx
                    && pt.absolute_row == abs_row
                {
                    bg_color = *cursor_bg;
                }
                if let Some(link) = link
                    && link.covers(col_idx, abs_row)
                {
                    bg_color = link.bg;
                    fg_color = link.fg;
                }
                if let Some(sh) = search {
                    for (i, m) in sh.matches.iter().enumerate() {
                        if m.row == abs_row && col_idx >= m.col_start && col_idx < m.col_end {
                            bg_color = if i == sh.active_index {
                                sh.active_bg
                            } else {
                                sh.inactive_bg
                            };
                            break;
                        }
                    }
                }

                self.bg_instances.push(BgInstance {
                    pos: [col_idx as f32, row_idx as f32],
                    viewport_offset: off,
                    bg_color,
                });

                let text = cell_ref.str();

                if !text.is_empty() {
                    let ch = text.chars().next().unwrap();
                    if unicode_width(ch) > 1 && col_idx + 1 < cols {
                        self.bg_instances.push(BgInstance {
                            pos: [(col_idx + 1) as f32, row_idx as f32],
                            viewport_offset: off,
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

                if preedit.is_some_and(|p| p.covers(col_idx, row_idx)) {
                    continue;
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
            // Trailing cells in the row (incl. cursor on empty cell).
            let abs_row = row_offset + row_idx;
            for col_idx in last_col..cols {
                // Only a Block cursor fills the empty trailing cell; Bar/Underline
                // get an overlay glyph after the loop instead.
                let is_block_cursor = matches!(
                    cursor,
                    Some((cx, cy, _, CursorPaint::Block)) if row_idx == cy && col_idx == cx
                );
                let mut bg = if is_block_cursor {
                    default_fg
                } else {
                    default_bg
                };
                // vi copy mode cursor cell: trailing 영역 (콘텐츠 없는 row 끝) 에 vi cursor 가 위치한 경우 강조.
                if let Some((pt, cursor_bg)) = vi_cursor
                    && pt.col == col_idx
                    && pt.absolute_row == abs_row
                {
                    bg = *cursor_bg;
                }
                self.bg_instances.push(BgInstance {
                    pos: [col_idx as f32, row_idx as f32],
                    viewport_offset: off,
                    bg_color: bg,
                });
            }
        }

        // Bar/Underline cursor overlay: pushed after every cell glyph so the
        // block element renders on top of the character under the cursor. Block
        // cursors are already drawn via the fg/bg swap above.
        if let Some((cx, cy, wide, shape)) = cursor
            && let Some(glyph_ch) = shape.overlay_glyph()
            && cy < rows
        {
            // Underline spans both cells of a wide glyph; a bar marks only the
            // primary (left) cell.
            let span = if shape == CursorPaint::Underline && wide {
                2
            } else {
                1
            };
            for i in 0..span {
                let col = cx + i;
                if col < cols {
                    self.push_overlay_glyph(glyph_ch, col, cy, default_fg, off, queue);
                }
            }
        }

        // Right + bottom gutter (fractional cell area beyond grid).
        for row_idx in 0..rows {
            self.bg_instances.push(BgInstance {
                pos: [cols as f32, row_idx as f32],
                viewport_offset: off,
                bg_color: default_bg,
            });
        }
        for col_idx in 0..=cols {
            self.bg_instances.push(BgInstance {
                pos: [col_idx as f32, rows as f32],
                viewport_offset: off,
                bg_color: default_bg,
            });
        }
    }

    /// Push a single glyph instance used to overlay a sub-cell cursor shape
    /// (bar/underline block element) on top of the cell under the cursor.
    fn push_overlay_glyph(
        &mut self,
        ch: char,
        col: usize,
        row: usize,
        color: GpuRgba,
        off: [f32; 2],
        queue: &wgpu::Queue,
    ) {
        let key = GlyphKey {
            ch,
            bold: false,
            italic: false,
        };
        if let Some(entry) = self.atlas.get_or_insert(key, &mut self.font_config, queue)
            && entry.width > 0.0
            && entry.height > 0.0
        {
            self.glyph_instances.push(GlyphInstance {
                pos: [col as f32, row as f32],
                viewport_offset: off,
                uv_offset: [entry.uv_x, entry.uv_y],
                uv_size: [entry.uv_w, entry.uv_h],
                fg_color: color,
                glyph_offset: [entry.offset_x, entry.offset_y],
                glyph_size: [entry.width, entry.height],
                page: entry.page,
                _pad: 0,
            });
        }
    }

    /// Compute terminal grid size from a viewport rect (physical pixels).
    pub fn grid_size_for_rect(&self, rect: &PhysicalRect) -> (usize, usize) {
        let cell_w = self.font_config.metrics.cell_width.max(1.0);
        let cell_h = self.font_config.metrics.cell_height.max(1.0);
        let cols = (rect.width.value() / cell_w).floor() as usize;
        let rows = (rect.height.value() / cell_h).floor() as usize;
        (cols.max(1), rows.max(1))
    }

    fn append_preedit_overlay(
        &mut self,
        preedit: Option<&RenderPreedit>,
        queue: &wgpu::Queue,
        cols: usize,
        rows: usize,
        scroll_offset: usize,
    ) {
        let Some(preedit) = preedit else {
            return;
        };
        let screen_row = preedit.anchor_row + scroll_offset;
        if preedit.text.is_empty() || screen_row >= rows || preedit.anchor_col >= cols {
            return;
        }

        let off = self.current_viewport_offset;
        let mut col_idx = preedit.anchor_col;
        for ch in preedit.text.chars() {
            if col_idx >= cols {
                break;
            }

            let width = unicode_width(ch);
            for i in 0..width {
                if col_idx + i < cols {
                    self.bg_instances.push(BgInstance {
                        pos: [(col_idx + i) as f32, screen_row as f32],
                        viewport_offset: off,
                        bg_color: preedit.bg_color,
                    });
                }
            }

            let key = GlyphKey {
                ch,
                bold: false,
                italic: false,
            };
            if let Some(entry) = self.atlas.get_or_insert(key, &mut self.font_config, queue)
                && entry.width > 0.0
                && entry.height > 0.0
            {
                self.glyph_instances.push(GlyphInstance {
                    pos: [col_idx as f32, screen_row as f32],
                    viewport_offset: off,
                    uv_offset: [entry.uv_x, entry.uv_y],
                    uv_size: [entry.uv_w, entry.uv_h],
                    fg_color: preedit.fg_color,
                    glyph_offset: [entry.offset_x, entry.offset_y],
                    glyph_size: [entry.width, entry.height],
                    page: entry.page,
                    _pad: 0,
                });
            }

            col_idx += width;
        }
    }

    /// Resize the per-instance GPU buffers if the accumulated frame exceeds
    /// current capacity, then upload the frame's instance data in a single
    /// `write_buffer` call per kind. (R1: dynamic grow avoids silent clamp.)
    pub fn flush_buffers(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let bg_len = self.bg_instances.len();
        let glyph_len = self.glyph_instances.len();
        let needed = bg_len.max(glyph_len);

        if needed > self.max_instances {
            let mut new_cap = self.max_instances.max(1);
            while new_cap < needed {
                new_cap = new_cap.saturating_mul(2);
            }
            // Hard ceiling — 16M instances ≈ 1 GiB. Above this, clamp + warn.
            const HARD_CAP: usize = 16 * 1024 * 1024;
            if new_cap > HARD_CAP {
                tracing::warn!(
                    "renderer instance count {} exceeds hard cap {}; clamping",
                    needed,
                    HARD_CAP
                );
                new_cap = HARD_CAP;
                self.bg_instances.truncate(HARD_CAP);
                self.glyph_instances.truncate(HARD_CAP);
            }
            self.bg_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bg_instances"),
                size: (new_cap * std::mem::size_of::<BgInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.glyph_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("glyph_instances"),
                size: (new_cap * std::mem::size_of::<GlyphInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.max_instances = new_cap;
        }

        if bg_len > 0 {
            queue.write_buffer(
                &self.bg_instance_buffer,
                0,
                bytemuck::cast_slice(&self.bg_instances),
            );
        }
        if glyph_len > 0 {
            queue.write_buffer(
                &self.glyph_instance_buffer,
                0,
                bytemuck::cast_slice(&self.glyph_instances),
            );
        }
    }

    /// Issue all accumulated draws (bg pass then glyph pass) into a single
    /// render pass, applying per-surface scissor rects around the draw ranges.
    pub fn render_all<'a>(
        &'a self,
        render_pass: &mut wgpu::RenderPass<'a>,
        surface_width: u32,
        surface_height: u32,
    ) {
        if self.surface_ranges.is_empty() {
            return;
        }

        let any_bg = self.surface_ranges.iter().any(|(_, bg, _)| !bg.is_empty());
        let any_glyph = self.surface_ranges.iter().any(|(_, _, gl)| !gl.is_empty());

        if any_bg {
            render_pass.set_pipeline(&self.bg_pipeline);
            render_pass.set_bind_group(0, &self.bg_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.bg_instance_buffer.slice(..));
            for (rect, bg_range, _) in &self.surface_ranges {
                if bg_range.is_empty() {
                    continue;
                }
                let (x, y, w, h) = clip_scissor(rect, surface_width, surface_height);
                render_pass.set_scissor_rect(x, y, w, h);
                render_pass.draw(0..6, bg_range.clone());
                self.last_frame_bg_draws
                    .set(self.last_frame_bg_draws.get() + 1);
            }
        }

        if any_glyph {
            render_pass.set_pipeline(&self.glyph_pipeline);
            render_pass.set_bind_group(0, &self.glyph_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.glyph_instance_buffer.slice(..));
            for (rect, _, gl_range) in &self.surface_ranges {
                if gl_range.is_empty() {
                    continue;
                }
                let (x, y, w, h) = clip_scissor(rect, surface_width, surface_height);
                render_pass.set_scissor_rect(x, y, w, h);
                render_pass.draw(0..6, gl_range.clone());
                self.last_frame_glyph_draws
                    .set(self.last_frame_glyph_draws.get() + 1);
            }
        }
    }

    /// Frame 종료 시점의 draw call 카운트 `(bg, glyph, total)`. `render_all`
    /// 직후 (render() 끝) 에 호출되는 의미. `RenderPass::draw()` 호출만 셈 —
    /// `set_pipeline` / `set_scissor_rect` 는 포함하지 않음.
    pub fn draw_call_count(&self) -> (u32, u32, u32) {
        let bg = self.last_frame_bg_draws.get();
        let gl = self.last_frame_glyph_draws.get();
        (bg, gl, bg + gl)
    }

    /// 활성 (bg 또는 glyph range 비어있지 않은) surface 의 수.
    /// `regions` 의 변수 type 에 의존하지 않고 draw 결과로부터 직접 센다.
    pub fn active_surface_count(&self) -> u32 {
        self.surface_ranges
            .iter()
            .filter(|(_, bg, gl)| !bg.is_empty() || !gl.is_empty())
            .count() as u32
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

fn clip_scissor(
    viewport: &PhysicalRect,
    surface_width: u32,
    surface_height: u32,
) -> (u32, u32, u32, u32) {
    let x = (viewport.x.value().max(0.0) as u32).min(surface_width.saturating_sub(1));
    let y = (viewport.y.value().max(0.0) as u32).min(surface_height.saturating_sub(1));
    let max_w = surface_width.saturating_sub(x);
    let max_h = surface_height.saturating_sub(y);
    let w = (viewport.width.value().max(1.0) as u32).min(max_w).max(1);
    let h = (viewport.height.value().max(1.0) as u32).min(max_h).max(1);
    (x, y, w, h)
}
