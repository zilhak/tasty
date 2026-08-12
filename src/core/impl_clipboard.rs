//! `Core` — OSC 52 클립보드 read/write. `src/core/mod.rs` 의 `impl Core` 분할.

use super::*;

impl Core {
    /// OSC 52 clipboard read query. Security gate: off by default so an
    /// arbitrary (possibly remote/untrusted) program cannot silently read the
    /// local clipboard. When off, send nothing — no reply byte must leave the
    /// host. Handled here (not via a cascade) like the ClipboardSet write,
    /// since both need the `self.clipboard` port together with the terminal
    /// engine.
    pub(super) fn handle_clipboard_query(&mut self, engine: &mut crate::core::CoreState, sid: u32) {
        let allow = engine.settings.general.allow_clipboard_read;
        // Only touch the clipboard when allowed (default off → never read).
        let clip = if allow {
            match self.clipboard.read_text() {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("OSC 52 clipboard read failed: {e}");
                    None
                }
            }
        } else {
            None
        };
        if let Some(reply) = osc52_clipboard_read_reply(allow, clip.as_deref())
            && let Some(terminal) = engine.find_terminal_by_id_mut(sid)
        {
            terminal.send_bytes(&reply);
        }
    }
}

/// Build the OSC 52 read reply (`OSC 52 ; c ; <base64> ST`) for a clipboard
/// query, or `None` when no bytes must be emitted. Returns `None` when `allow`
/// is false (the security gate, off by default) or the clipboard had no text.
/// Isolating this keeps the "off → zero output" invariant unit-testable without
/// constructing a full `Core`.
fn osc52_clipboard_read_reply(allow: bool, clipboard_text: Option<&str>) -> Option<Vec<u8>> {
    if !allow {
        return None;
    }
    let text = clipboard_text?;
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    Some(format!("\x1b]52;c;{encoded}\x07").into_bytes())
}

#[cfg(test)]
mod osc52_clipboard_read_tests {
    use super::osc52_clipboard_read_reply;

    #[test]
    fn off_emits_no_bytes() {
        // Security invariant: a disallowed read query produces zero bytes, even
        // when the clipboard has content.
        assert_eq!(osc52_clipboard_read_reply(false, Some("secret")), None);
        assert_eq!(osc52_clipboard_read_reply(false, None), None);
    }

    #[test]
    fn on_encodes_clipboard_as_osc52() {
        // "hi" → base64 "aGk=", wrapped in `OSC 52 ; c ; <b64> BEL`.
        let reply = osc52_clipboard_read_reply(true, Some("hi")).expect("reply when allowed");
        assert_eq!(reply, b"\x1b]52;c;aGk=\x07".to_vec());
    }

    #[test]
    fn on_with_empty_clipboard_still_replies() {
        // An allowed query with no clipboard text resolves to None (nothing to
        // read), distinct from the gated-off case but also emitting no bytes.
        assert_eq!(osc52_clipboard_read_reply(true, None), None);
    }
}
