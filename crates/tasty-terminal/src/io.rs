//! `Terminal` 의 IO 경로 — PTY 출력 처리 / 입력 송신 / change apply.

use std::sync::mpsc;

use termwiz::surface::Change;

use crate::Terminal;

impl Terminal {
    /// Feed raw bytes through the shared ingest path. Useful for testing without
    /// a real PTY, and used by the debug `feed_bytes` IPC handler.
    pub fn process_bytes(&mut self, data: &[u8]) {
        self.ingest(data);
    }

    /// Wire a detached mirror's input forwarding sink. When the terminal has no
    /// PTY, `send_bytes`/`send_key` forward to this sink (the attach stream)
    /// instead of writing to a PTY. PTY-backed terminals ignore it.
    pub fn set_input_sink(&mut self, sink: mpsc::Sender<Vec<u8>>) {
        self.input_sink = Some(sink);
    }

    /// Route input bytes to the PTY when present, otherwise forward to the
    /// detached input sink. With neither, the bytes are dropped (stage 2 leaves
    /// the sink unwired; the attach stream connects it in stage 3).
    fn write_input(&mut self, bytes: Vec<u8>) {
        self.last_input_at = std::time::Instant::now();
        if let Some(pty) = self.pty.as_ref() {
            if let Err(e) = pty.pty_write_tx.send(bytes) {
                tracing::warn!("pty writer channel closed during input: {e}");
            }
        } else if let Some(sink) = self.input_sink.as_ref() {
            if let Err(e) = sink.send(bytes) {
                tracing::warn!("detached input sink closed: {e}");
            }
        } else {
            tracing::trace!(
                "detached terminal input dropped (no sink): {} bytes",
                bytes.len()
            );
        }
    }

    /// Send keyboard input to PTY (non-blocking, queued to writer thread).
    pub fn send_key(&mut self, text: &str) {
        self.write_input(text.as_bytes().to_vec());
    }

    pub(crate) fn send_terminal_response(&mut self, response: &str) {
        self.write_input(response.as_bytes().to_vec());
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
        self.write_input(bytes.to_vec());
    }
}
