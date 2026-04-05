use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{DecPrivateMode, DecPrivateModeCode, Mode as CsiMode};
use termwiz::surface::{Change, CursorVisibility, Position, Surface};

use super::{MouseTrackingMode, Terminal};

impl Terminal {
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
                if !enable {
                    self.flush_pending_changes();
                }
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
