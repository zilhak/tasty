//! VTE handler: osc 도메인.


use termwiz::escape::csi::Device;
use termwiz::escape::OperatingSystemCommand;

use crate::{Terminal, TerminalEvent, TerminalEventKind};

impl Terminal {
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
            OperatingSystemCommand::SetSelection(_selection, data) => {
                self.events.push(TerminalEvent {
                    surface_id: 0,
                    kind: TerminalEventKind::ClipboardSet(data),
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

    pub(crate) fn handle_device(&mut self, device: Device) {
        match device {
            Device::StatusReport => self.send_terminal_response("\x1b[0n"),
            Device::RequestPrimaryDeviceAttributes => {
                self.send_terminal_response("\x1b[?1;2c");
            }
            _ => {}
        }
    }

}
