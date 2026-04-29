use std::sync::LazyLock;

/// Maximum size of the output buffer (1 MB).
const OUTPUT_BUFFER_MAX: usize = 1_048_576;

/// Raw PTY output buffer with dual read-mark support.
///
/// Tracks two independent marks:
/// - `read_mark`: owned by the IPC `terminal.read_since_mark` API
/// - `scan_mark`: owned by the ClaudeError output scanner
pub(crate) struct OutputBuffer {
    buffer: Vec<u8>,
    read_mark: Option<usize>,
    scan_mark: usize,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            read_mark: None,
            scan_mark: 0,
        }
    }

    /// Append raw bytes and trim to max size, adjusting marks as needed.
    pub fn append(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        if self.buffer.len() > OUTPUT_BUFFER_MAX {
            let excess = self.buffer.len() - OUTPUT_BUFFER_MAX;
            self.buffer.drain(..excess);
            // Adjust read mark
            if let Some(mark) = &mut self.read_mark {
                if *mark <= excess {
                    self.read_mark = None; // mark was in trimmed region, invalidate
                } else {
                    *mark -= excess;
                }
            }
            // Adjust scan mark
            self.scan_mark = self.scan_mark.saturating_sub(excess);
        }
    }

    /// Set a read mark at the current end of the buffer.
    pub fn set_mark(&mut self) {
        self.read_mark = Some(self.buffer.len());
    }

    /// Read output since the last mark. If no mark was set, reads from the beginning.
    pub fn read_since_mark(&self, strip_ansi: bool) -> String {
        let start = self.read_mark.unwrap_or(0).min(self.buffer.len());
        let bytes = &self.buffer[start..];
        let text = String::from_utf8_lossy(bytes).to_string();
        if strip_ansi {
            strip_ansi_escapes(&text)
        } else {
            text
        }
    }

    /// Return raw bytes accumulated since the last `set_scan_mark()` call.
    /// Used by the ClaudeError scanner; independent of `read_since_mark`'s mark.
    pub fn output_since_scan_mark(&self, strip_ansi: bool) -> String {
        let start = self.scan_mark.min(self.buffer.len());
        let bytes = &self.buffer[start..];
        let text = String::from_utf8_lossy(bytes).to_string();
        if strip_ansi {
            strip_ansi_escapes(&text)
        } else {
            text
        }
    }

    /// Advance the scan mark to the current end of the buffer.
    pub fn set_scan_mark(&mut self) {
        self.scan_mark = self.buffer.len();
    }
}

static ANSI_ESCAPE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]|\x1b\][^\x07]*\x07|\x1b\][^\x1b]*\x1b\\")
        .expect("static regex is valid")
});

/// Strip ANSI escape sequences from a string using regex.
fn strip_ansi_escapes(s: &str) -> String {
    ANSI_ESCAPE_RE.replace_all(s, "").to_string()
}
