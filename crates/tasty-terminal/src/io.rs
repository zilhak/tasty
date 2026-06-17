//! IO 경로 — 입력 송신 / change apply.
//!
//! 입력 송신(`write_input`/`send_terminal_response`)과 change apply 는 락 안에서
//! 도는 `TerminalState` 에 둔다 — VTE 핸들러가 파서 스레드에서 DSR/DA 응답을
//! PTY 로 되쓰기 때문이다. 사용자 입력 API(`send_key`/`send_bytes`)는 핸들
//! (`Terminal`) 이 락을 잡아 위임한다 (ADR-0002).

use std::sync::mpsc;

use termwiz::cell::CellAttributes;
use termwiz::surface::Change;

use crate::{Terminal, TerminalState};

impl TerminalState {
    /// Route input bytes to the PTY writer (or the detached input sink). With
    /// neither wired, the bytes are dropped. Always records the input timestamp
    /// so PTY echo within `INPUT_ECHO_WINDOW` is not counted toward busy state.
    pub(crate) fn write_input(&mut self, bytes: Vec<u8>) {
        self.last_input_at = std::time::Instant::now();
        if let Some(sink) = self.input_tx.as_ref() {
            if let Err(e) = sink.send(bytes) {
                tracing::warn!("terminal input channel closed during input: {e}");
            }
        } else {
            tracing::trace!("terminal input dropped (no sink): {} bytes", bytes.len());
        }
    }

    /// Reply to a terminal query (DSR / DA / cursor position report). Runs on the
    /// parser thread during ingest, so it writes back through the same input
    /// channel.
    pub(crate) fn send_terminal_response(&mut self, response: &str) {
        self.write_input(response.as_bytes().to_vec());
    }

    pub(crate) fn apply_or_stage_change(&mut self, change: Change) {
        // Always apply changes immediately to keep surface state (especially
        // cursor position) current. Many VTE operations read cursor_position() at
        // generation time to produce absolute-positioned changes. Tasty's
        // architecture is process-then-render, so immediate application doesn't
        // cause visual tearing — the renderer always sees the final state.
        self.apply_change(change);
    }

    pub(crate) fn apply_change(&mut self, change: Change) {
        self.mirror_pen(&change);
        if self.use_alternate {
            self.surface_mut().add_change(change);
            return;
        }

        // Text can scroll the grid internally (auto-wrap past the bottom row)
        // without emitting a ScrollRegionUp; that path captures evictions itself.
        if let Change::Text(text) = change {
            self.apply_text_capturing_scrolls(text);
            return;
        }

        self.capture_before_scroll(&change);
        self.surface_mut().add_change(change);
    }

    /// Keep `current_pen` in sync with the pen mutations termwiz's `Surface`
    /// performs internally (it offers no pen accessor). Mirrors exactly the
    /// `Surface::apply_change` cases that touch the pen: full attribute replace,
    /// single-attribute change, and the clear ops (which termwiz resets to
    /// default with the clear color). Read back by `map_sgr` to apply SGRs that
    /// lack an `AttributeChange` variant (Overline/UnderlineColor/VerticalAlign).
    fn mirror_pen(&mut self, change: &Change) {
        match change {
            Change::AllAttributes(attr) => self.current_pen = attr.clone(),
            Change::Attribute(attr_change) => self.current_pen.apply_change(attr_change),
            Change::ClearScreen(color)
            | Change::ClearToEndOfLine(color)
            | Change::ClearToEndOfScreen(color) => {
                self.current_pen = CellAttributes::default().set_background(*color).clone();
            }
            _ => {}
        }
    }

    /// Swap the active/inactive pen mirrors when crossing the primary↔alternate
    /// screen boundary. termwiz holds a separate pen per surface but exposes no
    /// accessor, so `current_pen` tracks the active surface and `saved_pen` holds
    /// the other; swapping on each transition keeps the mirror aligned with the
    /// surface that subsequent changes land on. Call exactly on a real transition
    /// of `use_alternate`. See `modes.rs` (alt-screen modes 1049 / 47 / 1047).
    pub(crate) fn swap_pen_for_surface_switch(&mut self) {
        std::mem::swap(&mut self.current_pen, &mut self.saved_pen);
    }
}

impl Terminal {
    /// Feed raw bytes through the shared ingest path. Useful for testing without
    /// a real PTY, and used by the debug `feed_bytes` IPC handler.
    pub fn process_bytes(&mut self, data: &[u8]) {
        if self.lock_state().ingest(data) {
            self.dirty.store(true, std::sync::atomic::Ordering::Release);
        }
    }

    /// Wire a detached mirror's input forwarding sink. When the terminal has no
    /// PTY, `send_bytes`/`send_key` forward to this sink (the attach stream).
    /// PTY-backed terminals already have their writer channel wired and ignore
    /// reconfiguration through this path in practice.
    pub fn set_input_sink(&mut self, sink: mpsc::Sender<Vec<u8>>) {
        self.lock_state().input_tx = Some(sink);
    }

    /// Plumb the host's resolved theme palette so OSC 10/11/12/4 color *queries*
    /// are answered with the colors the renderer actually draws. The host calls
    /// this on terminal creation and whenever the theme changes.
    pub fn set_color_palette(&mut self, palette: crate::color::ColorPalette) {
        self.lock_state().color_palette = Some(palette);
    }

    /// Send keyboard input to PTY (non-blocking, queued to writer thread).
    pub fn send_key(&mut self, text: &str) {
        self.lock_state().write_input(text.as_bytes().to_vec());
    }

    /// Send raw bytes to PTY (non-blocking, queued to writer thread).
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        self.lock_state().write_input(bytes.to_vec());
    }
}
