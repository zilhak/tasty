//! `Terminal` 의 IO 경로 — PTY 출력 처리 / 입력 송신 / change apply.

use termwiz::escape::Action;
use termwiz::escape::csi::CSI;
use termwiz::surface::Change;

use crate::Terminal;

impl Terminal {
    /// This is useful for testing without a real PTY.
    pub fn process_bytes(&mut self, data: &[u8]) {
        if !data.is_empty() {
            self.last_output_at = std::time::Instant::now();
        }
        let actions = self.parser.parse_as_vec(data);
        for action in actions {
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                self.handle_mode(mode);
                continue;
            }
            let changes = self.action_to_changes(action);
            for change in changes {
                self.apply_or_stage_change(change);
            }
        }
    }

    /// Send keyboard input to PTY (non-blocking, queued to writer thread).
    pub fn send_key(&mut self, text: &str) {
        self.last_input_at = std::time::Instant::now();
        if let Err(e) = self.pty_write_tx.send(text.as_bytes().to_vec()) {
            tracing::warn!("pty writer channel closed during send_key: {e}");
        }
    }

    pub(crate) fn send_terminal_response(&self, response: &str) {
        if let Err(e) = self.pty_write_tx.send(response.as_bytes().to_vec()) {
            tracing::warn!("pty writer channel closed during terminal response: {e}");
        }
    }

    pub(crate) fn apply_or_stage_change(&mut self, change: Change) {
        // Always apply changes immediately to keep surface state (especially
        // cursor position) current. Many VTE operations (EraseLine, DeleteLine,
        // EraseCharacter, etc.) read cursor_position() at generation time to
        // produce absolute-positioned changes. If changes are staged during
        // synchronized output (mode 2026), cursor_position() returns stale
        // values, causing those operations to target wrong rows/columns.
        //
        // Tasty's architecture is process-then-render: all PTY data is processed
        // before the GPU reads the surface, so immediate application doesn't
        // cause visual tearing — the renderer always sees the final state.
        self.apply_change(change);
    }

    pub(crate) fn apply_change(&mut self, change: Change) {
        if self.use_alternate {
            self.surface_mut().add_change(change);
            return;
        }

        self.capture_before_scroll(&change);
        self.surface_mut().add_change(change);
    }

    /// Send raw bytes to PTY (non-blocking, queued to writer thread).
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        self.last_input_at = std::time::Instant::now();
        if let Err(e) = self.pty_write_tx.send(bytes.to_vec()) {
            tracing::warn!("pty writer channel closed during send_bytes: {e}");
        }
    }
}
