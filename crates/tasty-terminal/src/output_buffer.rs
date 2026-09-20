use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum number of raw output bytes one terminal retains (1 MiB).
///
/// This is the retention window *and* the ceiling on a single read: a read
/// cannot return more than what is retained, so no separate server-side read
/// cap exists. Consumers ask for less with `max_bytes`.
///
/// The value lives here only. A copy in the IPC layer would be a second place
/// to change and the two would drift silently — the event ring names the same
/// hazard (`docs/adr/0322-the-event-ring-keeps-positions-and-says-what-it-dropped.md`).
pub const OUTPUT_RETENTION_MAX_BYTES: usize = 1_048_576;

/// Disambiguates two streams created inside the same nanosecond.
static NEXT_STREAM_SEQ: AtomicU64 = AtomicU64::new(0);

/// Mint a token that identifies one terminal's output stream.
///
/// A surface id is reused when a surface closes and another opens, and
/// `surface.respawn_terminal` replaces the terminal under a surface id that
/// does not change at all. Either way a consumer's saved position would
/// silently address bytes it never saw. The token makes that case a value:
/// carry the token with the position and a mismatch is an error rather than
/// somebody else's output.
///
/// The wall clock supplies the part that survives a host restart (positions
/// restart at 0, so a counter alone would repeat). The counter supplies the
/// part the clock cannot: two terminals built in the same nanosecond.
fn mint_stream_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = NEXT_STREAM_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{seq:x}")
}

/// Where a read starts.
pub enum OutputCursor {
    /// The agent's server-held mark (`surface.set_mark`). With no mark ever
    /// set, or with a mark that has fallen out of retention, the read starts at
    /// the oldest retained byte — and then `skipped` is not zero.
    Mark,
    /// An absolute position the consumer holds. The server keeps no state for
    /// it, so two consumers reading this way never move each other.
    At(u64),
}

/// What a consumer asks for.
pub struct OutputReadRequest {
    pub from: OutputCursor,
    /// Upper bound on the **raw** bytes this read covers — not on the length of
    /// the text that comes back. Clamped to `1..=OUTPUT_RETENTION_MAX_BYTES`.
    pub max_bytes: usize,
    pub strip_ansi: bool,
    /// The stream the cursor was taken from. `Some` makes a stale cursor an
    /// error instead of a silent read of another stream's bytes.
    pub expect_stream: Option<String>,
}

/// What one read says.
///
/// **`raw_bytes` and `text.len()` are different numbers and only the first one
/// is in cursor units.** Lossy UTF-8 decoding replaces malformed bytes with
/// U+FFFD (3 bytes) and `strip_ansi` removes bytes, so the text can be both
/// longer and shorter than the region it came from. A consumer that advances
/// by `text.len()` desynchronises from the stream; it advances by
/// `next_cursor`.
#[derive(Debug)]
pub struct OutputRead {
    pub text: String,
    /// Raw bytes this read covered.
    pub raw_bytes: usize,
    /// Where this read actually started. Differs from the requested position
    /// exactly when `skipped` is not zero.
    pub cursor: u64,
    /// Where the next read continues. Equal to `retention_end` means the
    /// consumer is caught up, and the next read returns empty text until more
    /// output arrives — that is the only "end" this stream has, since a live
    /// terminal never closes its output.
    pub next_cursor: u64,
    /// Oldest retained position.
    pub retention_start: u64,
    /// One past the newest retained position.
    pub retention_end: u64,
    /// Raw bytes that fell out of retention between the requested position and
    /// where the read had to start. Not zero means output was lost for good.
    pub skipped: u64,
    /// This stream's token. Carry it back with the next cursor.
    pub stream: String,
}

/// Why a read was refused. Both arms mean the consumer's position cannot be
/// honoured; answering with bytes anyway would hand it output it must not
/// treat as a continuation.
#[derive(Debug, PartialEq, Eq)]
pub enum OutputReadError {
    /// The position names bytes this stream has not produced yet. Reading from
    /// a live terminal, that position may become valid later — which is why
    /// this cannot be answered with an empty read: the consumer would wait for
    /// a continuation that, when it arrives, is unrelated to what it last saw.
    AheadOfStream { cursor: u64, retention_end: u64 },
    /// The position was taken from a different stream — the surface id was
    /// reused, or the terminal was respawned under it.
    StreamMismatch { expected: String, actual: String },
}

/// Raw PTY output buffer with three ways to say where a read starts.
///
/// - `read_mark`: owned by the agent-facing mark API (`surface.set_mark`,
///   `surface.read_since_mark`, `surface.parse_since_mark`). An agent moves it
///   explicitly and reads are non-destructive, so the same window can be read
///   again. One per terminal, not one per consumer.
/// - `scan_mark`: owned by a periodic output scanner
///   (`surface.read_since_scan_mark`). It advances on every read, so a scanner
///   polling in a loop receives only the bytes that arrived since its previous
///   poll instead of the whole buffer, and it never moves the agent's mark.
/// - a position the **consumer** holds and passes in ([`OutputCursor::At`]).
///   The buffer keeps no state for it, so any number of consumers read this
///   way without moving each other.
///
/// The first two are separate so that neither consumer shifts the other's
/// window. A scanner sharing `read_mark` was measured to re-read up to the
/// whole buffer on every poll and to jump silently whenever an agent called
/// `surface.set_mark`
/// (`docs/adr/0307-the-output-scanner-reads-its-own-cursor.md`).
///
/// **Positions are absolute and never go backwards.** They count raw bytes
/// this terminal has produced since it started, so trimming the front moves
/// `base` and leaves every mark where it was. A mark that trimming has passed
/// is still a number the buffer can compare against, which is how a read can
/// say how much it lost instead of quietly starting over
/// (`docs/adr/0341-a-terminal-output-read-answers-from-a-position-the-consumer-holds.md`).
pub(crate) struct OutputBuffer {
    buffer: Vec<u8>,
    /// Absolute position of `buffer[0]`.
    base: u64,
    read_mark: Option<u64>,
    scan_mark: u64,
    stream: String,
}

impl OutputBuffer {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            base: 0,
            read_mark: None,
            scan_mark: 0,
            stream: mint_stream_id(),
        }
    }

    /// Append raw bytes and trim to the retention size.
    ///
    /// No mark is adjusted: the marks are absolute and only `base` moves.
    pub fn append(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
        if self.buffer.len() > OUTPUT_RETENTION_MAX_BYTES {
            let excess = self.buffer.len() - OUTPUT_RETENTION_MAX_BYTES;
            self.buffer.drain(..excess);
            self.base += excess as u64;
        }
    }

    /// Set a read mark at the current end of the buffer.
    pub fn set_mark(&mut self) {
        self.read_mark = Some(self.end());
    }

    /// Read output since the last mark. If no mark was set, reads from the
    /// beginning of what is retained.
    ///
    /// Kept as the compatibility path for `surface.read_since_mark`: the text
    /// is what it always was. What the mark cannot say on its own — that
    /// trimming passed it — is in [`Self::read`]'s `skipped`.
    pub fn read_since_mark(&self, strip_ansi: bool) -> String {
        self.read_from(self.mark_position(), OUTPUT_RETENTION_MAX_BYTES, strip_ansi)
            .text
    }

    /// Answer a read from a position, refusing the positions that cannot be a
    /// continuation of what this stream gave out.
    pub fn read(&self, req: &OutputReadRequest) -> Result<OutputRead, OutputReadError> {
        if let Some(expected) = &req.expect_stream
            && expected != &self.stream
        {
            return Err(OutputReadError::StreamMismatch {
                expected: expected.clone(),
                actual: self.stream.clone(),
            });
        }
        let requested = match req.from {
            OutputCursor::Mark => self.mark_position(),
            OutputCursor::At(at) => at,
        };
        if requested > self.end() {
            return Err(OutputReadError::AheadOfStream {
                cursor: requested,
                retention_end: self.end(),
            });
        }
        Ok(self.read_from(
            requested,
            req.max_bytes.clamp(1, OUTPUT_RETENTION_MAX_BYTES),
            req.strip_ansi,
        ))
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
        let text = self
            .read_from(
                self.scan_mark.max(self.base),
                OUTPUT_RETENTION_MAX_BYTES,
                strip_ansi,
            )
            .text;
        self.scan_mark = self.end();
        text
    }

    /// One past the newest retained position.
    fn end(&self) -> u64 {
        self.base + self.buffer.len() as u64
    }

    /// Where a mark read starts asking from. A mark trimming has passed is left
    /// where it is so that [`Self::read`] can measure the gap.
    fn mark_position(&self) -> u64 {
        self.read_mark.unwrap_or(self.base)
    }

    /// The read itself. Infallible: the requested position is clamped into the
    /// retained region and the gap is reported rather than refused.
    fn read_from(&self, requested: u64, max_bytes: usize, strip_ansi: bool) -> OutputRead {
        let start = requested.clamp(self.base, self.end());
        let off = (start - self.base) as usize;
        let take = cut_before_split_char(&self.buffer[off..], max_bytes);
        let text = String::from_utf8_lossy(&self.buffer[off..off + take]).to_string();
        OutputRead {
            text: if strip_ansi {
                tasty_ansi::strip_ansi(&text)
            } else {
                text
            },
            raw_bytes: take,
            cursor: start,
            next_cursor: start + take as u64,
            retention_start: self.base,
            retention_end: self.end(),
            skipped: start.saturating_sub(requested),
            stream: self.stream.clone(),
        }
    }
}

/// How many of `bytes` to take when at most `max` are wanted, moved back off a
/// UTF-8 continuation byte so that `max` does not cut a character in half.
///
/// Backing off costs at most three bytes and they arrive on the next read. It
/// is skipped when it would take nothing at all, because a read that returns
/// zero bytes while bytes are waiting never makes progress — a consumer with a
/// small `max_bytes` in front of a multi byte character would loop forever.
fn cut_before_split_char(bytes: &[u8], max: usize) -> usize {
    if max >= bytes.len() {
        return bytes.len();
    }
    let mut cut = max;
    let mut backed_off = 0;
    while cut > 0 && backed_off < 3 && bytes[cut] & 0b1100_0000 == 0b1000_0000 {
        cut -= 1;
        backed_off += 1;
    }
    if cut == 0 { max } else { cut }
}

#[cfg(test)]
mod tests {
    use super::{
        OUTPUT_RETENTION_MAX_BYTES, OutputBuffer, OutputCursor, OutputReadError, OutputReadRequest,
    };

    /// Push `n` bytes of filler whose content is irrelevant to the assertion.
    fn filler(n: usize) -> Vec<u8> {
        vec![b'.'; n]
    }

    /// Helper for the cursor form: ask from an absolute position with no cap
    /// and no stream check.
    fn at(b: &OutputBuffer, cursor: u64) -> super::OutputRead {
        b.read(&OutputReadRequest {
            from: OutputCursor::At(cursor),
            max_bytes: OUTPUT_RETENTION_MAX_BYTES,
            strip_ansi: false,
            expect_stream: None,
        })
        .expect("a position inside the stream is answerable")
    }

    /// Helper for the mark form.
    fn from_mark(b: &OutputBuffer) -> super::OutputRead {
        b.read(&OutputReadRequest {
            from: OutputCursor::Mark,
            max_bytes: OUTPUT_RETENTION_MAX_BYTES,
            strip_ansi: false,
            expect_stream: None,
        })
        .expect("the mark is never ahead of the stream")
    }

    #[test]
    fn a_read_mark_the_trim_passed_still_reads_the_whole_buffer_and_now_says_what_it_lost() {
        let mut b = OutputBuffer::new();
        b.append(b"head");
        b.set_mark();
        b.append(b"tail");
        assert_eq!(b.read_since_mark(false), "tail");
        assert_eq!(from_mark(&b).skipped, 0);

        // Overflow by more than the mark's offset, so the trim passes the mark.
        b.append(&filler(OUTPUT_RETENTION_MAX_BYTES));
        assert_eq!(
            b.read_since_mark(false).len(),
            OUTPUT_RETENTION_MAX_BYTES,
            "the text is what it always was — the whole retained buffer"
        );
        let read = from_mark(&b);
        assert_eq!(
            read.skipped, 4,
            "'tail' sat between the mark and the retained window and is gone \
             for good — that is what the read now says instead of starting \
             over in silence"
        );
        assert_eq!(read.cursor, read.retention_start);
    }

    #[test]
    fn a_read_mark_outside_the_trimmed_region_shifts_by_the_trimmed_amount() {
        let mut b = OutputBuffer::new();
        b.append(&filler(OUTPUT_RETENTION_MAX_BYTES));
        b.set_mark();
        b.append(b"after");
        // Trimmed 5 bytes off the front; the mark sat at OUTPUT_RETENTION_MAX_BYTES, well
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
        b.append(&filler(OUTPUT_RETENTION_MAX_BYTES));
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

    #[test]
    fn two_consumer_held_cursors_do_not_move_each_other_or_either_server_mark() {
        let mut b = OutputBuffer::new();
        b.append(b"one");
        let a1 = at(&b, 0);
        assert_eq!(a1.text, "one");
        b.append(b"two");

        // B reads the same region A already read. A's position is unaffected
        // because the buffer holds nothing on either consumer's behalf.
        let b1 = at(&b, 0);
        assert_eq!(b1.text, "onetwo");
        let a2 = at(&b, a1.next_cursor);
        assert_eq!(a2.text, "two", "B's read did not move A on");

        // And neither touched the two server-held cursors.
        assert_eq!(b.read_since_mark(false), "onetwo");
        assert_eq!(b.take_since_scan_mark(false), "onetwo");
    }

    #[test]
    fn a_cursor_the_trim_passed_reports_the_gap_instead_of_starting_over_in_silence() {
        let mut b = OutputBuffer::new();
        b.append(b"lost");
        let cursor = at(&b, 0).cursor;
        b.append(&filler(OUTPUT_RETENTION_MAX_BYTES));

        let read = at(&b, cursor);
        assert_eq!(
            read.skipped, 4,
            "the four bytes of 'lost' are gone for good"
        );
        assert_eq!(read.cursor, read.retention_start);
        assert_eq!(read.retention_start, 4);
        assert_eq!(read.retention_end, 4 + OUTPUT_RETENTION_MAX_BYTES as u64);
    }

    #[test]
    fn a_cursor_past_the_end_of_the_stream_is_refused_rather_than_answered_empty() {
        let mut b = OutputBuffer::new();
        b.append(b"abc");
        let err = b
            .read(&OutputReadRequest {
                from: OutputCursor::At(9),
                max_bytes: OUTPUT_RETENTION_MAX_BYTES,
                strip_ansi: false,
                expect_stream: None,
            })
            .expect_err(
                "answering empty would let the consumer wait for a \
                 continuation that, once the terminal produces six more bytes, \
                 is unrelated to anything it saw",
            );
        assert_eq!(
            err,
            OutputReadError::AheadOfStream {
                cursor: 9,
                retention_end: 3
            }
        );
        // The end itself is not past the end: that is "caught up".
        let caught_up = at(&b, 3);
        assert_eq!(caught_up.text, "");
        assert_eq!(caught_up.next_cursor, caught_up.retention_end);
    }

    #[test]
    fn a_cursor_carried_over_from_another_stream_is_refused() {
        let mut b = OutputBuffer::new();
        b.append(b"abc");
        let other = OutputBuffer::new();
        assert_ne!(
            b.stream, other.stream,
            "two buffers built back to back must not share a token — a reused \
             surface id would otherwise read as the same stream"
        );

        let err = b
            .read(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: OUTPUT_RETENTION_MAX_BYTES,
                strip_ansi: false,
                expect_stream: Some(other.stream.clone()),
            })
            .expect_err("the token names a stream this buffer is not");
        assert_eq!(
            err,
            OutputReadError::StreamMismatch {
                expected: other.stream.clone(),
                actual: b.stream.clone(),
            }
        );

        // The same position with this stream's own token is answerable.
        assert_eq!(
            b.read(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: OUTPUT_RETENTION_MAX_BYTES,
                strip_ansi: false,
                expect_stream: Some(b.stream.clone()),
            })
            .expect("own token")
            .text,
            "abc"
        );
    }

    #[test]
    fn max_bytes_caps_the_raw_region_and_the_next_cursor_counts_raw_bytes() {
        let mut b = OutputBuffer::new();
        b.append("\x1b[31mred\x1b[0m".as_bytes());
        let read = b
            .read(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: 8,
                strip_ansi: true,
                expect_stream: None,
            })
            .expect("inside the stream");
        assert_eq!(read.raw_bytes, 8);
        assert_eq!(read.next_cursor, 8);
        assert_eq!(
            read.text, "red",
            "stripping removed bytes, so the text is shorter than the region"
        );
        assert_ne!(
            read.text.len(),
            read.raw_bytes,
            "a consumer advancing by text.len() would desynchronise — this is \
             why next_cursor is a separate number"
        );
    }

    #[test]
    fn a_cap_landing_mid_character_backs_off_so_neither_side_loses_it() {
        let mut b = OutputBuffer::new();
        // 'ㄱ' is three bytes; ask for four so the cap lands inside the second.
        b.append("ㄱㄴ".as_bytes());
        let first = b
            .read(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: 4,
                strip_ansi: false,
                expect_stream: None,
            })
            .expect("inside the stream");
        assert_eq!(first.text, "ㄱ");
        assert_eq!(first.raw_bytes, 3, "backed off one byte");
        assert_eq!(at(&b, first.next_cursor).text, "ㄴ");
    }

    #[test]
    fn a_cap_smaller_than_the_next_character_still_makes_progress() {
        let mut b = OutputBuffer::new();
        b.append("ㄱ".as_bytes());
        let read = b
            .read(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: 1,
                strip_ansi: false,
                expect_stream: None,
            })
            .expect("inside the stream");
        assert_eq!(
            read.raw_bytes, 1,
            "backing off would take nothing, and a read that takes nothing \
             while bytes are waiting never finishes"
        );
        assert_eq!(read.next_cursor, 1);
    }

    #[test]
    fn max_bytes_is_clamped_at_both_ends() {
        let mut b = OutputBuffer::new();
        b.append(b"abcdef");
        let zero = b
            .read(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: 0,
                strip_ansi: false,
                expect_stream: None,
            })
            .expect("inside the stream");
        assert_eq!(
            zero.raw_bytes, 1,
            "0 would make every read empty and the consumer would never advance"
        );
        let huge = b
            .read(&OutputReadRequest {
                from: OutputCursor::At(0),
                max_bytes: usize::MAX,
                strip_ansi: false,
                expect_stream: None,
            })
            .expect("inside the stream");
        assert_eq!(huge.raw_bytes, 6, "nothing beyond what is retained exists");
    }

    #[test]
    fn positions_keep_counting_across_a_trim() {
        let mut b = OutputBuffer::new();
        b.append(&filler(OUTPUT_RETENTION_MAX_BYTES));
        assert_eq!(at(&b, 0).retention_start, 0);
        b.append(b"tail");
        let read = at(&b, 4);
        assert_eq!(read.retention_start, 4, "the window moved, not the numbers");
        assert_eq!(
            read.retention_end,
            4 + OUTPUT_RETENTION_MAX_BYTES as u64,
            "a position that once named a retained byte never names a different \
             one later"
        );
    }
}
