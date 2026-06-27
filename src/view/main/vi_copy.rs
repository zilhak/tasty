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

/// double-key 시퀀스 대기 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOp {
    /// 첫 'g' 입력 후 두 번째 'g' 대기.
    G,
    /// `"` 입력 후 register character 대기.
    DoubleQuote,
    /// `q` 입력 후 register character 대기 (recording 시작).
    Q,
    /// `@` 입력 후 register character 대기 (recording replay).
    At,
}

/// 매크로 buffer 에 저장하는 키 — winit `Key` 의 lifetime 의존을 끊기 위해
/// 평탄화된 표현.
#[derive(Debug, Clone)]
pub struct MacroKey {
    pub repr: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
}

impl MacroKey {
    fn from_winit(key: &Key, modifiers: ModifiersState) -> Option<Self> {
        let repr = match key.as_ref() {
            Key::Character(s) => s.to_string(),
            Key::Named(n) => named_key_repr(&n)?.to_string(),
            _ => return None,
        };
        Some(Self {
            repr,
            ctrl: modifiers.control_key(),
            shift: modifiers.shift_key(),
            alt: modifiers.alt_key(),
            logo: modifiers.super_key(),
        })
    }

    fn to_winit(&self) -> (Key, ModifiersState) {
        let key = match repr_to_named_key(&self.repr) {
            Some(n) => Key::Named(n),
            None => Key::Character(self.repr.clone().into()),
        };
        let mut modifiers = ModifiersState::empty();
        if self.ctrl {
            modifiers |= ModifiersState::CONTROL;
        }
        if self.shift {
            modifiers |= ModifiersState::SHIFT;
        }
        if self.alt {
            modifiers |= ModifiersState::ALT;
        }
        if self.logo {
            modifiers |= ModifiersState::SUPER;
        }
        (key, modifiers)
    }
}

fn named_key_repr(n: &NamedKey) -> Option<&'static str> {
    match n {
        NamedKey::Escape => Some("Escape"),
        NamedKey::Enter => Some("Enter"),
        NamedKey::Backspace => Some("Backspace"),
        _ => None,
    }
}

fn repr_to_named_key(repr: &str) -> Option<NamedKey> {
    match repr {
        "Escape" => Some(NamedKey::Escape),
        "Enter" => Some(NamedKey::Enter),
        "Backspace" => Some(NamedKey::Backspace),
        _ => None,
    }
}

/// 진행 중인 매크로 녹화 상태.
#[derive(Debug, Clone)]
pub struct MacroRecording {
    pub register: char,
    pub keys: Vec<MacroKey>,
}

/// replay 재귀 깊이 한도. 16 을 넘으면 무한 루프로 간주하고 중단.
pub const MAX_REPLAY_DEPTH: u8 = 16;

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
    /// `gg` 등 double-key 시퀀스 대기 상태. 다음 키가 일치하면 동작, 아니면 취소.
    pub pending_op: Option<PendingOp>,
    /// 다음 yank 가 사용할 register (`+` clipboard, `*` primary, `"` 무명).
    /// `"` prefix 로 명시적으로 설정되며, yank 후 자동 초기화.
    pub active_register: Option<char>,
    /// 진행 중인 매크로 녹화. `q{reg}` 로 시작, `q` 로 중단 + `registers` 에 저장.
    pub recording: Option<MacroRecording>,
    /// 녹화 완료된 매크로 저장소 — `@{reg}` replay 시 lookup.
    pub registers: std::collections::HashMap<char, Vec<MacroKey>>,
    /// 현재 replay 재귀 깊이. `MAX_REPLAY_DEPTH` 도달 시 추가 replay 중단.
    pub replay_depth: u8,
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
            pending_op: None,
            active_register: None,
            recording: None,
            registers: std::collections::HashMap::new(),
            replay_depth: 0,
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
        if let Some(line) = terminal.screen_lines().get(screen_row) {
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
    /// `W` — 다음 WORD (whitespace-delimited) 의 시작.
    NextStartBig,
    /// `B` — 이전 WORD 의 시작.
    PrevStartBig,
    /// `E` — 다음 WORD 의 끝.
    NextEndBig,
}

fn is_big_word_char(ch: char) -> bool {
    !ch.is_whitespace()
}

fn class(kind: WordJump, ch: char) -> bool {
    match kind {
        WordJump::NextStartBig | WordJump::PrevStartBig | WordJump::NextEndBig => {
            is_big_word_char(ch)
        }
        _ => is_word_char(ch),
    }
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
        WordJump::NextStart | WordJump::NextStartBig => {
            // 현재 word 끝까지 진행 → 다음 word 시작까지.
            if idx >= flat.len() {
                return start;
            }
            let starting_word = class(kind, flat[idx].2);
            let mut i = idx + 1;
            while i < flat.len() && class(kind, flat[i].2) == starting_word && starting_word {
                i += 1;
            }
            while i < flat.len() && !class(kind, flat[i].2) {
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
        WordJump::PrevStart | WordJump::PrevStartBig => {
            if idx == 0 {
                return start;
            }
            // 한 칸 뒤로 후 word 의 시작점까지 뒤로.
            let mut i = idx.saturating_sub(1);
            // skip non-word backward
            while i > 0 && !class(kind, flat[i].2) {
                i -= 1;
            }
            // back to start of current word
            while i > 0 && class(kind, flat[i - 1].2) {
                i -= 1;
            }
            let (r, c, _) = flat[i];
            SelectionPoint {
                col: c,
                absolute_row: r,
            }
        }
        WordJump::NextEnd | WordJump::NextEndBig => {
            if idx >= flat.len() {
                return start;
            }
            let mut i = idx;
            // 현재가 word 마지막 char 면 다음 word 의 끝으로.
            if class(kind, flat[i].2) && (i + 1 >= flat.len() || !class(kind, flat[i + 1].2)) {
                i += 1;
            }
            // skip non-word forward
            while i < flat.len() && !class(kind, flat[i].2) {
                i += 1;
            }
            // advance to end of word
            while i + 1 < flat.len() && class(kind, flat[i + 1].2) {
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
        /// commit 시점의 검색 방향 — 현재 호출자는 query 만 쓰지만 액션의 완전한
        /// 의미를 위해 함께 담는다(`SearchNext`/`SearchPrev` 와 짝).
        #[allow(dead_code)] // variant 필드 — 액션 의미 완전성 위해 보존, 현재 미read
        direction: SearchDir,
    },
    /// search next/prev (n / N).
    SearchNext,
    SearchPrev,
    /// `"` 뒤에 알 수 없는 register character → toast 안내, 해당 키 소비.
    InvalidRegister,
    /// 매크로 녹화 시작 / 중단 → toast 안내.
    MacroRecordToggle,
    /// 매크로 replay 실패 (빈 register / depth 초과) → toast 안내.
    MacroReplayFailed,
}

/// 메인 키 dispatcher. cursor / visual / count / search 상태를 mutate.
/// terminal read-only 로 word_jump 등에 필요. PTY 송신은 전혀 안 일어남.
/// 매크로 녹화 중인 키는 outcome 이 `MacroRecordToggle` 이 아닌 경우에 한해 자동
/// 누적된다. replay 중 (`replay_depth > 0`) 키는 녹화에 다시 누적되지 않는다.
pub fn handle_vi_key(
    vi: &mut ViCopyMode,
    terminal: &tasty_terminal::Terminal,
    key: &Key,
    modifiers: ModifiersState,
) -> ViKeyOutcome {
    let outcome = handle_vi_key_inner(vi, terminal, key, modifiers);
    // 녹화 중이고, 녹화 시작/중단 토글 자체가 아니며, replay 중이 아니면 키를 buffer 에 push.
    let should_record = vi.recording.is_some()
        && vi.replay_depth == 0
        && !matches!(outcome, ViKeyOutcome::MacroRecordToggle);
    if should_record
        && let Some(mk) = MacroKey::from_winit(key, modifiers)
        && let Some(rec) = vi.recording.as_mut()
    {
        rec.keys.push(mk);
    }
    outcome
}

fn handle_vi_key_inner(
    vi: &mut ViCopyMode,
    terminal: &tasty_terminal::Terminal,
    key: &Key,
    modifiers: ModifiersState,
) -> ViKeyOutcome {
    // 1) mini-prompt 가 열린 경우: 검색 buffer 입력에 우선 라우팅.
    if let Some(search) = vi.search.as_mut()
        && let Some(buf) = search.buffer.as_mut()
    {
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

    // pending_op 분기: 직전 키로 시작된 double-key 시퀀스 대기 중.
    // 'g' 가 두 번째로 들어오면 top 이동, 그 외 키면 pending 만 취소 + 정상 처리 계속.
    // (`5gg` 의 경우 count_buf 는 첫 'g' 의 take_count() 에서 이미 소비됨 — §단순화 정책.)
    if let Some(op) = vi.pending_op.take() {
        match op {
            PendingOp::G => {
                if matches!(key.as_ref(), Key::Character(s) if s == "g") {
                    vi.cursor.absolute_row = 0;
                    vi.cursor.col = 0;
                    vi.count_buf.clear();
                    return ViKeyOutcome::Moved;
                }
                // 그 외: pending 만 취소하고 정상 처리 계속.
            }
            PendingOp::DoubleQuote => {
                if let Key::Character(s) = key
                    && let Some(ch) = s.chars().next()
                    && matches!(ch, '+' | '*' | '"')
                {
                    vi.active_register = Some(ch);
                    return ViKeyOutcome::Consumed;
                }
                return ViKeyOutcome::InvalidRegister;
            }
            PendingOp::Q => {
                // q 단독 + 다음 키가 register character 면 recording 시작.
                // 그 외 키 (특히 Esc / 문자 키) 면 vim 비표준 호환 동작 = 기존 `q` exit.
                if let Key::Character(s) = key
                    && let Some(reg) = s.chars().next()
                    && reg.is_ascii_alphanumeric()
                {
                    vi.recording = Some(MacroRecording {
                        register: reg,
                        keys: Vec::new(),
                    });
                    return ViKeyOutcome::MacroRecordToggle;
                }
                return ViKeyOutcome::Exit;
            }
            PendingOp::At => {
                if let Key::Character(s) = key
                    && let Some(reg) = s.chars().next()
                {
                    if vi.replay_depth >= MAX_REPLAY_DEPTH {
                        return ViKeyOutcome::MacroReplayFailed;
                    }
                    let Some(keys) = vi.registers.get(&reg).cloned() else {
                        return ViKeyOutcome::MacroReplayFailed;
                    };
                    vi.replay_depth += 1;
                    for mk in &keys {
                        let (k, m) = mk.to_winit();
                        let _ = handle_vi_key(vi, terminal, &k, m); // replay outcome 무시 — 호스트 효과는 outer wrapper 가 마지막 outcome 으로 한 번만 처리.
                    }
                    vi.replay_depth -= 1;
                    // replay 종료 후 count_buf 가 누수되지 않게 정리 (재진입 격리).
                    vi.count_buf.clear();
                    return ViKeyOutcome::Moved;
                }
                return ViKeyOutcome::Consumed;
            }
        }
    }

    let cols = terminal.cols();
    let scrollback_len = terminal.scrollback_len();
    let rows = terminal.rows();
    let max_row = scrollback_len + rows.saturating_sub(1);

    let ctrl = modifiers.control_key();

    // 2) Ctrl+v → block visual.
    if ctrl
        && let Key::Character(c) = key
        && c.eq_ignore_ascii_case("v")
    {
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

    // 3) 일반 키.
    match key.as_ref() {
        Key::Named(NamedKey::Escape) => {
            // vim 일치: 녹화 중 Esc 는 녹화를 조용히 저장 후 정상 Esc 동작.
            if let Some(rec) = vi.recording.take() {
                vi.registers.insert(rec.register, rec.keys);
            }
            if vi.has_visual() {
                vi.visual = ViCopyVisual::None;
                vi.anchor = None;
                vi.count_buf.clear();
                return ViKeyOutcome::Moved;
            }
            ViKeyOutcome::Exit
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
                    // 첫 'g' → pending 세팅. 두 번째 'g' 가 와야 top 이동 (top-level pending 분기에서 처리).
                    vi.pending_op = Some(PendingOp::G);
                    ViKeyOutcome::Consumed
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
                '^' => {
                    // 현재 행의 첫 non-whitespace col. 전부 공백이거나 빈 행이면 col=0.
                    let row = row_chars(terminal, vi.cursor.absolute_row);
                    let first_non_ws = row
                        .iter()
                        .find(|(_, ch)| !ch.is_whitespace())
                        .map(|(c, _)| *c)
                        .unwrap_or(0);
                    vi.cursor.col = first_non_ws;
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
                'W' => {
                    vi.cursor = word_jump(terminal, vi.cursor, WordJump::NextStartBig, count);
                    ViKeyOutcome::Moved
                }
                'B' => {
                    vi.cursor = word_jump(terminal, vi.cursor, WordJump::PrevStartBig, count);
                    ViKeyOutcome::Moved
                }
                'E' => {
                    vi.cursor = word_jump(terminal, vi.cursor, WordJump::NextEndBig, count);
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
                '"' => {
                    vi.pending_op = Some(PendingOp::DoubleQuote);
                    ViKeyOutcome::Consumed
                }
                'q' => {
                    if vi.recording.is_some() {
                        // 녹화 중 → 중단 + register 에 저장. 종료 키 (`q`) 자체는 buffer
                        // 에 포함되지 않음 (outer wrapper 가 MacroRecordToggle 을 보고 skip).
                        let rec = vi.recording.take().expect("recording is Some");
                        vi.registers.insert(rec.register, rec.keys);
                        ViKeyOutcome::MacroRecordToggle
                    } else {
                        // 녹화 안 함 → register character 대기 (PendingOp::Q).
                        vi.pending_op = Some(PendingOp::Q);
                        ViKeyOutcome::Consumed
                    }
                }
                '@' => {
                    vi.pending_op = Some(PendingOp::At);
                    ViKeyOutcome::Consumed
                }
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
            ViKeyOutcome::InvalidRegister => {
                let sid = self.vi_copy.as_ref().map(|v| v.surface_id).unwrap_or(0);
                self.state.toasts.push_info(
                    crate::i18n::t("toast.vi_copy_invalid_register"),
                    crate::adapters::ui::ToastScope::Surface(sid),
                );
            }
            ViKeyOutcome::MacroRecordToggle => {
                let (sid, recording) = self
                    .vi_copy
                    .as_ref()
                    .map(|v| (v.surface_id, v.recording.is_some()))
                    .unwrap_or((0, false));
                let key = if recording {
                    "toast.vi_copy_macro_recording_started"
                } else {
                    "toast.vi_copy_macro_recording_stopped"
                };
                self.state.toasts.push_info(
                    crate::i18n::t(key),
                    crate::adapters::ui::ToastScope::Surface(sid),
                );
            }
            ViKeyOutcome::MacroReplayFailed => {
                let sid = self.vi_copy.as_ref().map(|v| v.surface_id).unwrap_or(0);
                self.state.toasts.push_info(
                    crate::i18n::t("toast.vi_copy_macro_replay_failed"),
                    crate::adapters::ui::ToastScope::Surface(sid),
                );
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
        let register = vi.active_register;
        let text = match self.core_state.find_terminal_by_id(sid) {
            Some(t) => extract_selected_text(t, &sel),
            None => String::new(),
        };
        if text.is_empty() {
            self.vi_copy = None;
            return;
        }
        if let Some(cb) = &mut self.clipboard {
            match register {
                Some('*') => cb.set_text_primary(&text),
                Some('+') | Some('"') | None => cb.set_text(&text),
                _ => cb.set_text(&text),
            }
        }
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
    fn gg_moves_to_top() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = 5;
        handle_vi_key(&mut vi, &t, &key_char('g'), ModifiersState::empty());
        assert_eq!(vi.cursor.absolute_row, 5, "first g should not move");
        assert_eq!(vi.pending_op, Some(PendingOp::G));
        handle_vi_key(&mut vi, &t, &key_char('g'), ModifiersState::empty());
        assert_eq!(vi.cursor.absolute_row, 0);
        assert_eq!(vi.cursor.col, 0);
        assert_eq!(vi.pending_op, None);
    }

    #[test]
    fn single_g_then_other_cancels_pending() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = 5;
        handle_vi_key(&mut vi, &t, &key_char('g'), ModifiersState::empty());
        assert_eq!(vi.pending_op, Some(PendingOp::G));
        // j → pending 취소 + 정상 j 이동.
        handle_vi_key(&mut vi, &t, &key_char('j'), ModifiersState::empty());
        assert_eq!(vi.pending_op, None);
        assert_eq!(
            vi.cursor.absolute_row, 6,
            "j should still move after pending cancel"
        );
    }

    #[test]
    fn g_then_escape_cancels_pending_then_exits() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('g'), ModifiersState::empty());
        assert_eq!(vi.pending_op, Some(PendingOp::G));
        let out = handle_vi_key(
            &mut vi,
            &t,
            &Key::Named(NamedKey::Escape),
            ModifiersState::empty(),
        );
        // pending 취소 후 escape 정상 처리 (visual 없으므로 Exit).
        assert!(matches!(out, ViKeyOutcome::Exit));
        assert_eq!(vi.pending_op, None);
    }

    /// 터미널 첫 화면 행에 텍스트를 주입한다.
    fn write_line(t: &mut Terminal, text: &str) {
        t.process_bytes(text.as_bytes());
    }

    #[test]
    fn caret_jumps_to_first_non_whitespace() {
        let mut t = term(40, 10);
        write_line(&mut t, "   foo");
        let mut vi = ViCopyMode::enter(0, &t);
        // 첫 화면 행 = scrollback_len.
        vi.cursor.absolute_row = t.scrollback_len();
        vi.cursor.col = 10;
        handle_vi_key(&mut vi, &t, &key_char('^'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, 3, "should land on first non-whitespace");
    }

    #[test]
    fn caret_on_empty_line_stays_at_zero() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = t.scrollback_len();
        vi.cursor.col = 5;
        handle_vi_key(&mut vi, &t, &key_char('^'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, 0);
    }

    #[test]
    fn caret_on_all_whitespace_line_goes_to_zero() {
        let mut t = term(40, 10);
        write_line(&mut t, "      ");
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = t.scrollback_len();
        vi.cursor.col = 4;
        handle_vi_key(&mut vi, &t, &key_char('^'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, 0);
    }

    #[test]
    fn big_w_jumps_over_punctuation() {
        let mut t = term(40, 10);
        write_line(&mut t, "foo.bar baz");
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = t.scrollback_len();
        vi.cursor.col = 0;
        handle_vi_key(&mut vi, &t, &key_char('W'), ModifiersState::empty());
        // BIG WORD treats `foo.bar` as one — next start = col 8 (`baz`).
        assert_eq!(vi.cursor.col, 8);
    }

    #[test]
    fn big_e_jumps_to_big_word_end() {
        let mut t = term(40, 10);
        write_line(&mut t, "foo.bar baz");
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = t.scrollback_len();
        vi.cursor.col = 0;
        handle_vi_key(&mut vi, &t, &key_char('E'), ModifiersState::empty());
        // BIG word end of `foo.bar` is col 6 (last char `r`).
        assert_eq!(vi.cursor.col, 6);
    }

    #[test]
    fn big_b_jumps_to_prev_big_word_start() {
        let mut t = term(40, 10);
        write_line(&mut t, "foo.bar baz");
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = t.scrollback_len();
        vi.cursor.col = 8;
        handle_vi_key(&mut vi, &t, &key_char('B'), ModifiersState::empty());
        // Previous BIG word start = col 0.
        assert_eq!(vi.cursor.col, 0);
    }

    #[test]
    fn big_w_count_repeats() {
        let mut t = term(40, 10);
        write_line(&mut t, "a.b c.d e.f");
        let mut vi = ViCopyMode::enter(0, &t);
        vi.cursor.absolute_row = t.scrollback_len();
        vi.cursor.col = 0;
        handle_vi_key(&mut vi, &t, &key_char('2'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('W'), ModifiersState::empty());
        // 2W from col 0 → `e.f` at col 8.
        assert_eq!(vi.cursor.col, 8);
    }

    #[test]
    fn register_prefix_plus_sets_active_register() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        let out = handle_vi_key(&mut vi, &t, &key_char('"'), ModifiersState::empty());
        assert!(matches!(out, ViKeyOutcome::Consumed));
        assert_eq!(vi.pending_op, Some(PendingOp::DoubleQuote));
        handle_vi_key(&mut vi, &t, &key_char('+'), ModifiersState::empty());
        assert_eq!(vi.active_register, Some('+'));
        assert_eq!(vi.pending_op, None);
    }

    #[test]
    fn register_prefix_star_sets_active_register() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('"'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('*'), ModifiersState::empty());
        assert_eq!(vi.active_register, Some('*'));
    }

    #[test]
    fn invalid_register_returns_invalid_register_outcome() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('"'), ModifiersState::empty());
        let out = handle_vi_key(&mut vi, &t, &key_char('j'), ModifiersState::empty());
        assert!(matches!(out, ViKeyOutcome::InvalidRegister));
        assert_eq!(vi.active_register, None);
        assert_eq!(vi.pending_op, None);
    }

    #[test]
    fn record_then_replay_repeats_motion() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        // qa l j j j q → register a 에 4 키 저장.
        handle_vi_key(&mut vi, &t, &key_char('q'), ModifiersState::empty());
        let out = handle_vi_key(&mut vi, &t, &key_char('a'), ModifiersState::empty());
        assert!(matches!(out, ViKeyOutcome::MacroRecordToggle));
        handle_vi_key(&mut vi, &t, &key_char('l'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('j'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('j'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('j'), ModifiersState::empty());
        let stop = handle_vi_key(&mut vi, &t, &key_char('q'), ModifiersState::empty());
        assert!(matches!(stop, ViKeyOutcome::MacroRecordToggle));
        assert!(vi.recording.is_none());
        assert_eq!(vi.registers.get(&'a').map(|v| v.len()), Some(4));

        let after_record = vi.cursor;
        // @a → replay: 다시 한번 (l, j×3) 실행.
        handle_vi_key(&mut vi, &t, &key_char('@'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('a'), ModifiersState::empty());
        assert_eq!(vi.cursor.col, after_record.col + 1);
        assert_eq!(vi.cursor.absolute_row, after_record.absolute_row + 3);
    }

    #[test]
    fn empty_macro_replay_signals_failure() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('@'), ModifiersState::empty());
        let out = handle_vi_key(&mut vi, &t, &key_char('z'), ModifiersState::empty());
        assert!(matches!(out, ViKeyOutcome::MacroReplayFailed));
    }

    #[test]
    fn recursive_macro_depth_capped() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        // register a 에 `@a` 만 들어가는 매크로 — 무한 재귀 시도.
        let mk_at = MacroKey::from_winit(&key_char('@'), ModifiersState::empty()).unwrap();
        let mk_a = MacroKey::from_winit(&key_char('a'), ModifiersState::empty()).unwrap();
        vi.registers.insert('a', vec![mk_at, mk_a]);
        handle_vi_key(&mut vi, &t, &key_char('@'), ModifiersState::empty());
        let _ = handle_vi_key(&mut vi, &t, &key_char('a'), ModifiersState::empty()); // outcome 무관 — depth 가 다시 0 으로 돌아왔는지만 확인.
        assert_eq!(vi.replay_depth, 0, "depth should reset after replay");
    }

    #[test]
    fn recording_esc_stops_recording_then_exits() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('q'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('a'), ModifiersState::empty());
        assert!(vi.recording.is_some());
        let out = handle_vi_key(
            &mut vi,
            &t,
            &Key::Named(NamedKey::Escape),
            ModifiersState::empty(),
        );
        assert!(matches!(out, ViKeyOutcome::Exit));
        assert!(vi.recording.is_none());
        assert!(vi.registers.contains_key(&'a'));
    }

    #[test]
    fn q_alone_then_non_register_exits() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        handle_vi_key(&mut vi, &t, &key_char('q'), ModifiersState::empty());
        assert_eq!(vi.pending_op, Some(PendingOp::Q));
        // ASCII 비알파숫자 키 (`!`) → vim 비표준 호환 동작 = Exit.
        let out = handle_vi_key(&mut vi, &t, &key_char('!'), ModifiersState::empty());
        assert!(matches!(out, ViKeyOutcome::Exit));
        assert!(vi.recording.is_none());
    }

    #[test]
    fn macro_replay_does_not_leak_count_buf() {
        let t = term(40, 10);
        let mut vi = ViCopyMode::enter(0, &t);
        // qa 3 j q  → register a 에 `3` `j` 저장 (count prefix 가 buffer 에 들어감).
        handle_vi_key(&mut vi, &t, &key_char('q'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('a'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('3'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('j'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('q'), ModifiersState::empty());

        // replay.
        handle_vi_key(&mut vi, &t, &key_char('@'), ModifiersState::empty());
        handle_vi_key(&mut vi, &t, &key_char('a'), ModifiersState::empty());
        assert!(
            vi.count_buf.is_empty(),
            "count_buf should be empty after replay; got {:?}",
            vi.count_buf
        );
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
