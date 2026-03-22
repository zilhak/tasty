use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::App;
use crate::model::SplitDirection;

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

        // Ctrl+Shift combinations
        if ctrl && shift {
            if let Key::Character(c) = key {
                match c.as_str() {
                    // Ctrl+Shift+W: Close active pane (unsplit)
                    "W" | "w" => {
                        if state.close_active_pane() {
                            if let Some(gpu) = &self.gpu {
                                let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                                state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                            }
                            self.dirty = true;
                            return true;
                        }
                        return false;
                    }
                    // Ctrl+Shift+N: New workspace
                    "N" | "n" => {
                        let _ = state.add_workspace();
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+T: New tab in focused pane
                    "T" | "t" => {
                        let _ = state.add_tab();
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+E: Pane split vertical (new independent tab bar)
                    "E" | "e" => {
                        let _ = state.split_pane(SplitDirection::Vertical);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+O: Pane split horizontal (new independent tab bar)
                    "O" | "o" => {
                        let _ = state.split_pane(SplitDirection::Horizontal);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+D: SurfaceGroup split vertical (within current tab)
                    "D" | "d" => {
                        let _ = state.split_surface(SplitDirection::Vertical);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+J: SurfaceGroup split horizontal (within current tab)
                    "J" | "j" => {
                        let _ = state.split_surface(SplitDirection::Horizontal);
                        if let Some(gpu) = &self.gpu {
                            let rect = Self::compute_terminal_rect_with_sidebar(gpu, state.sidebar_width);
                            state.resize_all(rect, gpu.cell_width(), gpu.cell_height());
                        }
                        self.dirty = true;
                        return true;
                    }
                    // Ctrl+Shift+I: Toggle notification panel
                    "I" | "i" => {
                        state.notification_panel_open = !state.notification_panel_open;
                        // Mark all as read when opening
                        if state.notification_panel_open {
                            state.notifications.mark_all_read();
                        }
                        self.dirty = true;
                        return true;
                    }
                    _ => {}
                }
            }

            // Ctrl+Shift+Tab: previous tab in focused pane
            if let Key::Named(NamedKey::Tab) = key {
                state.prev_tab_in_pane();
                self.dirty = true;
                return true;
            }
        }

        // Ctrl+W: Close active tab (if >1 tabs)
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                let s = c.as_str();
                if s == "w" || s == "W" || s == "\u{17}" {
                    if state.close_active_tab() {
                        self.dirty = true;
                        return true;
                    }
                    return false;
                }
            }
        }

        // Ctrl+,: Toggle settings window
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                if c.as_str() == "," {
                    state.settings_open = !state.settings_open;
                    self.dirty = true;
                    return true;
                }
            }
        }

        // Ctrl+Tab: next tab in focused pane
        if ctrl && !shift && !alt {
            if let Key::Named(NamedKey::Tab) = key {
                state.next_tab_in_pane();
                self.dirty = true;
                return true;
            }
        }

        // Clipboard paste shortcuts
        // Ctrl+Shift+V (Linux style)
        if ctrl && shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "V" || c.as_str() == "v")
                    && state.settings.clipboard.linux_style
                {
                    self.paste_to_terminal();
                    self.dirty = true;
                    return true;
                }
            }
        }
        // Ctrl+V (Windows style) -- only when no text selection exists
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                if (c.as_str() == "v" || c.as_str() == "V" || c.as_str() == "\u{16}")
                    && state.settings.clipboard.windows_style
                {
                    self.paste_to_terminal();
                    self.dirty = true;
                    return true;
                }
            }
        }
        // Alt+V (macOS style)
        if alt && !ctrl && !shift {
            if let Key::Character(c) = key {
                if (c.as_str() == "v" || c.as_str() == "V")
                    && state.settings.clipboard.macos_style
                {
                    self.paste_to_terminal();
                    self.dirty = true;
                    return true;
                }
            }
        }

        // Alt+1~9: switch workspace
        if alt && !ctrl && !shift {
            if let Key::Character(c) = key {
                if let Some(digit) = c.chars().next().and_then(|ch| ch.to_digit(10)) {
                    if digit >= 1 && digit <= 9 {
                        state.switch_workspace((digit - 1) as usize);
                        self.dirty = true;
                        return true;
                    }
                }
            }
        }

        // Alt+Arrow: move focus between panes
        if alt && !ctrl && !shift {
            match key {
                Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::ArrowDown) => {
                    state.move_focus_forward();
                    self.dirty = true;
                    return true;
                }
                Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowUp) => {
                    state.move_focus_backward();
                    self.dirty = true;
                    return true;
                }
                _ => {}
            }
        }

        false
    }
}
