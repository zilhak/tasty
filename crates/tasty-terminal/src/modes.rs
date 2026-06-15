use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{DecPrivateMode, DecPrivateModeCode, Mode as CsiMode};
use termwiz::surface::{Change, CursorVisibility, Position, Surface};

use super::{MouseTrackingMode, TerminalState};

impl TerminalState {
    /// Handle DECSET/DECRST mode changes.
    pub(crate) fn handle_mode(&mut self, mode: &CsiMode) {
        match mode {
            CsiMode::SetDecPrivateMode(DecPrivateMode::Code(code)) => {
                self.set_dec_mode(code, true);
            }
            CsiMode::ResetDecPrivateMode(DecPrivateMode::Code(code)) => {
                self.set_dec_mode(code, false);
            }
            CsiMode::SetDecPrivateMode(DecPrivateMode::Unspecified(_))
            | CsiMode::ResetDecPrivateMode(DecPrivateMode::Unspecified(_)) => {
                // Unknown mode, ignore
            }
            _ => {}
        }
    }

    pub(crate) fn set_dec_mode(&mut self, code: &DecPrivateModeCode, enable: bool) {
        match *code {
            DecPrivateModeCode::ApplicationCursorKeys => {
                self.application_cursor_keys = enable;
            }
            DecPrivateModeCode::StartBlinkingCursor => {
                // Cursor blink -- no-op for now (rendering doesn't support blink)
            }
            DecPrivateModeCode::ShowCursor => {
                self.cursor_visible = enable;
                let vis = if enable {
                    CursorVisibility::Visible
                } else {
                    CursorVisibility::Hidden
                };
                self.apply_or_stage_change(Change::CursorVisibility(vis));
            }
            DecPrivateModeCode::ClearAndEnableAlternateScreen => {
                // Mode 1049: save cursor, switch to alt screen, clear it
                if enable {
                    // Save cursor on primary
                    let pos = self.primary_surface.cursor_position();
                    self.alt_saved_cursor = Some((pos.0, pos.1));
                    // Create alternate surface if needed
                    if self.alternate_surface.is_none() {
                        self.alternate_surface = Some(Surface::new(self.cols, self.rows));
                    }
                    self.use_alternate = true;
                    // Clear alternate screen
                    if let Some(alt) = &mut self.alternate_surface {
                        alt.add_change(Change::ClearScreen(ColorAttribute::Default));
                        alt.add_change(Change::CursorPosition {
                            x: Position::Absolute(0),
                            y: Position::Absolute(0),
                        });
                    }
                } else {
                    // Leave alternate screen
                    self.use_alternate = false;
                    // Restore cursor on primary
                    if let Some((x, y)) = self.alt_saved_cursor.take() {
                        self.apply_or_stage_change(Change::CursorPosition {
                            x: Position::Absolute(x),
                            y: Position::Absolute(y),
                        });
                    }
                }
            }
            DecPrivateModeCode::EnableAlternateScreen
            | DecPrivateModeCode::OptEnableAlternateScreen => {
                // Mode 47 / 1047: switch without save/clear
                if enable {
                    if self.alternate_surface.is_none() {
                        self.alternate_surface = Some(Surface::new(self.cols, self.rows));
                    }
                    self.use_alternate = true;
                } else {
                    self.use_alternate = false;
                }
            }
            DecPrivateModeCode::SaveCursor => {
                // Mode 1048: save/restore cursor
                if enable {
                    let pos = self.surface().cursor_position();
                    self.saved_cursor = Some((pos.0, pos.1));
                } else if let Some((x, y)) = self.saved_cursor {
                    self.apply_or_stage_change(Change::CursorPosition {
                        x: Position::Absolute(x),
                        y: Position::Absolute(y),
                    });
                }
            }
            DecPrivateModeCode::BracketedPaste => {
                self.bracketed_paste = enable;
            }
            DecPrivateModeCode::MouseTracking => {
                self.mouse_tracking = if enable {
                    MouseTrackingMode::Click
                } else {
                    MouseTrackingMode::None
                };
            }
            DecPrivateModeCode::ButtonEventMouse => {
                // Mode 1002
                self.mouse_tracking = if enable {
                    MouseTrackingMode::CellMotion
                } else {
                    MouseTrackingMode::None
                };
            }
            DecPrivateModeCode::AnyEventMouse => {
                // Mode 1003
                self.mouse_tracking = if enable {
                    MouseTrackingMode::AllMotion
                } else {
                    MouseTrackingMode::None
                };
            }
            DecPrivateModeCode::SGRMouse => {
                self.sgr_mouse = enable;
            }
            DecPrivateModeCode::FocusTracking => {
                self.focus_tracking = enable;
            }
            DecPrivateModeCode::SynchronizedOutput => {
                self.synchronized_output = enable;
            }
            DecPrivateModeCode::AutoWrap => {
                // AutoWrap is handled by termwiz Surface internally, ignore for now
            }
            _ => {
                // Unknown/unsupported mode, ignore
            }
        }
    }
}

impl TerminalState {
    /// Whether application cursor keys mode is active (DECCKM).
    pub fn application_cursor_keys(&self) -> bool {
        self.application_cursor_keys
    }

    /// Whether the cursor is visible (DECTCEM).
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Whether bracketed paste mode is active.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    /// Current mouse tracking mode.
    pub fn mouse_tracking(&self) -> MouseTrackingMode {
        self.mouse_tracking
    }

    /// Whether SGR mouse encoding is active.
    pub fn sgr_mouse(&self) -> bool {
        self.sgr_mouse
    }

    /// Whether focus tracking is active.
    pub fn focus_tracking(&self) -> bool {
        self.focus_tracking
    }

    /// Whether the alternate screen is active.
    pub fn is_alternate_screen(&self) -> bool {
        self.use_alternate
    }

    /// Scan the active surface for an isolated reverse-video cell.
    ///
    /// Some TUIs (notably Ink-based ones like Claude Code) hide the real terminal
    /// cursor with `\e[?25l` and draw their own "fake cursor" by emitting a single
    /// cell with the reverse-video attribute (`\e[7m`). This scan detects that cell
    /// so we can use its position as the IME preedit anchor.
    ///
    /// Returns the cell position only when a **single** reverse-video cell exists.
    /// Multi-cell reverse regions (selection highlight, inverse-painted UI) are
    /// ambiguous and return None.
    pub fn find_fake_cursor_cell(&self) -> Option<(usize, usize)> {
        let surface = self.surface();
        let mut found: Option<(usize, usize)> = None;
        for (row_idx, line) in surface.screen_lines().iter().enumerate() {
            for cell_ref in line.visible_cells() {
                if cell_ref.attrs().reverse() {
                    if found.is_some() {
                        return None; // two or more — ambiguous
                    }
                    found = Some((cell_ref.cell_index(), row_idx));
                }
            }
        }
        found
    }

    // ---- Scrollback buffer methods (delegated to Scrollback) ----
}
