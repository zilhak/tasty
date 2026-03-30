use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::tasty_window::TastyWindow;
use crate::model::SplitDirection;

/// Convert a physical key code to a Key::Character for shortcut matching.
/// On macOS, when IME is composing (e.g. Korean), logical_key may contain
/// the composed character (e.g. "ㅇ" instead of "d"). This function extracts
/// the intended key from the physical key code.
pub(crate) fn physical_key_to_logical(physical: &PhysicalKey) -> Option<Key> {
    let code = match physical {
        PhysicalKey::Code(c) => c,
        _ => return None,
    };
    let ch: &str = match code {
        KeyCode::KeyA => "a", KeyCode::KeyB => "b", KeyCode::KeyC => "c",
        KeyCode::KeyD => "d", KeyCode::KeyE => "e", KeyCode::KeyF => "f",
        KeyCode::KeyG => "g", KeyCode::KeyH => "h", KeyCode::KeyI => "i",
        KeyCode::KeyJ => "j", KeyCode::KeyK => "k", KeyCode::KeyL => "l",
        KeyCode::KeyM => "m", KeyCode::KeyN => "n", KeyCode::KeyO => "o",
        KeyCode::KeyP => "p", KeyCode::KeyQ => "q", KeyCode::KeyR => "r",
        KeyCode::KeyS => "s", KeyCode::KeyT => "t", KeyCode::KeyU => "u",
        KeyCode::KeyV => "v", KeyCode::KeyW => "w", KeyCode::KeyX => "x",
        KeyCode::KeyY => "y", KeyCode::KeyZ => "z",
        KeyCode::Digit0 => "0", KeyCode::Digit1 => "1", KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3", KeyCode::Digit4 => "4", KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6", KeyCode::Digit7 => "7", KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Minus => "-", KeyCode::Equal => "=",
        KeyCode::BracketLeft => "[", KeyCode::BracketRight => "]",
        KeyCode::Semicolon => ";", KeyCode::Quote => "'",
        KeyCode::Backquote => "`", KeyCode::Backslash => "\\",
        KeyCode::Comma => ",", KeyCode::Period => ".", KeyCode::Slash => "/",
        _ => return None,
    };
    Some(Key::Character(ch.into()))
}

/// Parse a binding string like "ctrl+shift+n" and check if it matches
/// the given key + modifiers. Returns false for empty bindings.
fn matches_binding(binding: &str, key: &Key, mods: ModifiersState) -> bool {
    if binding.is_empty() {
        return false;
    }

    let parts: Vec<&str> = binding.split('+').collect();
    if parts.is_empty() {
        return false;
    }

    // Extract expected modifiers and the key part (last non-modifier token)
    let mut expect_ctrl = false;
    let mut expect_shift = false;
    let mut expect_alt = false;
    let mut key_part = "";

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" => expect_ctrl = true,
            "shift" => expect_shift = true,
            "alt" => expect_alt = true,
            _ => key_part = part,
        }
    }

    // Check modifiers match exactly.
    // On macOS, "alt" in binding maps to Cmd (super_key) since the physical
    // position of Cmd on macOS keyboards matches Alt on Windows/Linux keyboards.
    #[cfg(target_os = "macos")]
    let alt_matches = mods.super_key() == expect_alt;
    #[cfg(not(target_os = "macos"))]
    let alt_matches = mods.alt_key() == expect_alt;

    if mods.control_key() != expect_ctrl
        || mods.shift_key() != expect_shift
        || !alt_matches
    {
        return false;
    }

    // Match the key part
    let key_lower = key_part.to_lowercase();
    match key {
        Key::Character(c) => {
            let ch = c.to_lowercase();
            if ch == key_lower {
                return true;
            }
            // Ctrl+letter may arrive as control character (0x01-0x1A).
            // Convert back to the letter for matching.
            if expect_ctrl && c.len() == 1 {
                let byte = c.as_bytes()[0];
                if byte >= 1 && byte <= 26 {
                    let letter = ((byte - 1) + b'a') as char;
                    return letter.to_string() == key_lower;
                }
            }
            false
        }
        Key::Named(named) => {
            let named_str = named_key_to_string(named);
            named_str == key_lower
        }
        _ => false,
    }
}

fn named_key_to_string(key: &NamedKey) -> String {
    match key {
        NamedKey::Tab => "tab".into(),
        NamedKey::Space => "space".into(),
        NamedKey::Enter => "enter".into(),
        NamedKey::Backspace => "backspace".into(),
        NamedKey::Delete => "delete".into(),
        NamedKey::Insert => "insert".into(),
        NamedKey::Home => "home".into(),
        NamedKey::End => "end".into(),
        NamedKey::PageUp => "pageup".into(),
        NamedKey::PageDown => "pagedown".into(),
        NamedKey::ArrowUp => "up".into(),
        NamedKey::ArrowDown => "down".into(),
        NamedKey::ArrowLeft => "left".into(),
        NamedKey::ArrowRight => "right".into(),
        NamedKey::F1 => "f1".into(),
        NamedKey::F2 => "f2".into(),
        NamedKey::F3 => "f3".into(),
        NamedKey::F4 => "f4".into(),
        NamedKey::F5 => "f5".into(),
        NamedKey::F6 => "f6".into(),
        NamedKey::F7 => "f7".into(),
        NamedKey::F8 => "f8".into(),
        NamedKey::F9 => "f9".into(),
        NamedKey::F10 => "f10".into(),
        NamedKey::F11 => "f11".into(),
        NamedKey::F12 => "f12".into(),
        NamedKey::Escape => "escape".into(),
        _ => String::new(),
    }
}

impl TastyWindow {
    /// Handle keyboard shortcuts. Returns true if the event was consumed by a shortcut.
    pub(crate) fn handle_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        // On macOS, "alt" maps to Cmd (super_key) — physical position match.
        #[cfg(target_os = "macos")]
        let alt = mods.super_key();
        #[cfg(not(target_os = "macos"))]
        let alt = mods.alt_key();

        // Pre-compute values that need &self before borrowing &mut self.state
        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.gpu.cell_width();
        let cell_h = self.gpu.cell_height();

        // Clipboard copy shortcuts (handled before state borrow to avoid borrow conflict)
        // Linux style: Ctrl+Shift+C
        if ctrl && shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "C" || c.as_str() == "c")
                    && self.state.engine.settings.clipboard.linux_style
                {
                    if self.copy_selection_to_clipboard() {
                        self.mark_dirty();
                        return true;
                    }
                }
            }
        }
        // Windows style: Ctrl+C — copy if selection exists, else let SIGINT through
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                if (c.as_str() == "c" || c.as_str() == "C" || c.as_str() == "\x03")
                    && self.state.engine.settings.clipboard.windows_style
                {
                    if self.copy_selection_to_clipboard() {
                        self.mark_dirty();
                        return true;
                    }
                    // No selection — fall through so \x03 reaches terminal as SIGINT
                }
            }
        }
        // macOS style: Alt+C
        if alt && !ctrl && !shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "c" || c.as_str() == "C")
                    && self.state.engine.settings.clipboard.macos_style
                {
                    if self.copy_selection_to_clipboard() {
                        self.mark_dirty();
                        return true;
                    }
                }
            }
        }

        let state = &mut self.state;
        let kb = state.engine.settings.keybindings.clone();

        if matches_binding(&kb.new_workspace, key, mods) {
            let _ = state.add_workspace();
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.new_tab, key, mods) {
            let _ = state.add_tab();
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.split_pane_vertical, key, mods) {
            let _ = state.split_pane(SplitDirection::Vertical);
            state.resize_all(terminal_rect, cell_w, cell_h);
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.split_pane_horizontal, key, mods) {
            let _ = state.split_pane(SplitDirection::Horizontal);
            state.resize_all(terminal_rect, cell_w, cell_h);
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.split_surface_vertical, key, mods) {
            let _ = state.split_surface(SplitDirection::Vertical);
            state.resize_all(terminal_rect, cell_w, cell_h);
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.split_surface_horizontal, key, mods) {
            let _ = state.split_surface(SplitDirection::Horizontal);
            state.resize_all(terminal_rect, cell_w, cell_h);
            self.mark_dirty();
            return true;
        }

        // --- Configurable shortcuts (previously hardcoded) ---

        // Open settings modal
        if matches_binding(&kb.toggle_settings, key, mods) {
            let _ = self.proxy.send_event(crate::AppEvent::OpenSettings);
            return true;
        }

        // Toggle notification panel
        if matches_binding(&kb.toggle_notifications, key, mods) {
            state.notification_panel_open = !state.notification_panel_open;
            if state.notification_panel_open {
                state.engine.notifications.mark_all_read();
            }
            self.mark_dirty();
            return true;
        }

        // Close active pane
        if matches_binding(&kb.close_pane, key, mods) {
            if state.close_active_pane() {
                    state.resize_all(terminal_rect, cell_w, cell_h);
                self.mark_dirty();
                return true;
            }
            return false;
        }

        // Close active surface (fallback to close pane if only one surface remains)
        if matches_binding(&kb.close_surface, key, mods) {
            if state.close_active_surface() {
                    state.resize_all(terminal_rect, cell_w, cell_h);
                self.mark_dirty();
                return true;
            }
            // Surface was the only one in the group — try closing the pane instead
            if state.close_active_pane() {
                state.resize_all(terminal_rect, cell_w, cell_h);
                self.mark_dirty();
                return true;
            }
            return false;
        }

        // Focus pane next/prev
        if matches_binding(&kb.focus_pane_next, key, mods) {
            state.move_pane_focus_forward();
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.focus_pane_prev, key, mods) {
            state.move_pane_focus_backward();
            self.mark_dirty();
            return true;
        }

        // Focus surface next/prev
        if matches_binding(&kb.focus_surface_next, key, mods) {
            state.move_surface_focus_forward();
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.focus_surface_prev, key, mods) {
            state.move_surface_focus_backward();
            self.mark_dirty();
            return true;
        }

        // Sidebar toggle (open/close)
        if matches_binding(&kb.toggle_sidebar, key, mods) {
            state.sidebar_visible = !state.sidebar_visible;
            self.mark_dirty();
            return true;
        }

        // Sidebar collapse (full/compact)
        if matches_binding(&kb.toggle_sidebar_collapse, key, mods) {
            state.sidebar_collapsed = !state.sidebar_collapsed;
            self.mark_dirty();
            return true;
        }

        // --- Hardcoded shortcuts (not user-configurable) ---

        // Ctrl+Shift+Tab: previous tab in focused pane
        if ctrl && shift {
            if let Key::Named(NamedKey::Tab) = key {
                state.prev_tab_in_pane();
                self.mark_dirty();
                return true;
            }
        }

        // Ctrl+W: Close active tab
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                let s = c.as_str();
                if s == "w" || s == "W" || s == "\u{17}" {
                    if state.close_active_tab() {
                        self.mark_dirty();
                        return true;
                    }
                    return false;
                }
            }
        }

        // Ctrl+Tab: next tab in focused pane
        if ctrl && !shift && !alt {
            if let Key::Named(NamedKey::Tab) = key {
                state.next_tab_in_pane();
                self.mark_dirty();
                return true;
            }
        }

        // Configurable modifier+1~0: switch tab in focused pane
        // Configurable modifier+1~9: switch workspace
        {
            let tab_mod = kb.tab_switch_modifier.to_lowercase();
            let ws_mod = kb.workspace_switch_modifier.to_lowercase();

            if let Key::Character(c) = key {
                let ch = c.chars().next().unwrap_or('\0');
                if ch.is_ascii_digit() {
                    // Tab switch: modifier + number
                    let tab_mod_matches = match tab_mod.as_str() {
                        "alt" => alt && !ctrl && !shift,
                        _ => ctrl && !shift && !alt,  // default: ctrl
                    };
                    if tab_mod_matches {
                        let index = if ch == '0' { 9 } else { (ch as usize) - ('1' as usize) };
                        if state.goto_tab_in_pane(index) {
                            self.mark_dirty();
                            return true;
                        }
                        return false;
                    }

                    // Workspace switch: modifier + number (1~9)
                    let ws_mod_matches = match ws_mod.as_str() {
                        "ctrl" => ctrl && !shift && !alt,
                        _ => alt && !ctrl && !shift,  // default: alt
                    };
                    if ws_mod_matches {
                        if let Some(digit) = ch.to_digit(10) {
                            if digit >= 1 && digit <= 9 {
                                state.switch_workspace((digit - 1) as usize);
                                self.mark_dirty();
                                return true;
                            }
                        }
                    }
                }
            }
        }

        // Clipboard paste shortcuts
        if ctrl && shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "V" || c.as_str() == "v")
                    && state.engine.settings.clipboard.linux_style
                {
                    self.paste_to_terminal();
                    self.mark_dirty();
                    return true;
                }
            }
        }
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                if (c.as_str() == "v" || c.as_str() == "V" || c.as_str() == "\u{16}")
                    && state.engine.settings.clipboard.windows_style
                {
                    self.paste_to_terminal();
                    self.mark_dirty();
                    return true;
                }
            }
        }
        if alt && !ctrl && !shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "v" || c.as_str() == "V")
                    && state.engine.settings.clipboard.macos_style
                {
                    self.paste_to_terminal();
                    self.mark_dirty();
                    return true;
                }
            }
        }

        // Ctrl+= or Ctrl+Shift+=: increase font size
        if ctrl && !alt {
            if let Key::Character(c) = key {
                if c.as_str() == "=" || c.as_str() == "+" {
                    let current = state.engine.settings.appearance.font_size;
                    let new_size = (current + 1.0).min(72.0);
                    state.engine.settings.appearance.font_size = new_size;
                    self.mark_dirty();
                    return true;
                }
            }
        }

        // Ctrl+-: decrease font size
        if ctrl && !alt {
            if let Key::Character(c) = key {
                if c.as_str() == "-" {
                    let current = state.engine.settings.appearance.font_size;
                    let new_size = (current - 1.0).max(6.0);
                    state.engine.settings.appearance.font_size = new_size;
                    self.mark_dirty();
                    return true;
                }
            }
        }

        // Ctrl+0: reset font size to default (14)
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                if c.as_str() == "0" {
                    state.engine.settings.appearance.font_size = 14.0;
                    self.mark_dirty();
                    return true;
                }
            }
        }

        false
    }
}
