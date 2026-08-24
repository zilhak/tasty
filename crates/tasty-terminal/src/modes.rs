use termwiz::cell::CellAttributes;
use termwiz::color::ColorAttribute;
use termwiz::escape::csi::{
    DecPrivateMode, DecPrivateModeCode, Mode as CsiMode, TerminalMode, TerminalModeCode,
};
use termwiz::surface::{Change, CursorVisibility, Position, Surface};

use super::{CursorShape, MouseTrackingMode, TerminalState};

/// 1000 / 1002 / 1003 의 **독립 모드 레지스터**.
///
/// xterm 에서 셋은 서로 별개의 on/off 비트고, 실효 동작은 켜진 것 중 **가장 넓은 것**
/// 으로 정해진다(1003 ⊃ 1002 ⊃ 1000). 하나를 끄는 것이 다른 것을 끄지 않는다.
/// 실효 레벨([`MouseTrackingMode`]) 하나만 저장하면 이 정보가 손실돼서, 앱이
/// `1003h` 뒤에 `1002l` 을 보내는 것만으로 트래킹이 통째로 꺼진다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MouseTrackingRegisters {
    /// 1000 — 버튼 press/release 만.
    pub(crate) click: bool,
    /// 1002 — 버튼이 눌린 동안의 셀 이동까지.
    pub(crate) cell_motion: bool,
    /// 1003 — 버튼과 무관한 모든 이동까지.
    pub(crate) any_event: bool,
}

impl MouseTrackingRegisters {
    /// 켜진 비트 중 가장 넓은 것 = 실효 트래킹 레벨.
    pub(crate) fn effective(self) -> MouseTrackingMode {
        if self.any_event {
            MouseTrackingMode::AllMotion
        } else if self.cell_motion {
            MouseTrackingMode::CellMotion
        } else if self.click {
            MouseTrackingMode::Click
        } else {
            MouseTrackingMode::None
        }
    }
}

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
            CsiMode::SetMode(TerminalMode::Code(code)) => {
                self.set_standard_mode(code, true);
            }
            CsiMode::ResetMode(TerminalMode::Code(code)) => {
                self.set_standard_mode(code, false);
            }
            CsiMode::SetMode(TerminalMode::Unspecified(_))
            | CsiMode::ResetMode(TerminalMode::Unspecified(_)) => {
                // Unknown standard mode, ignore
            }
            _ => {}
        }
    }

    /// Handle standard ANSI mode (SM/RM, no `?` prefix) changes.
    pub(crate) fn set_standard_mode(&mut self, code: &TerminalModeCode, enable: bool) {
        match *code {
            TerminalModeCode::Insert => {
                // IRM: subsequent prints shift existing cells right (see action_to_changes).
                self.insert_mode = enable;
            }
            TerminalModeCode::ShowCursor => {
                // Standard mode 25 mirrors DEC private 25 (DECTCEM).
                self.set_dec_mode(&DecPrivateModeCode::ShowCursor, enable);
            }
            _ => {
                // KAM/SRM/LNM/BiDi: not supported, ignore.
            }
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
                    // Stash the primary pen and adopt the alt surface's pen mirror
                    // (only on a real primary→alt transition).
                    if !self.use_alternate {
                        self.swap_pen_for_surface_switch();
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
                    // The clear above resets the alt surface pen to default
                    // (ClearScreen bypasses `apply_change`/`mirror_pen`).
                    self.current_pen = CellAttributes::default();
                } else {
                    // Leave alternate screen — restore the primary pen mirror.
                    if self.use_alternate {
                        self.swap_pen_for_surface_switch();
                    }
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
                    // No clear here: adopt the alt surface's retained pen mirror.
                    if !self.use_alternate {
                        self.swap_pen_for_surface_switch();
                    }
                    self.use_alternate = true;
                } else {
                    if self.use_alternate {
                        self.swap_pen_for_surface_switch();
                    }
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
                // Mode 1000 — 자기 비트만 건드린다. 1002/1003 이 켜져 있으면 그대로
                // 유지되고 실효 레벨도 그쪽이 이긴다.
                self.update_mouse_tracking(|r| r.click = enable);
            }
            DecPrivateModeCode::ButtonEventMouse => {
                // Mode 1002
                self.update_mouse_tracking(|r| r.cell_motion = enable);
            }
            DecPrivateModeCode::AnyEventMouse => {
                // Mode 1003
                self.update_mouse_tracking(|r| r.any_event = enable);
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
            DecPrivateModeCode::ReverseVideo => {
                // DECSCNM (mode 5): reverse the whole screen. Stored as a flag the
                // renderer consumes by swapping the default fg/bg; the grid content
                // is untouched.
                self.reverse_screen = enable;
            }
            DecPrivateModeCode::OriginMode => {
                // DECOM (mode 6): origin mode. Setting or resetting it always homes
                // the cursor — to the region top-left in origin mode, else to the
                // screen top-left. Absolute positioning honors this via
                // resolve_origin_row(); relative-move confinement to the region is
                // not modeled (rarely relied upon).
                self.origin_mode = enable;
                let home_row = self.origin_home_row();
                self.apply_or_stage_change(Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(home_row),
                });
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

    /// Current cursor shape (DECSCUSR).
    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    /// Whether bracketed paste mode is active.
    pub fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    /// Current mouse tracking mode — 켜진 세 레지스터에서 계산한 **실효 레벨**.
    ///
    /// 소비 측은 이 레벨만 보면 되므로 저장 형식이 3 비트로 바뀌어도 시그니처는 그대로다.
    pub fn mouse_tracking(&self) -> MouseTrackingMode {
        self.mouse_tracking.effective()
    }

    /// 모드 레지스터 하나를 갱신하며 "첫 마우스 캡처 안내" 플래그를 관리한다.
    ///
    /// 무장/해제는 **실효 레벨의 엣지**로 판정한다 — `None → ON` 에서만 무장하고
    /// (ON→ON 전환, 예 1000→1002 는 재무장 안 함), `ON → None` 에서 disarm 한다
    /// (세션당 1회, ADR-0022 ②). 실효 레벨이 안 바뀌는 갱신(예 1003 이 켜진 채
    /// `1002l`)은 무장 상태도 건드리지 않는다 — 예전엔 이런 갱신이 트래킹과 함께
    /// 무장까지 날려서, 트래킹이 살아 있는데 안내가 다시 안 뜨는 상태가 됐다.
    fn update_mouse_tracking(&mut self, f: impl FnOnce(&mut MouseTrackingRegisters)) {
        let before = self.mouse_tracking.effective();
        f(&mut self.mouse_tracking);
        let after = self.mouse_tracking.effective();
        if before == MouseTrackingMode::None && after != MouseTrackingMode::None {
            self.mouse_capture_hint_armed = true;
        } else if before != MouseTrackingMode::None && after == MouseTrackingMode::None {
            self.mouse_capture_hint_armed = false;
        }
    }

    /// 무장된 "첫 마우스 캡처 안내" 플래그를 소비한다 — 무장돼 있었으면 `true` 를 반환하고
    /// 즉시 disarm 해 같은 트래킹 세션에서 다시 뜨지 않게 한다. 좌·우 클릭 보고 경로가
    /// 같은 플래그를 공유하므로, 먼저 호출한 쪽만 true 를 받는다 (첫 상호작용 1회).
    pub fn take_mouse_capture_hint(&mut self) -> bool {
        let armed = self.mouse_capture_hint_armed;
        self.mouse_capture_hint_armed = false;
        armed
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

    /// Whether synchronized output mode (DEC 2026) is active.
    pub fn synchronized_output(&self) -> bool {
        self.synchronized_output
    }

    /// Whether reverse-screen mode (DECSCNM, DEC private mode 5) is active. The
    /// renderer swaps the default foreground/background when this is set.
    pub fn screen_reverse(&self) -> bool {
        self.reverse_screen
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
