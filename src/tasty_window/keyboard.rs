use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::TastyWindow;

impl TastyWindow {
    pub(super) fn handle_keyboard_input(&mut self, event: &winit::event::KeyEvent, _egui_consumed: bool) {
        if event.state != ElementState::Pressed {
            return;
        }

        if event.logical_key == Key::Named(NamedKey::Escape) {
            if self.state.settings_open {
                self.state.settings_open = false;
                self.state.settings_ui_state = crate::settings_ui::SettingsUiState::new();
                self.mark_dirty();
                return;
            }
            if self.state.notification_panel_open {
                self.state.notification_panel_open = false;
                self.mark_dirty();
                return;
            }
        }

        // Notification panel is a popup (not a modal) — it does NOT block input.
        // Only true modals (settings) block keyboard input to the terminal.
        let overlay_open = self.state.settings_open;

        if !overlay_open {
            // On macOS, IME composition (e.g. Korean) can replace the logical key
            // with the composed character. When modifier keys are held, use the
            // physical key code to determine the intended key for shortcut matching.
            let shortcut_key = if self.modifiers.control_key() || self.modifiers.super_key() || self.modifiers.alt_key() {
                crate::shortcuts::physical_key_to_logical(&event.physical_key)
                    .unwrap_or_else(|| event.logical_key.clone())
            } else {
                event.logical_key.clone()
            };
            if self.handle_shortcut(&shortcut_key, self.modifiers) {
                self.mark_dirty();
                return;
            }
        }
        if overlay_open {
            return;
        }
        // Note: egui_consumed is intentionally NOT checked here for keyboard events.
        // egui consumes Ctrl+C/V/X as clipboard shortcuts, but when a terminal is
        // focused these keys must reach the terminal (e.g. Ctrl+C → SIGINT).
        // egui UI elements (settings, dialogs) are guarded by overlay_open above.

        // Forward to terminal
        // When IME is active, skip text sending — let Ime::Commit handle it.
        // This prevents double input when switching to Korean/Chinese/Japanese IME.
        // When IME is active, suppress non-ASCII text (Korean/Chinese/Japanese
        // composition — Ime::Commit will handle it). ASCII text (numbers,
        // punctuation like 1234567890,./) passes through IME unchanged and
        // won't generate Ime::Commit, so we must send it here.
        //
        let text_for_terminal = if self.ime_active {
            match &event.text {
                Some(t) if t.as_str().is_ascii() => &event.text,
                _ => &None,
            }
        } else {
            &event.text
        };
        let typing_surface_id = self.state.focused_surface_id();
        if let Some(terminal) = self.state.focused_terminal_mut() {
            let (dirty, sent) = Self::send_key_to_terminal(terminal, &event.logical_key, text_for_terminal, self.modifiers);
            if dirty { self.dirty = true; }

            // Clear selection only when actual content was sent to the terminal PTY
            if sent && self.text_selection.is_some() {
                self.text_selection = None;
                self.dirty = true;
            }
        }
        if let Some(sid) = typing_surface_id {
            self.state.record_typing(sid);
        }
    }

    /// Send a key to the terminal. Returns (dirty, sent) where `sent` indicates
    /// whether any bytes were actually written to the terminal PTY.
    fn send_key_to_terminal(
        terminal: &mut tasty_terminal::Terminal,
        key: &Key,
        text: &Option<winit::keyboard::SmolStr>,
        modifiers: ModifiersState,
    ) -> (bool, bool) {
        let app_cursor = terminal.application_cursor_keys();
        let is_alt_screen = terminal.is_alternate_screen();
        let mut dirty = false;
        let mut sent = false;

        let is_scrollback_key = !is_alt_screen && matches!(
            key.as_ref(),
            Key::Named(NamedKey::PageUp) | Key::Named(NamedKey::PageDown)
        );

        match key.as_ref() {
            Key::Named(NamedKey::Enter) => {
                if modifiers.shift_key() {
                    // Kitty keyboard protocol: CSI 13 ; 2 u (Shift+Enter)
                    terminal.send_bytes(b"\x1b[13;2u");
                } else {
                    terminal.send_bytes(b"\r");
                }
                sent = true;
            }
            Key::Named(NamedKey::Backspace) => { terminal.send_bytes(b"\x7f"); sent = true; }
            Key::Named(NamedKey::Tab) => {
                if modifiers.shift_key() { terminal.send_bytes(b"\x1b[Z"); }
                else { terminal.send_bytes(b"\t"); }
                sent = true;
            }
            Key::Named(NamedKey::Escape) => { terminal.send_bytes(b"\x1b"); sent = true; }
            Key::Named(NamedKey::ArrowUp) => {
                if app_cursor { terminal.send_bytes(b"\x1bOA") } else { terminal.send_bytes(b"\x1b[A") }
                sent = true;
            }
            Key::Named(NamedKey::ArrowDown) => {
                if app_cursor { terminal.send_bytes(b"\x1bOB") } else { terminal.send_bytes(b"\x1b[B") }
                sent = true;
            }
            Key::Named(NamedKey::ArrowRight) => {
                if app_cursor { terminal.send_bytes(b"\x1bOC") } else { terminal.send_bytes(b"\x1b[C") }
                sent = true;
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if app_cursor { terminal.send_bytes(b"\x1bOD") } else { terminal.send_bytes(b"\x1b[D") }
                sent = true;
            }
            Key::Named(NamedKey::Home) => { terminal.send_bytes(b"\x1b[H"); sent = true; }
            Key::Named(NamedKey::End) => { terminal.send_bytes(b"\x1b[F"); sent = true; }
            Key::Named(NamedKey::PageUp) => {
                if is_alt_screen { terminal.send_bytes(b"\x1b[5~"); sent = true; }
                else { terminal.scroll_up(terminal.rows()); dirty = true; }
            }
            Key::Named(NamedKey::PageDown) => {
                if is_alt_screen { terminal.send_bytes(b"\x1b[6~"); sent = true; }
                else { terminal.scroll_down(terminal.rows()); dirty = true; }
            }
            Key::Named(NamedKey::Insert) => { terminal.send_bytes(b"\x1b[2~"); sent = true; }
            Key::Named(NamedKey::Delete) => { terminal.send_bytes(b"\x1b[3~"); sent = true; }
            Key::Named(NamedKey::F1) => { terminal.send_bytes(b"\x1bOP"); sent = true; }
            Key::Named(NamedKey::F2) => { terminal.send_bytes(b"\x1bOQ"); sent = true; }
            Key::Named(NamedKey::F3) => { terminal.send_bytes(b"\x1bOR"); sent = true; }
            Key::Named(NamedKey::F4) => { terminal.send_bytes(b"\x1bOS"); sent = true; }
            Key::Named(NamedKey::F5) => { terminal.send_bytes(b"\x1b[15~"); sent = true; }
            Key::Named(NamedKey::F6) => { terminal.send_bytes(b"\x1b[17~"); sent = true; }
            Key::Named(NamedKey::F7) => { terminal.send_bytes(b"\x1b[18~"); sent = true; }
            Key::Named(NamedKey::F8) => { terminal.send_bytes(b"\x1b[19~"); sent = true; }
            Key::Named(NamedKey::F9) => { terminal.send_bytes(b"\x1b[20~"); sent = true; }
            Key::Named(NamedKey::F10) => { terminal.send_bytes(b"\x1b[21~"); sent = true; }
            Key::Named(NamedKey::F11) => { terminal.send_bytes(b"\x1b[23~"); sent = true; }
            Key::Named(NamedKey::F12) => { terminal.send_bytes(b"\x1b[24~"); sent = true; }
            _ => {
                // Ctrl+letter → send control character (0x01-0x1A)
                if modifiers.control_key() && !modifiers.alt_key() {
                    if let Key::Character(c) = key {
                        if let Some(ch) = c.chars().next() {
                            if ch.is_ascii_alphabetic() {
                                let ctrl_char = (ch.to_ascii_lowercase() as u8) - b'a' + 1;
                                terminal.send_bytes(&[ctrl_char]);
                                sent = true;
                                return (dirty, sent);
                            }
                        }
                    }
                }
                if let Some(text) = text {
                    let s = text.as_str();
                    if !s.is_empty() { terminal.send_key(s); sent = true; }
                }
            }
        }
        // Scroll to bottom only when actual content was sent to the terminal,
        // not on modifier-only keypresses (Ctrl, Cmd, Shift, Alt).
        if sent && !is_scrollback_key && terminal.scroll_offset > 0 {
            terminal.scroll_to_bottom();
            dirty = true;
        }

        (dirty, sent)
    }

    pub(super) fn handle_ime(&mut self, ime_event: winit::event::Ime, egui_consumed: bool) {
        if egui_consumed { self.mark_dirty(); return; }
        match ime_event {
            winit::event::Ime::Enabled => {
                self.ime_active = true;
            }
            winit::event::Ime::Disabled => {
                self.ime_active = false;
                self.clear_ime_preedit();
            }
            winit::event::Ime::Preedit(text, cursor) => {
                if text.is_empty() {
                    self.clear_ime_preedit();
                } else {
                    let surface_id = self.state.focused_surface_id();
                    let cursor_pos = self.state.focused_terminal().map(|terminal| terminal.surface().cursor_position());
                    self.ime_preedit = match (surface_id, cursor_pos) {
                        (Some(surface_id), Some((anchor_col, anchor_row))) => Some(crate::gpu::ImePreeditState {
                            text,
                            cursor,
                            anchor_col,
                            anchor_row,
                            surface_id,
                        }),
                        _ => None,
                    };
                    self.update_ime_cursor_area();
                }
                self.mark_dirty();
            }
            winit::event::Ime::Commit(text) => {
                self.clear_ime_preedit();
                let sid = self.state.focused_surface_id();
                if let Some(terminal) = self.state.focused_terminal_mut() {
                    terminal.send_key(&text);
                }
                if let Some(sid) = sid {
                    self.state.record_typing(sid);
                }
                self.mark_dirty();
            }
        }
    }
}
