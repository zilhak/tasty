//! vi-style 키보드 복사 모드 — `Ctrl+Shift+Space` 진입, hjkl/wbe 이동, v/V/Ctrl+v
//! visual, y yank, / ? 검색, n/N 매치 이동, count prefix 지원.
//!
//! mode 가 활성일 때 keyboard 핸들러가 `handle_vi_key` 로 키를 가로채 PTY 송신을
//! 차단한다. mouse drag 가 시작되면 자동 종료된다 (mouse.rs).

use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::selection::{SelectionMode, SelectionPoint, TextSelection};

/// vi-mode 의 visual 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViCopyVisual {
    None,
    Char,
    Line,
    Block,
}

/// 검색 방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDir {
    Forward,
    Backward,
}

/// `/` 또는 `?` 입력 시 활성화되는 mini-prompt + 직전 검색의 방향.
#[derive(Debug, Clone)]
pub struct ViCopySearch {
    pub direction: SearchDir,
    /// mini-prompt 가 활성일 때 입력 중 buffer. None 이면 mini-prompt 종료 상태.
    pub buffer: Option<String>,
}

/// vi copy mode state.
#[derive(Debug, Clone)]
pub struct ViCopyMode {
    pub surface_id: u32,
    pub cursor: SelectionPoint,
    pub visual: ViCopyVisual,
    pub anchor: Option<SelectionPoint>,
    /// count prefix 누적 buffer ("3", "12" 등). take_count() 시 1 이상 정수로 변환.
    pub count_buf: String,
    pub search: Option<ViCopySearch>,
}

impl ViCopyMode {
    /// 진입 시 cursor 는 현재 화면 첫 행 (display row 0) 의 0 col 로.
    pub fn enter(surface_id: u32, terminal: &tasty_terminal::Terminal) -> Self {
        let scrollback_len = terminal.scrollback_len();
        let scroll_offset = terminal.scroll_offset();
        // display row 0 의 absolute_row = scrollback_len - scroll_offset.
        let absolute_row = scrollback_len.saturating_sub(scroll_offset);
        Self {
            surface_id,
            cursor: SelectionPoint {
                col: 0,
                absolute_row,
            },
            visual: ViCopyVisual::None,
            anchor: None,
            count_buf: String::new(),
            search: None,
        }
    }

    /// 누적된 count 를 소비하여 반환. 비어 있거나 0 이면 1.
    /// 6자리 cap (max 999_999) 으로 overflow 차단.
    pub fn take_count(&mut self) -> usize {
        let buf = std::mem::take(&mut self.count_buf);
        if buf.is_empty() {
            return 1;
        }
        let trimmed = if buf.len() > 6 {
            &buf[buf.len() - 6..]
        } else {
            &buf[..]
        };
        trimmed.parse::<usize>().unwrap_or(1).max(1)
    }

    /// visual mode 가 활성이면 anchor↔cursor 기반 TextSelection 반환.
    /// None 인 경우 1-cell highlight (anchor==cursor) 를 반환하여 cursor 시각화에 사용.
    pub fn live_selection(&self) -> TextSelection {
        let (anchor, mode) = match self.visual {
            ViCopyVisual::None => (self.cursor, SelectionMode::Normal),
            ViCopyVisual::Char => (self.anchor.unwrap_or(self.cursor), SelectionMode::Normal),
            ViCopyVisual::Line => (self.anchor.unwrap_or(self.cursor), SelectionMode::Line),
            ViCopyVisual::Block => (self.anchor.unwrap_or(self.cursor), SelectionMode::Block),
        };
        TextSelection {
            anchor,
            cursor: self.cursor,
            mode,
            surface_id: self.surface_id,
            dragging: false,
        }
    }

    /// visual 중인지 (선택 활성).
    pub fn has_visual(&self) -> bool {
        self.visual != ViCopyVisual::None
    }
}

/// 한 글자가 word character 인가? (vi 의 `iskeyword` 기본값과 동일)
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// 한 row 의 셀 텍스트를 col → char 매핑으로 추출. col 인덱스는 visible_cells 의
/// cell_index 와 동일 (멀티-byte 셀의 시작 col).
fn row_chars(terminal: &tasty_terminal::Terminal, abs_row: usize) -> Vec<(usize, char)> {
    let scrollback_len = terminal.scrollback_len();
    let mut out: Vec<(usize, char)> = Vec::new();
    if abs_row < scrollback_len {
        if let Some(line) = terminal.scrollback_line_owned(abs_row) {
            let mut col = 0usize;
            for (text, _) in &line {
                let ch = text.chars().next().unwrap_or(' ');
                out.push((col, ch));
                #[cfg(feature = "gui")]
                let w = crate::renderer::unicode_width(ch);
                #[cfg(not(feature = "gui"))]
                let w = if ch.is_ascii() { 1 } else { 2 };
                col += w.max(1);
            }
        }
    } else {
        let screen_row = abs_row - scrollback_len;
        if let Some(line) = terminal.surface().screen_lines().get(screen_row) {
            for cell in line.visible_cells() {
                let col = cell.cell_index();
                let ch = cell.str().chars().next().unwrap_or(' ');
                out.push((col, ch));
            }
        }
    }
    out
}

/// word jump 종류.
#[derive(Debug, Clone, Copy)]
pub enum WordJump {
    /// `w` — 다음 word 의 시작.
    NextStart,
    /// `b` — 이전 word 의 시작.
    PrevStart,
    /// `e` — 다음 word 의 끝.
    NextEnd,
}

/// 단어 점프. `count` 반복.
pub fn word_jump(
    terminal: &tasty_terminal::Terminal,
    start: SelectionPoint,
    kind: WordJump,
    count: usize,
) -> SelectionPoint {
    let scrollback_len = terminal.scrollback_len();
    let rows = terminal.rows();
    let total_rows = scrollback_len + rows;
    let mut cur = start;
    for _ in 0..count {
        cur = word_jump_once(terminal, cur, kind, total_rows);
    }
    cur
}

fn word_jump_once(
    terminal: &tasty_terminal::Terminal,
    start: SelectionPoint,
    kind: WordJump,
    total_rows: usize,
) -> SelectionPoint {
    // 평탄화: 현재 cursor 시점 ±20 row 범위로 (col, char) 시퀀스를 만들고
    // 그 위에서 일반적인 word boundary scan. 충분히 넓으면 실용상 무한 스크롤백
    // 모두 다룰 필요가 없다.
    let span: isize = 30;
    let from = (start.absolute_row as isize - span).max(0) as usize;
    let to = ((start.absolute_row as isize + span) as usize).min(total_rows.saturating_sub(1));
    let mut flat: Vec<(usize, usize, char)> = Vec::new(); // (abs_row, col, ch)
    for r in from..=to {
        for (c, ch) in row_chars(terminal, r) {
            flat.push((r, c, ch));
        }
    }
    if flat.is_empty() {
        return start;
    }
    // start 위치를 flat 안의 index 로 매핑 (없으면 가까운 위치).
    let mut idx = flat
        .iter()
        .position(|(r, c, _)| {
            *r > start.absolute_row || (*r == start.absolute_row && *c >= start.col)
        })
        .unwrap_or(flat.len() - 1);
    match kind {
        WordJump::NextStart => {
            // 현재 word 끝까지 진행 → 다음 word 시작까지.
            if idx >= flat.len() {
                return start;
            }
            let starting_word = is_word_char(flat[idx].2);
            let mut i = idx + 1;
            while i < flat.len() && is_word_char(flat[i].2) == starting_word && starting_word {
                i += 1;
            }
            while i < flat.len() && !is_word_char(flat[i].2) {
                i += 1;
            }
            if i < flat.len() {
                let (r, c, _) = flat[i];
                return SelectionPoint {
                    col: c,
                    absolute_row: r,
                };
            }
            start
        }
        WordJump::PrevStart => {
            if idx == 0 {
                return start;
            }
            // 한 칸 뒤로 후 word 의 시작점까지 뒤로.
            let mut i = idx.saturating_sub(1);
            // skip non-word backward
            while i > 0 && !is_word_char(flat[i].2) {
                i -= 1;
            }
            // back to start of current word
            while i > 0 && is_word_char(flat[i - 1].2) {
                i -= 1;
            }
            let (r, c, _) = flat[i];
            SelectionPoint {
                col: c,
                absolute_row: r,
            }
        }
        WordJump::NextEnd => {
            if idx >= flat.len() {
                return start;
            }
            let mut i = idx;
            // 현재가 word 마지막 char 면 다음 word 의 끝으로.
            if is_word_char(flat[i].2) && (i + 1 >= flat.len() || !is_word_char(flat[i + 1].2)) {
                i += 1;
            }
            // skip non-word forward
            while i < flat.len() && !is_word_char(flat[i].2) {
                i += 1;
            }
            // advance to end of word
            while i + 1 < flat.len() && is_word_char(flat[i + 1].2) {
                i += 1;
            }
            if i < flat.len() {
                let (r, c, _) = flat[i];
                return SelectionPoint {
                    col: c,
                    absolute_row: r,
                };
            }
            // fallback
            idx = flat.len() - 1;
            let (r, c, _) = flat[idx];
            SelectionPoint {
                col: c,
                absolute_row: r,
            }
        }
    }
}

/// 이동 키 처리 결과. handler 가 후속 처리 (viewport 정렬, count 소비).
#[derive(Debug)]
pub enum ViKeyOutcome {
    /// 키 무시.
    NotHandled,
    /// 키 소비 + 추가 작업 없음 (count_buf 누적 등).
    Consumed,
    /// 키 소비 + cursor 이동. 호출자가 viewport 정렬 수행.
    Moved,
    /// y / Enter 등 결과 액션. 호출자가 yank/clipboard 처리.
    Yank,
    /// mode 종료.
    Exit,
    /// 검색 실행: mini-prompt 가 commit 됨. 호출자가 query 로 검색.
    SearchCommit {
        query: String,
        #[allow(dead_code)]
        direction: SearchDir,
    },
    /// search next/prev (n / N).
    SearchNext,
    SearchPrev,
}

/// 메인 키 dispatcher. cursor / visual / count / search 상태를 mutate.
/// terminal read-only 로 word_jump 등에 필요. PTY 송신은 전혀 안 일어남.
pub fn handle_vi_key(
    vi: &mut ViCopyMode,
    terminal: &tasty_terminal::Terminal,
    key: &Key,
    modifiers: ModifiersState,
) -> ViKeyOutcome {
    // 1) mini-prompt 가 열린 경우: 검색 buffer 입력에 우선 라우팅.
    if let Some(search) = vi.search.as_mut() {
        if let Some(buf) = search.buffer.as_mut() {
            match key.as_ref() {
                Key::Named(NamedKey::Escape) => {
                    vi.search = None;
                    return ViKeyOutcome::Consumed;
                }
                Key::Named(NamedKey::Enter) => {
                    if buf.is_empty() {
                        vi.search = None;
                        return ViKeyOutcome::Consumed;
                    }
                    let query = std::mem::take(buf);
                    let direction = search.direction;
                    search.buffer = None;
                    return ViKeyOutcome::SearchCommit { query, direction };
                }
                Key::Named(NamedKey::Backspace) => {
                    buf.pop();
                    return ViKeyOutcome::Consumed;
                }
                _ => {
                    if let Key::Character(s) = key {
                        // 인쇄 가능한 ASCII / 유니코드 누적.
                        for ch in s.chars() {
                            if !ch.is_control() {
                                buf.push(ch);
                            }
                        }
                        return ViKeyOutcome::Consumed;
                    }
                    return ViKeyOutcome::Consumed;
                }
            }
        }
    }

    let cols = terminal.cols();
    let scrollback_len = terminal.scrollback_len();
    let rows = terminal.rows();
    let max_row = scrollback_len + rows.saturating_sub(1);

    let ctrl = modifiers.control_key();

    // 2) Ctrl+v → block visual.
    if ctrl {
        if let Key::Character(c) = key {
            if c.eq_ignore_ascii_case("v") {
                if vi.visual == ViCopyVisual::Block {
                    vi.visual = ViCopyVisual::None;
                    vi.anchor = None;
                } else {
                    vi.visual = ViCopyVisual::Block;
                    vi.anchor = Some(vi.cursor);
                }
                vi.count_buf.clear();
                return ViKeyOutcome::Moved;
            }
        }
    }

    // 3) 일반 키.
    match key.as_ref() {
        Key::Named(NamedKey::Escape) => {
            if vi.has_visual() {
                vi.visual = ViCopyVisual::None;
                vi.anchor = None;
                vi.count_buf.clear();
                return ViKeyOutcome::Moved;
            }
            return ViKeyOutcome::Exit;
        }
        Key::Character(s) => {
            let ch = match s.chars().next() {
                Some(c) => c,
                None => return ViKeyOutcome::NotHandled,
            };
            // count prefix: '0' 단독은 line start, 그 외 digit 은 누적.
            if ch.is_ascii_digit() {
                if ch == '0' && vi.count_buf.is_empty() {
                    vi.cursor.col = 0;
                    return ViKeyOutcome::Moved;
                }
                vi.count_buf.push(ch);
                return ViKeyOutcome::Consumed;
            }
            let count = vi.take_count();
            match ch {
                'h' => {
                    vi.cursor.col = vi.cursor.col.saturating_sub(count);
                    ViKeyOutcome::Moved
                }
                'l' => {
                    vi.cursor.col = (vi.cursor.col + count).min(cols.saturating_sub(1));
                    ViKeyOutcome::Moved
                }
                'j' => {
                    vi.cursor.absolute_row = (vi.cursor.absolute_row + count).min(max_row);
                    ViKeyOutcome::Moved
                }
                'k' => {
                    vi.cursor.absolute_row = vi.cursor.absolute_row.saturating_sub(count);
                    ViKeyOutcome::Moved
                }
                'g' => {
                    // `gg` 처리: count_buf 가 `g` 처음일 때는 따로 1 글자 buffer 가 없으므로
                    // 간이 처리 — 한 번 `g` 누르면 top 으로 (vim 의 `gg` 와 약간 다르지만 단순).
                    // 정확한 `gg` 시퀀스는 별도 pending state 도입이 필요. 여기는 한 번에 top.
                    vi.cursor.absolute_row = 0;
                    vi.cursor.col = 0;
                    ViKeyOutcome::Moved
                }
                'G' => {
                    vi.cursor.absolute_row = max_row;
                    vi.cursor.col = 0;
                    ViKeyOutcome::Moved
                }
                'H' => {
                    // viewport top.
                    let scroll_offset = terminal.scroll_offset();
                    vi.cursor.absolute_row = scrollback_len.saturating_sub(scroll_offset);
                    ViKeyOutcome::Moved
                }
                'M' => {
                    let scroll_offset = terminal.scroll_offset();
                    let top = scrollback_len.saturating_sub(scroll_offset);
                    vi.cursor.absolute_row = (top + rows / 2).min(max_row);
                    ViKeyOutcome::Moved
                }
                'L' => {
                    let scroll_offset = terminal.scroll_offset();
                    let top = scrollback_len.saturating_sub(scroll_offset);
                    vi.cursor.absolute_row = (top + rows.saturating_sub(1)).min(max_row);
                    ViKeyOutcome::Moved
                }
                '$' => {
                    vi.cursor.col = cols.saturating_sub(1);
                    ViKeyOutcome::Moved
                }
                'w' => {
                    vi.cursor = word_jump(terminal, vi.cursor, WordJump::NextStart, count);
                    ViKeyOutcome::Moved
                }
                'b' => {
                    vi.cursor = word_jump(terminal, vi.cursor, WordJump::PrevStart, count);
                    ViKeyOutcome::Moved
                }
                'e' => {
                    vi.cursor = word_jump(terminal, vi.cursor, WordJump::NextEnd, count);
                    ViKeyOutcome::Moved
                }
                'v' => {
                    if vi.visual == ViCopyVisual::Char {
                        vi.visual = ViCopyVisual::None;
                        vi.anchor = None;
                    } else {
                        vi.visual = ViCopyVisual::Char;
                        vi.anchor = Some(vi.cursor);
                    }
                    ViKeyOutcome::Moved
                }
                'V' => {
                    if vi.visual == ViCopyVisual::Line {
                        vi.visual = ViCopyVisual::None;
                        vi.anchor = None;
                    } else {
                        vi.visual = ViCopyVisual::Line;
                        vi.anchor = Some(vi.cursor);
                    }
                    ViKeyOutcome::Moved
                }
                'y' => ViKeyOutcome::Yank,
                'q' => ViKeyOutcome::Exit,
                '/' => {
                    vi.search = Some(ViCopySearch {
                        direction: SearchDir::Forward,
                        buffer: Some(String::new()),
                    });
                    ViKeyOutcome::Moved
                }
                '?' => {
                    vi.search = Some(ViCopySearch {
                        direction: SearchDir::Backward,
                        buffer: Some(String::new()),
                    });
                    ViKeyOutcome::Moved
                }
                'n' => ViKeyOutcome::SearchNext,
                'N' => ViKeyOutcome::SearchPrev,
                _ => ViKeyOutcome::NotHandled,
            }
        }
        _ => ViKeyOutcome::NotHandled,
    }
}

// ─── MainView integration ───────────────────────────────────────────────────

use crate::core::CoreState;
use crate::selection::extract_selected_text;

use super::MainView;

impl MainView {
    /// `pending_enter_copy_mode` 플래그를 소비하고 mode 진입을 시도한다.
    pub(crate) fn try_enter_vi_copy_mode(&mut self) {
        if !self.state.dialogs.pending_enter_copy_mode {
            return;
        }
        self.state.dialogs.pending_enter_copy_mode = false;
        if self.vi_copy.is_some() {
            // 이미 활성 → noop.
            return;
        }
        let Some(sid) = self.state.focused_surface_id(&self.core_state) else {
            return;
        };
        let Some(terminal) = self.state.focused_terminal(&self.core_state) else {
            return;
        };
        if terminal.is_alternate_screen() {
            self.state.toasts.push_info(
                crate::i18n::t("toast.vi_copy_blocked_alt_screen"),
                crate::adapters::ui::ToastScope::Surface(sid),
            );
            return;
        }
        self.vi_copy = Some(ViCopyMode::enter(sid, terminal));
        self.text_selection = None;
        self.base.dirty = true;
    }

    /// vi mode 가 활성일 때 키 이벤트를 가로채고 처리한다. true 면 키가 소비됨.
    pub(crate) fn try_handle_vi_key(
        &mut self,
        key: &winit::keyboard::Key,
        modifiers: ModifiersState,
    ) -> bool {
        if self.vi_copy.is_none() {
            return false;
        }

        // surface ID + terminal 의 viewport 상태를 미리 read.
        let (rows, scrollback_len) = {
            let Some(t) = self.state.focused_terminal(&self.core_state) else {
                // surface 가 사라짐 → vi mode 종료.
                self.vi_copy = None;
                return true;
            };
            (t.rows(), t.scrollback_len())
        };

        let outcome = {
            let terminal = match self.state.focused_terminal(&self.core_state) {
                Some(t) => t,
                None => {
                    self.vi_copy = None;
                    return true;
                }
            };
            let vi = self.vi_copy.as_mut().expect("vi_copy is Some");
            handle_vi_key(vi, terminal, key, modifiers)
        };

        match outcome {
            ViKeyOutcome::NotHandled => return false,
            ViKeyOutcome::Consumed => {}
            ViKeyOutcome::Moved => {
                self.vi_copy_viewport_align(rows, scrollback_len);
            }
            ViKeyOutcome::Yank => {
                self.vi_copy_yank();
            }
            ViKeyOutcome::Exit => {
                self.vi_copy = None;
            }
            ViKeyOutcome::SearchCommit {
                query,
                direction: _,
            } => {
                let surface_id = self.vi_copy.as_ref().map(|v| v.surface_id).unwrap_or(0);
                self.state.search.query = query;
                self.state.search.surface_id = surface_id;
                if let Some(terminal) = self.core_state.find_terminal_by_id(surface_id) {
                    self.state.search.execute(terminal);
                }
                Self::vi_copy_jump_to_current_match(self, rows, scrollback_len);
            }
            ViKeyOutcome::SearchNext => {
                self.vi_copy_search_navigate(true, rows, scrollback_len);
            }
            ViKeyOutcome::SearchPrev => {
                self.vi_copy_search_navigate(false, rows, scrollback_len);
            }
        }
        self.base.dirty = true;
        true
    }

    /// cursor 가 viewport 밖이면 scroll 하여 정렬.
    fn vi_copy_viewport_align(&mut self, rows: usize, scrollback_len: usize) {
        let Some(vi) = self.vi_copy.as_ref() else {
            return;
        };
        let cursor_row = vi.cursor.absolute_row;
        let Some(terminal) = self.state.focused_terminal_mut(&mut self.core_state) else {
            return;
        };
        let scroll_offset = terminal.scroll_offset();
        let viewport_top = scrollback_len.saturating_sub(scroll_offset);
        let viewport_bottom = viewport_top + rows.saturating_sub(1);
        if cursor_row < viewport_top {
            terminal.scroll_up(viewport_top - cursor_row);
        } else if cursor_row > viewport_bottom {
            terminal.scroll_down(cursor_row - viewport_bottom);
        }
    }

    /// 현재 vi selection 을 클립보드에 복사하고 mode 종료.
    fn vi_copy_yank(&mut self) {
        let Some(vi) = self.vi_copy.as_ref() else {
            return;
        };
        if !vi.has_visual() {
            // visual 없으면 yank 무의미 — mode 종료만.
            self.vi_copy = None;
            return;
        }
        let sel = vi.live_selection();
        let sid = sel.surface_id;
        let text = match self.core_state.find_terminal_by_id(sid) {
            Some(t) => extract_selected_text(t, &sel),
            None => String::new(),
        };
        if text.is_empty() {
            self.vi_copy = None;
            return;
        }
        if let Some(cb) = &mut self.clipboard {
            cb.set_text(&text);
        }
        self.core_state.record_internal_copy(&text);
        self.state.toasts.push_info(
            crate::i18n::t("toast.copied"),
            crate::adapters::ui::ToastScope::Surface(sid),
        );
        self.vi_copy = None;
    }

    fn vi_copy_search_navigate(&mut self, forward: bool, rows: usize, scrollback_len: usize) {
        if self.state.search.matches.is_empty() {
            return;
        }
        let Some(vi) = self.vi_copy.as_ref() else {
            return;
        };
        let dir_forward = match vi.search.as_ref().map(|s| s.direction) {
            Some(SearchDir::Backward) => !forward,
            _ => forward,
        };
        if dir_forward {
            self.state.search.next_match();
        } else {
            self.state.search.prev_match();
        }
        Self::vi_copy_jump_to_current_match(self, rows, scrollback_len);
    }

    fn vi_copy_jump_to_current_match(view: &mut Self, rows: usize, scrollback_len: usize) {
        let m = match view
            .state
            .search
            .matches
            .get(view.state.search.current_index)
        {
            Some(m) => m.clone(),
            None => return,
        };
        if let Some(vi) = view.vi_copy.as_mut() {
            vi.cursor = SelectionPoint {
                col: m.col_start,
                absolute_row: m.row,
            };
        }
        view.vi_copy_viewport_align(rows, scrollback_len);
    }

    /// `live_selection` 우선 — vi mode 의 1-cell cursor 또는 visual selection 을
    /// mouse text_selection 보다 우선 반환. None 이면 기존 text_selection 그대로.
    pub(crate) fn active_text_selection(&self) -> Option<crate::selection::TextSelection> {
        if let Some(vi) = self.vi_copy.as_ref() {
            return Some(vi.live_selection());
        }
        self.text_selection.clone()
    }
}

/// 표준 입력 path 에서 호출하는 진입점. core_state 를 다시 한번 참조하지 않도록
/// 별도 모듈로 분리하지 않고 same module.
#[allow(dead_code)]
fn _module_uses_core_state(_: &CoreState) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tasty_terminal::{Terminal, TerminalConfig};

    fn term(cols: usize, rows: usize) -> Terminal {
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        Terminal::new(
            TerminalConfig {
                cols,
                rows,
                shell: None,
                args: &[],
                surface_id: 0,
                working_dir: None,
                initial_input: None,
            },
            waker,
        )
        .expect("terminal creation")
    }

    fn key_char(c: char) -> Key {
        Key::Character(c.to_string().into())
    }

    #[test]
    fn enter_then_exit_returns_to_normal() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        assert_eq!(vi.visual, ViCopyVisual::None);
        let out = handle_vi_key(
            &mut vi,
            &t,
            &Key::Named(NamedKey::Escape),
            ModifiersState::empty(),
        );
        assert!(matches!(out, ViKeyOutcome::Exit));
    }

    #[test]
    fn hjkl_move_cursor() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        let start = vi.cursor;
        // j moves down
        handle_vi_key(&mut vi, &t, &key_char('j'), ModifiersState::empty());
        assert_eq!(vi.cursor.absolute_row, start.absolute_row + 1);
        // l moves right
        handle_vi_key(&mut vi, &t, &key_char('l'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, 1);
        // h moves left
        handle_vi_key(&mut vi, &t, &key_char('h'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, 0);
        // k moves up
        handle_vi_key(&mut vi, &t, &key_char('k'), ModifiersState::empty());
        assert_eq!(vi.cursor.absolute_row, start.absolute_row);
    }

    #[test]
    fn count_prefix_repeats_movement() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        let start = vi.cursor;
        handle_vi_key(&mut vi, &t, &key_char('5'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('l'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, start.col + 5);
        // count buf cleared after use
        assert!(vi.count_buf.is_empty());
    }

    #[test]
    fn zero_alone_is_line_start_but_part_of_count() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.col = 10;
        // '0' alone: line start.
        handle_vi_key(&mut vi, &t, &key_char('0'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, 0);
        // '1' then '0' then 'l' → 10 cols right.
        handle_vi_key(&mut vi, &t, &key_char('1'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('0'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('l'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, 10);
    }

    #[test]
    fn visual_char_then_escape_clears_visual_not_mode() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('v'), ModifiersState::empty());
        assert_eq!(vi.visual, ViCopyVisual::Char);
        let out = handle_vi_key(
            &mut vi,
            &t,
            &Key::Named(NamedKey::Escape),
            ModifiersState::empty(),
        );
        assert!(matches!(out, ViKeyOutcome::Moved));
        assert_eq!(vi.visual, ViCopyVisual::None);
    }

    #[test]
    fn yank_outcome_signals_copy() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('v'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('l'), ModifiersState::empty());
        let out = handle_vi_key(&mut vi, &t, &key_char('y'), ModifiersState::empty());
        assert!(matches!(out, ViKeyOutcome::Yank));
    }

    #[test]
    fn search_slash_opens_buffer_and_enter_commits() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('/'), ModifiersState::empty());
        assert!(vi.search.is_some());
        assert!(vi.search.as_ref().unwrap().buffer.is_some());
        handle_vi_key(&mut vi, &t, &key_char('a'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('b'), ModifiersState::empty());
        let out = handle_vi_key(
            &mut vi,
            &t,
            &Key::Named(NamedKey::Enter),
            ModifiersState::empty(),
        );
        match out {
            ViKeyOutcome::SearchCommit {
                ref query,
                direction,
            } => {
                assert_eq!(query, "ab");
                assert_eq!(direction, SearchDir::Forward);
            }
            other => panic!("expected SearchCommit, got {other:?}"),
        }
        assert!(vi.search.as_ref().unwrap().buffer.is_none());
    }

    #[test]
    fn count_buf_overflow_capped() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        for _ in 0..15 {
            handle_vi_key(&mut vi, &t, &key_char('9'), ModifiersState::empty());
        }
        let count = vi.take_count();
        assert!(count <= 999_999, "count {count} exceeds cap");
    }
}
