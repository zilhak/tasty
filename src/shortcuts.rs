use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::App;
use crate::model::SplitDirection;

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

    // Check modifiers match exactly
    if mods.control_key() != expect_ctrl
        || mods.shift_key() != expect_shift
        || mods.alt_key() != expect_alt
    {
        return false;
    }

    // Match the key part
    let key_lower = key_part.to_lowercase();
    match key {
        Key::Character(c) => {
            let ch = c.to_lowercase();
            ch == key_lower
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

impl App {
    /// Handle keyboard shortcuts. Returns true if the event was consumed by a shortcut.
    pub(crate) fn handle_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        let alt = mods.alt_key();

        let state = match &mut self.state {
            Some(s) => s,
            None => return false,
        };

        // --- Configurable keybindings (from settings) ---
        let kb = state.settings.keybindings.clone();

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
            if let Some(gpu) = &self.gpu {
                let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
            }
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.split_pane_horizontal, key, mods) {
            let _ = state.split_pane(SplitDirection::Horizontal);
            if let Some(gpu) = &self.gpu {
                let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
            }
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.split_surface_vertical, key, mods) {
            let _ = state.split_surface(SplitDirection::Vertical);
            if let Some(gpu) = &self.gpu {
                let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
            }
            self.mark_dirty();
            return true;
        }
        if matches_binding(&kb.split_surface_horizontal, key, mods) {
            let _ = state.split_surface(SplitDirection::Horizontal);
            if let Some(gpu) = &self.gpu {
                let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
            }
            self.mark_dirty();
            return true;
        }

        // --- Configurable shortcuts (previously hardcoded) ---

        // Toggle settings window
        if matches_binding(&kb.toggle_settings, key, mods) {
            state.settings_open = !state.settings_open;
            self.mark_dirty();
            return true;
        }

        // Toggle notification panel
        if matches_binding(&kb.toggle_notifications, key, mods) {
            state.notification_panel_open = !state.notification_panel_open;
            if state.notification_panel_open {
                state.notifications.mark_all_read();
            }
            self.mark_dirty();
            return true;
        }

        // Close active pane
        if matches_binding(&kb.close_pane, key, mods) {
            if state.close_active_pane() {
                if let Some(gpu) = &self.gpu {
                    let rect =
                        Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                    state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                }
                self.mark_dirty();
                return true;
            }
            return false;
        }

        // Close active surface
        if matches_binding(&kb.close_surface, key, mods) {
            if state.close_active_surface() {
                if let Some(gpu) = &self.gpu {
                    let rect =
                        Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                    state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                }
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
                    && state.settings.clipboard.linux_style
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
                    && state.settings.clipboard.windows_style
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
                    && state.settings.clipboard.macos_style
                {
                    self.paste_to_terminal();
                    self.mark_dirty();
                    return true;
                }
            }
        }

        false
    }
}
