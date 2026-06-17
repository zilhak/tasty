//! VTE handler: osc 도메인.

use std::sync::Arc;

use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::escape::OperatingSystemCommand;
use termwiz::escape::csi::Device;
use termwiz::escape::osc::ColorOrQuery;
use termwiz::surface::{Change, CursorVisibility};

use crate::{TerminalEvent, TerminalEventKind, TerminalState};

impl TerminalState {
    pub(crate) fn map_osc(&mut self, osc: OperatingSystemCommand) {
        match osc {
            OperatingSystemCommand::SetIconNameAndWindowTitle(title) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::TitleChanged(title),
                });
            }
            OperatingSystemCommand::SetWindowTitle(title)
            | OperatingSystemCommand::SetWindowTitleSun(title) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::TitleChanged(title),
                });
            }
            OperatingSystemCommand::CurrentWorkingDirectory(url) => {
                let path = if let Some(stripped) = url.strip_prefix("file://") {
                    if let Some(slash_pos) = stripped.find('/') {
                        stripped[slash_pos..].to_string()
                    } else {
                        stripped.to_string()
                    }
                } else {
                    url.clone()
                };
                // On Windows, OSC 7 paths arrive as "/C:/foo/bar" (URI form);
                // strip the leading slash so PathBuf yields a valid drive path.
                #[cfg(windows)]
                let path = {
                    let bytes = path.as_bytes();
                    if bytes.len() >= 4
                        && bytes[0] == b'/'
                        && bytes[2] == b':'
                        && bytes[3] == b'/'
                        && bytes[1].is_ascii_alphabetic()
                    {
                        path[1..].replace('/', "\\")
                    } else {
                        path
                    }
                };
                // Cache the CWD so get_cwd() can return it instantly without
                // spawning an external process (critical on Windows).
                self.cached_cwd = Some(std::path::PathBuf::from(&path));
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::CwdChanged(path),
                });
            }
            OperatingSystemCommand::SystemNotification(body) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::Notification {
                        title: "Terminal".to_string(),
                        body,
                    },
                });
            }
            OperatingSystemCommand::RxvtExtension(parts) => {
                if parts.first().map(|s| s.as_str()) == Some("notify") {
                    let title = parts.get(1).cloned().unwrap_or_default();
                    let body = parts.get(2).cloned().unwrap_or_default();
                    self.events.push(TerminalEvent {
                        surface_id: 0,
                        kind: TerminalEventKind::Notification { title, body },
                    });
                }
            }
            // OSC 8 (SetHyperlink): state-transition command. An open
            // (`OSC 8 ; params ; URI`) attaches the hyperlink to every cell
            // printed afterward; a close (`OSC 8 ; ;`, empty URI) clears it.
            // Emitting it as an attribute change lets termwiz's surface pen
            // hold the state, so subsequent Print cells inherit it automatically
            // — no separate current_hyperlink field is needed. FullReset/DECSTR
            // clear it via AllAttributes(default), which zeroes the pen hyperlink.
            OperatingSystemCommand::SetHyperlink(opt) => {
                self.apply_or_stage_change(Change::Attribute(AttributeChange::Hyperlink(
                    opt.map(Arc::new),
                )));
            }
            // OSC 10/11/12 (… up to 19): dynamic colors. `OSC 10 ; spec ; spec …`
            // assigns fg, then bg, then cursor in sequence, so the n-th entry maps
            // to color number `base + n`. Only *queries* (`?`) are answered — with
            // the plumbed theme RGB; set requests are ignored (no palette-override
            // storage yet — H3 scope). Color numbers tasty has no source for
            // (13..=19) are left unanswered. The response reflects the OSC number
            // and uses ST termination (xterm convention; neovim rejects the BEL
            // form).
            OperatingSystemCommand::ChangeDynamicColors(first_color, colors) => {
                let Some(palette) = self.color_palette.clone() else {
                    return;
                };
                let base = first_color as u8;
                for (offset, color) in colors.iter().enumerate() {
                    if !matches!(color, ColorOrQuery::Query) {
                        continue;
                    }
                    let number = base + offset as u8;
                    if let Some(rgb) = palette.dynamic_color(number) {
                        self.send_terminal_response(&format!(
                            "\x1b]{};{}\x1b\\",
                            number,
                            rgb.to_x11_16bit()
                        ));
                    }
                }
            }
            // OSC 4: ANSI palette colors. Each pair queries one palette index;
            // a query (`?`) is answered with the theme RGB for that index. Indices
            // outside the 16 theme-defined ANSI colors (the fixed xterm cube/ramp)
            // are left unanswered. Set requests are ignored (H3 scope).
            OperatingSystemCommand::ChangeColorNumber(pairs) => {
                let Some(palette) = self.color_palette.clone() else {
                    return;
                };
                for pair in &pairs {
                    if !matches!(pair.color, ColorOrQuery::Query) {
                        continue;
                    }
                    if let Some(rgb) = palette.ansi_color(pair.palette_index) {
                        self.send_terminal_response(&format!(
                            "\x1b]4;{};{}\x1b\\",
                            pair.palette_index,
                            rgb.to_x11_16bit()
                        ));
                    }
                }
            }
            OperatingSystemCommand::SetSelection(_selection, data) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::ClipboardSet(data),
                });
            }
            // OSC 52 read query (`OSC 52 ; c ; ? ST`). Emit an event only — the
            // terminal crate has no access to the system clipboard or the user's
            // privacy setting. The host decides whether to answer (gated on
            // `allow_clipboard_read`, default off → no reply). The selection kind
            // is ignored; the host always answers for the clipboard (`c`).
            OperatingSystemCommand::QuerySelection(_selection) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::ClipboardQuery,
                });
            }
            OperatingSystemCommand::Unspecified(params) => {
                if let Some(first) = params.first() {
                    if first == b"133" {
                        // OSC 133 ; <A|B|C|D> [; payload ...] (BEL or ST).
                        // params[0] = "133", params[1] = "A"/"B"/"C"/"D" (or with payload),
                        // params[2..] = extra payload tokens (often `cmd=...`, exit_code, etc).
                        if let Some(second) = params.get(1)
                            && let Some(&phase_byte) = second.first()
                            && matches!(phase_byte, b'A' | b'B' | b'C' | b'D')
                        {
                            let phase = phase_byte as char;
                            // Build payload as `<second_rest>[;param2][;param3]...`
                            let mut payload = String::new();
                            let second_str = String::from_utf8_lossy(second);
                            if second_str.len() > 1 {
                                // e.g. second = "D;0" → second_rest = "0"
                                payload.push_str(&second_str[1..]);
                                // Strip a leading ';' if present so the payload is
                                // semicolon-joined without sentinels.
                                payload = payload.trim_start_matches(';').to_string();
                            }
                            for extra in params.iter().skip(2) {
                                if !payload.is_empty() {
                                    payload.push(';');
                                }
                                payload.push_str(&String::from_utf8_lossy(extra));
                            }
                            self.events.push(TerminalEvent {
                                surface_id: 0,
                                kind: TerminalEventKind::PromptBoundary { phase, payload },
                            });
                        }
                        return;
                    }
                    if first == b"99" {
                        let mut title = String::new();
                        let mut body = String::new();
                        for param in params.iter().skip(1) {
                            let s = String::from_utf8_lossy(param);
                            if let Some(val) = s.strip_prefix("t=") {
                                title = val.to_string();
                            } else if let Some(val) = s.strip_prefix("d=0;") {
                                body = val.to_string();
                            } else if let Some(val) = s.strip_prefix("d=1;") {
                                body = val.to_string();
                            } else if !s.contains('=') {
                                body = s.to_string();
                            }
                        }
                        if title.is_empty() {
                            title = "Terminal".to_string();
                        }
                        self.events.push(TerminalEvent {
                            surface_id: 0,
                            kind: TerminalEventKind::Notification { title, body },
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// XtGetTcap (`DCS + q Pt ST`): a request for termcap/terminfo capability
    /// strings, where `Pt` is one or more hex-encoded capability names joined by
    /// `;` (termwiz decodes them to ASCII for us). tasty does not yet expose a
    /// capability database, so every queried cap is answered with the invalid /
    /// unsupported reply `DCS 0 + r <hexcap> ST` (status 0), echoing the
    /// requested name back as hex. This says "that cap is currently not provided"
    /// — distinct from silence, which leaves the querying app waiting on a
    /// timeout. This is a *current* limitation; real cap answers (status 1,
    /// `DCS 1 + r <cap>=<hexvalue> ST`) may be added later.
    pub(crate) fn handle_xtgettcap(&mut self, names: &[String]) {
        for name in names {
            let mut response = String::from("\x1bP0+r");
            for &b in name.as_bytes() {
                response.push_str(&format!("{b:02x}"));
            }
            response.push_str("\x1b\\");
            self.send_terminal_response(&response);
        }
    }

    pub(crate) fn handle_device(&mut self, device: Device) {
        match device {
            Device::StatusReport => self.send_terminal_response("\x1b[0n"),
            Device::RequestPrimaryDeviceAttributes => {
                self.send_terminal_response("\x1b[?1;2c");
            }
            // DA2 (CSI > c): secondary device attributes — `CSI > Pp ; Pv ; Pc c`.
            // Pp = terminal type (0, xterm-compatible class), Pv = version code,
            // Pc = ROM cartridge (0). Apps identify the terminal by name via
            // XTVERSION; DA2 just needs to answer with a recognizable triple.
            Device::RequestSecondaryDeviceAttributes => {
                self.send_terminal_response("\x1b[>0;10;0c");
            }
            // XTVERSION (CSI > q): terminal name and version, reported as a DCS
            // string `DCS > | tasty(<version>) ST`. The name "tasty" is the
            // identifier modern apps (neovim, tmux) key on. The version is the
            // tasty-terminal crate version (policy: identification, not exact
            // build); ST (ESC \) terminator per xterm convention.
            Device::RequestTerminalNameAndVersion => {
                self.send_terminal_response(concat!(
                    "\x1bP>|tasty(",
                    env!("CARGO_PKG_VERSION"),
                    ")\x1b\\"
                ));
            }
            // DA3 (CSI = c): tertiary device attributes — `DCS ! | <unit id> ST`.
            // Unit id is a fixed hex string (very rarely queried).
            Device::RequestTertiaryDeviceAttributes => {
                self.send_terminal_response("\x1bP!|54415354\x1b\\");
            }
            // DECSTR (CSI ! p): soft terminal reset. Unlike RIS (ESC c), this does
            // NOT clear the screen, switch the alternate screen, or touch the
            // palette/tab stops — screen content and alt-screen state are preserved.
            // It restores the cursor/SGR/mode subset apps rely on at init time.
            Device::SoftReset => {
                // Margins → full screen (DECSTBM).
                self.scroll_region = None;
                // Saved cursor → cleared (DECSC store).
                self.saved_cursor = None;
                // Application cursor keys (DECCKM) → reset.
                self.application_cursor_keys = false;
                // Insert/replace mode (IRM) → replace.
                self.insert_mode = false;
                // Text cursor enable (DECTCEM) → visible.
                self.cursor_visible = true;
                // DECSCUSR cursor shape is intentionally NOT reset by DECSTR
                // (matches xterm — only RIS restores the default shape).
                // SGR → default, cursor → visible. Applied via the surface since
                // handle_device returns (); screen content stays intact.
                self.apply_or_stage_change(Change::AllAttributes(CellAttributes::default()));
                self.apply_or_stage_change(Change::CursorVisibility(CursorVisibility::Visible));
            }
            _ => {}
        }
    }
}
