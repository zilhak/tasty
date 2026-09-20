/// Maximum size of the output buffer (1 MB).
const OUTPUT_BUFFER_MAX: usize = 1_048_576;

/// Raw PTY output buffer with two independent read cursors.
///
/// - `read_mark`: owned by the agent-facing mark API (`surface.set_mark`,
///   `surface.read_since_mark`, `surface.parse_since_mark`). An agent moves it
///   explicitly and reads are non-destructive, so the same window can be read
///   again.
/// - `scan_mark`: owned by a periodic output scanner
///   (`surface.read_since_scan_mark`). It advances on every read, so a scanner
///   polling in a loop receives only the bytes that arrived since its previous
///   poll instead of the whole buffer, and it never moves the agent's mark.
///
/// The two cursors are separate so that neither consumer shifts the other's
/// window. A scanner sharing `read_mark` was measured to re-read up to the whole
/// buffer on every poll and to jump silently whenever an agent called
/// `surface.set_mark`
/// (`docs/adr/0307-the-output-scanner-reads-its-own-cursor.md`).
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
            tasty_ansi::strip_ansi(&text)
        } else {
            text
        }
    }

    /// Read the output accumulated since the previous scan-cursor read and
    /// advance the scan cursor past it.
    ///
    /// Reading and advancing are one operation on purpose. Split into two calls
    /// they leave a window in which appended output is passed over by the
    /// advance and therefore reported to nobody.
    ///
    /// The returned slice starts wherever the previous read stopped, which is a
    /// byte offset and not a boundary of anything. Two kinds of thing get split
    /// there, and a caller that matches on the text has to tolerate both:
    ///
    /// - An escape sequence straddling two reads. With `strip_ansi` the leftover
    ///   half survives into the text; a caller that needs whole sequences should
    ///   ask for the raw bytes instead.
    /// - A multi byte character straddling two reads. Each half is lossy decoded
    ///   on its own, so both come out as U+FFFD and the character is gone from
    ///   the text on either side. `read_since_mark` has the same property at its
    ///   own start offset, so this is not particular to the scan cursor; what is
    ///   particular is that an advancing cursor creates a new such offset on
    ///   every read.
    pub fn take_since_scan_mark(&mut self, strip_ansi: bool) -> String {
        let start = self.scan_mark.min(self.buffer.len());
        let text = String::from_utf8_lossy(&self.buffer[start..]).to_string();
        self.scan_mark = self.buffer.len();
        if strip_ansi {
            tasty_ansi::strip_ansi(&text)
        } else {
            text
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OUTPUT_BUFFER_MAX, OutputBuffer};

    /// Push `n` bytes of filler whose content is irrelevant to the assertion.
    fn filler(n: usize) -> Vec<u8> {
        vec![b'.'; n]
    }

    #[test]
    fn a_read_mark_inside_the_trimmed_region_is_dropped_and_the_next_read_starts_over() {
        let mut b = OutputBuffer::new();
        b.append(b"head");
        b.set_mark();
        b.append(b"tail");
        assert_eq!(b.read_since_mark(false), "tail");

        // Overflow by more than the mark's offset, so the mark lands in the
        // trimmed region.
        b.append(&filler(OUTPUT_BUFFER_MAX));
        let after = b.read_since_mark(false);
        assert_eq!(
            after.len(),
            OUTPUT_BUFFER_MAX,
            "the mark was dropped, so the read covers the whole retained buffer \
             instead of the bytes since the mark — and the response carries no \
             sign that it happened"
        );
    }

    #[test]
    fn a_read_mark_outside_the_trimmed_region_shifts_by_the_trimmed_amount() {
        let mut b = OutputBuffer::new();
        b.append(&filler(OUTPUT_BUFFER_MAX));
        b.set_mark();
        b.append(b"after");
        // Trimmed 5 bytes off the front; the mark sat at OUTPUT_BUFFER_MAX, well
        // past that, so it survives and still points just before "after".
        assert_eq!(b.read_since_mark(false), "after");
    }

    #[test]
    fn the_scan_cursor_advances_so_each_read_returns_only_what_arrived_since() {
        let mut b = OutputBuffer::new();
        b.append(b"first");
        assert_eq!(b.take_since_scan_mark(false), "first");
        assert_eq!(
            b.take_since_scan_mark(false),
            "",
            "nothing was appended between the two reads"
        );
        b.append(b"second");
        assert_eq!(b.take_since_scan_mark(false), "second");
    }

    #[test]
    fn the_two_cursors_do_not_move_each_other() {
        let mut b = OutputBuffer::new();
        b.append(b"before");
        b.take_since_scan_mark(false);

        // The agent sets its mark; the scan cursor must not follow it.
        b.append(b"between");
        b.set_mark();
        b.append(b"after");
        assert_eq!(
            b.take_since_scan_mark(false),
            "betweenafter",
            "set_mark moved the scan cursor — the scanner would have lost \
             'between'"
        );

        // And the scan read must not have moved the agent's mark.
        assert_eq!(b.read_since_mark(false), "after");
    }

    #[test]
    fn trim_pulls_the_scan_cursor_back_instead_of_dropping_it() {
        let mut b = OutputBuffer::new();
        b.append(&filler(OUTPUT_BUFFER_MAX));
        b.take_since_scan_mark(false);
        b.append(b"tail");
        assert_eq!(
            b.take_since_scan_mark(false),
            "tail",
            "the cursor was clamped to the retained region, so only the newly \
             appended bytes come back"
        );
    }

    #[test]
    fn strip_ansi_applies_to_the_scan_read_as_well() {
        let mut b = OutputBuffer::new();
        b.append(b"\x1b[31mred\x1b[0m");
        assert_eq!(b.take_since_scan_mark(true), "red");
    }
}
