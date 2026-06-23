use winit::event::{ElementState, MouseButton, MouseScrollDelta};

use super::{DividerDrag, DividerDragKind, HoveredLink, MainView};
use crate::core::intent::{DomainIntent, SendPayload};
use crate::settings::LinkModifier;
use crate::terminal_link::{self, LinkHighlight};
use crate::theme;
use crate::view::ui::View;
use tasty_type_geometry::length::PhysicalPx;

impl MainView {
    /// 현재 마우스 좌표와 수식키 상태로 hovered_link를 갱신한다.
    /// 변경이 있으면 true를 반환 (렌더 dirty 플래그를 켜기 위함).
    pub(crate) fn update_hovered_link(&mut self) -> bool {
        let engine = &mut self.core_state;
        let prev = self.hovered_link.as_ref().map(|h| {
            (
                h.surface_id,
                h.highlight.start_col,
                h.highlight.end_col,
                h.highlight.absolute_row,
            )
        });

        let modifier = LinkModifier::parse(&engine.settings.general.link_click_modifier);
        let mods = &self.base.modifiers;
        let matches_mods = modifier.matches(mods.control_key(), mods.alt_key(), mods.super_key());

        let new_link = if !matches_mods || self.state.settings_open || self.state.popup_hovered {
            None
        } else {
            self.compute_hovered_link()
        };

        let changed = prev
            != new_link.as_ref().map(|h| {
                (
                    h.surface_id,
                    h.highlight.start_col,
                    h.highlight.end_col,
                    h.highlight.absolute_row,
                )
            });
        self.hovered_link = new_link;
        changed
    }

    fn compute_hovered_link(&self) -> Option<HoveredLink> {
        let engine = &self.core_state;
        let pos = self.cursor_position?;
        let terminal_rect = self.compute_terminal_rect();
        let x = pos.x as f32;
        let y = pos.y as f32;
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            return None;
        }
        // 마우스 아래 surface id를 구하고 그 surface의 terminal을 사용.
        // focused 기반이 아니라 실제 hover 위치의 surface로 판별해야 여러 pane 중
        // 어느 곳이든 동작한다.
        let surface_id = self
            .state
            .surface_id_at_position(engine, x, y, terminal_rect)?;
        let terminal = engine.find_terminal_by_id(surface_id)?;
        let surface_rect = self
            .state
            .surface_rect_by_id(engine, surface_id, terminal_rect)?;

        let (cols, rows) = terminal.dimensions();
        let point = crate::selection::pixel_to_grid(
            x,
            y,
            &surface_rect,
            self.base.gpu.cell_width(),
            self.base.gpu.cell_height(),
            cols,
            rows,
            terminal.scroll_offset(),
            terminal.scrollback_len(),
        );
        let span = terminal_link::link_at(terminal, point.col, point.absolute_row)?;
        let th = theme::theme();
        let highlight = LinkHighlight {
            start_col: span.start_col,
            end_col: span.end_col,
            absolute_row: span.absolute_row,
            fg: th.accent_primary().to_gpu_rgba(),
            bg: th.selection_bg.to_gpu_rgba(),
        };
        Some(HoveredLink {
            surface_id,
            uri: span.uri,
            highlight,
        })
    }

    pub(super) fn handle_cursor_moved(
        &mut self,
        position: winit::dpi::PhysicalPosition<f64>,
        egui_consumed: bool,
    ) {
        self.cursor_position = Some(position);
        let overlay_open = self.state.settings_open;
        if egui_consumed || overlay_open || self.state.popup_hovered {
            if self.hovered_link.take().is_some() {
                self.mark_dirty();
            }
            self.mark_dirty();
            return;
        }

        let terminal_rect = self.compute_terminal_rect();
        let x = position.x as f32;
        let y = position.y as f32;

        if self.update_hovered_link() {
            self.mark_dirty();
        }

        // Handle selection drag
        if self.left_mouse_down && self.dragging_divider.is_none() {
            let is_dragging = self.text_selection.as_ref().is_some_and(|s| s.dragging);
            if is_dragging && let Some((point, _)) = self.mouse_to_grid(x, y, &terminal_rect) {
                if let Some(sel) = &mut self.text_selection {
                    sel.cursor = point;
                }
                self.mark_dirty();
            }
        }

        if let Some(drag) = self.dragging_divider {
            let cell_w = self.base.gpu.cell_width();
            let cell_h = self.base.gpu.cell_height();
            let changed = {
                let engine = &mut self.core_state;
                let changed = match drag.kind {
                    DividerDragKind::Pane => {
                        self.state
                            .update_pane_divider(engine, &drag.info, x, y, terminal_rect)
                    }
                    DividerDragKind::Surface => {
                        self.state
                            .update_surface_divider(engine, &drag.info, x, y, terminal_rect)
                    }
                };
                if changed {
                    self.state.resize_all(engine, terminal_rect, cell_w, cell_h);
                }
                changed
            };
            if changed {
                self.mark_dirty();
            }
        }
        // Cursor icon is determined in the egui render cycle (gpu/mod.rs)
    }

    pub(super) fn handle_mouse_input(
        &mut self,
        button_state: ElementState,
        button: MouseButton,
        egui_consumed: bool,
    ) {
        let overlay_open = self.state.settings_open;
        if egui_consumed || overlay_open || self.state.popup_hovered {
            // Even when egui consumes the event (e.g. egui-rendered panels),
            // we still need to update pane focus on left-click within the terminal area.
            //
            // 단, 팝업 위(`popup_hovered`)일 때는 제외한다. 입력 레이어 계약(Layer 3 =
            // Popup, docs/architecture/input-layer.md)상 "팝업 위면 터미널 무시"이므로,
            // 터미널 영역 위에 떠 있는 팝업을 클릭해도 뒤 pane/surface 로 포커스가
            // 넘어가면 안 된다. 형제 핸들러(handle_cursor_moved/handle_mouse_wheel)는
            // 이미 popup_hovered 가드를 가진다 — 이 블록만 누락돼 있었다.
            if egui_consumed
                && !overlay_open
                && !self.state.popup_hovered
                && button == MouseButton::Left
                && button_state == ElementState::Pressed
            {
                let terminal_rect = self.compute_terminal_rect();
                if let Some(pos) = self.cursor_position {
                    let (x, y) = (pos.x as f32, pos.y as f32);
                    if terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                        let engine = &mut self.core_state;
                        let changed_pane =
                            self.state
                                .focus_pane_at_position(engine, x, y, terminal_rect);
                        let changed_surf =
                            self.state
                                .focus_surface_at_position(engine, x, y, terminal_rect);
                        if changed_pane || changed_surf {
                            self.base.dirty = true;
                        }
                    }
                }
            }
            if button_state == ElementState::Released {
                self.dragging_divider = None;
                self.left_mouse_down = false;
            }
            if egui_consumed {
                self.mark_dirty();
            }
            return;
        }
        if button == MouseButton::Right && button_state == ElementState::Pressed {
            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                    return;
                }
                let sf = self.base.gpu.scale_factor();
                let engine = &mut self.core_state;
                let Some(surface_id) =
                    self.state
                        .surface_id_at_position(engine, x, y, terminal_rect)
                else {
                    return;
                };
                if engine.find_terminal_by_id(surface_id).is_none() {
                    return;
                }
                self.state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::TerminalSurface {
                        surface_id,
                        x: x / sf,
                        y: y / sf,
                    });
                self.mark_dirty();
            }
            return;
        }
        if button == MouseButton::Left {
            if button_state == ElementState::Pressed {
                self.left_mouse_down = true;
                // mouse drag 시작은 vi copy mode 와 충돌 — 자동 종료. (R7)
                if self.vi_copy.is_some() {
                    self.vi_copy = None;
                    self.base.dirty = true;
                }
            } else {
                self.left_mouse_down = false;
            }

            let terminal_rect = self.compute_terminal_rect();
            if let Some(pos) = self.cursor_position {
                let (x, y) = (pos.x as f32, pos.y as f32);
                // 수식키+클릭은 무조건 링크 클릭 동작으로 라우팅.
                // 링크 위면 열고, 링크 위가 아니면 아무것도 안 함 (selection 시작 안 함).
                let modifier =
                    LinkModifier::parse(&self.core_state.settings.general.link_click_modifier);
                let mods = &self.base.modifiers;
                let link_mods_match = !matches!(modifier, LinkModifier::None)
                    && modifier.matches(mods.control_key(), mods.alt_key(), mods.super_key());
                if link_mods_match && button_state == ElementState::Pressed {
                    if terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                        let engine = &mut self.core_state;
                        let changed_pane =
                            self.state
                                .focus_pane_at_position(engine, x, y, terminal_rect);
                        let changed_surf =
                            self.state
                                .focus_surface_at_position(engine, x, y, terminal_rect);
                        if changed_pane || changed_surf {
                            self.base.dirty = true;
                        }
                    }
                    if let Some(hovered) = self.hovered_link.clone() {
                        // 원격(mirror) surface 판별: 클릭한 surface 의 terminal 이 detached
                        // mirror(자식 PTY 없음)면 화면 경로가 원격 호스트 경로라 로컬 핸들러로
                        // 열 수 없다. ID(hovered.surface_id) 로 직접 판별 — 포커스 독립.
                        let is_mirror = self
                            .core_state
                            .find_terminal_by_id(hovered.surface_id)
                            .map(|t| t.process_id().is_none())
                            .unwrap_or(false);
                        match crate::file_dispatch::parse_link(&hovered.uri) {
                            crate::file_dispatch::LinkKind::FileTarget(path) => {
                                if is_mirror {
                                    // 원격 경로: 로컬 핸들러 lookup/identify 를 타지 않고 빈
                                    // picker(placeholder)만 띄운다 — empty-state, 실제 동작 없음.
                                    crate::file::dispatch::open_picker(
                                        &mut self.state,
                                        &mut self.core_state,
                                        crate::file::format::FileTarget::new(path),
                                        None,
                                        Vec::new(),
                                    );
                                } else {
                                    self.state.dispatch_intent(
                                        crate::core::intent::DomainIntent::DispatchFile {
                                            target: crate::file::format::FileTarget::new(path),
                                            depth: crate::file::format::DetectDepth::Deep,
                                            origin_surface_id: None,
                                        }
                                        .from_user_menu("terminal_link_click"),
                                    );
                                }
                            }
                            crate::file_dispatch::LinkKind::External(uri) => {
                                // 외부 URL(http:// 등)은 mirror 여부와 무관하게 기존대로 처리.
                                terminal_link::open_uri(&uri);
                            }
                        }
                    }
                    self.mark_dirty();
                    return;
                }
                if button_state == ElementState::Pressed {
                    let threshold = 4.0;
                    let engine = &mut self.core_state;
                    let pane_div =
                        self.state
                            .find_pane_divider_at(engine, x, y, terminal_rect, threshold);
                    let surf_div =
                        self.state
                            .find_surface_divider_at(engine, x, y, terminal_rect, threshold);
                    if let Some(info) = pane_div {
                        self.dragging_divider = Some(DividerDrag {
                            info,
                            kind: DividerDragKind::Pane,
                        });
                    } else if let Some(info) = surf_div {
                        self.dragging_divider = Some(DividerDrag {
                            info,
                            kind: DividerDragKind::Surface,
                        });
                    } else {
                        let (need_flush, mouse_tracking) = {
                            let old_surface = self.state.focused_surface_id(engine);
                            let changed_pane =
                                self.state
                                    .focus_pane_at_position(engine, x, y, terminal_rect);
                            let changed_surf =
                                self.state
                                    .focus_surface_at_position(engine, x, y, terminal_rect);
                            if changed_pane || changed_surf {
                                self.base.dirty = true;
                            }
                            let ime_active = self.ime_preedit.is_some();
                            let need_flush =
                                ime_active && self.state.focused_surface_id(engine) != old_surface;
                            // Start text selection (only if not mouse-tracking or Shift held)
                            let mouse_tracking = self
                                .state
                                .focused_terminal(engine)
                                .map(|t| t.mouse_tracking())
                                .unwrap_or(tasty_terminal::MouseTrackingMode::None);
                            (need_flush, mouse_tracking)
                        };
                        if need_flush {
                            self.flush_ime_preedit();
                        }
                        let shift = self.base.modifiers.shift_key();
                        if mouse_tracking == tasty_terminal::MouseTrackingMode::None || shift {
                            if shift {
                                self.extend_selection(x, y, &terminal_rect);
                            } else {
                                self.start_selection(x, y, &terminal_rect);
                            }
                        }
                    }
                } else if button_state == ElementState::Released {
                    if self.dragging_divider.is_some() {
                        self.dragging_divider = None;
                        let cell_w = self.base.gpu.cell_width();
                        let cell_h = self.base.gpu.cell_height();
                        let engine = &mut self.core_state;
                        self.state.resize_all(engine, terminal_rect, cell_w, cell_h);
                        self.base.dirty = true;
                    }
                    // Finish selection drag
                    if let Some(sel) = &mut self.text_selection {
                        sel.dragging = false;
                        if sel.is_empty() {
                            // Single click (no drag) — move cursor to clicked position
                            self.move_cursor_to_click(x, y, &terminal_rect);
                            self.text_selection = None;
                        }
                    }
                    self.mark_dirty();
                }
            }
        }
    }

    pub(super) fn handle_mouse_wheel(&mut self, delta: MouseScrollDelta, egui_consumed: bool) {
        let overlay_open = self.state.settings_open;
        if egui_consumed {
            self.mark_dirty();
        }
        if !egui_consumed && !overlay_open && !self.state.popup_hovered {
            // Find the surface under the cursor, falling back to the focused surface
            let terminal_rect = self.compute_terminal_rect();
            let target_id = self
                .cursor_position
                .and_then(|pos| {
                    let (x, y) = (pos.x as f32, pos.y as f32);
                    self.state
                        .surface_id_at_position(&self.core_state, x, y, terminal_rect)
                })
                .or_else(|| self.state.focused_surface_id(&self.core_state));

            if let Some(surface_id) = target_id {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as i32,
                };
                if lines == 0 {
                    return;
                }
                let info = self.core_state.find_terminal_by_id(surface_id).map(|t| {
                    (
                        t.is_alternate_screen(),
                        t.mouse_tracking(),
                        t.sgr_mouse(),
                        t.scroll_offset(),
                        t.scrollback_len(),
                        t.dimensions(),
                    )
                });
                let Some((is_alt, tracking, sgr, scroll_offset, sb_len, (cols, rows))) = info
                else {
                    return;
                };

                if tracking != tasty_terminal::MouseTrackingMode::None {
                    // 마우스 추적이 켜져 있으면 휠을 마우스 이벤트로 전송한다 (표준
                    // 동작). alt screen 이라고 무조건 arrow 로 바꾸면, 앱(예: Claude
                    // Code)이 그 arrow 를 history 이동으로 해석해 스크롤이 깨진다.
                    let cell_w = self.base.gpu.cell_width();
                    let cell_h = self.base.gpu.cell_height();
                    let (col, row) = self
                        .cursor_position
                        .and_then(|pos| {
                            let (x, y) = (pos.x as f32, pos.y as f32);
                            let rect = self.state.surface_rect_by_id(
                                &self.core_state,
                                surface_id,
                                terminal_rect,
                            )?;
                            let point = crate::selection::pixel_to_grid(
                                x,
                                y,
                                &rect,
                                cell_w,
                                cell_h,
                                cols,
                                rows,
                                scroll_offset,
                                sb_len,
                            );
                            // viewport 기준 1-based (col, row). alt screen 은 scrollback
                            // 이 없어 absolute_row 가 곧 viewport row.
                            let viewport_top = sb_len.saturating_sub(scroll_offset);
                            let row = point
                                .absolute_row
                                .saturating_sub(viewport_top)
                                .min(rows.saturating_sub(1))
                                + 1;
                            let col = point.col.min(cols.saturating_sub(1)) + 1;
                            Some((col, row))
                        })
                        .unwrap_or((1, 1));
                    // xterm wheel button: 64 = up, 65 = down.
                    let btn = if lines > 0 { 64 } else { 65 };
                    let count = lines.unsigned_abs() as usize;
                    let bytes = encode_wheel_report(sgr, btn, col, row, count);
                    self.state.dispatch_intent(
                        DomainIntent::SendToSurface {
                            surface_id,
                            payload: SendPayload::Bytes(bytes),
                        }
                        .from_user_shortcut("mouse_wheel"),
                    );
                } else if is_alt {
                    // 마우스 추적 OFF + alt screen — alternate scroll mode: 휠을 arrow
                    // 키로 변환 (vim/less 등에서 휠 스크롤). lines 만큼 한 Vec 에 concat
                    // 후 1 Intent (큐 폭증 회피).
                    let seq: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    let count = lines.unsigned_abs() as usize;
                    let mut bytes = Vec::with_capacity(seq.len() * count);
                    for _ in 0..count {
                        bytes.extend_from_slice(seq);
                    }
                    self.state.dispatch_intent(
                        DomainIntent::SendToSurface {
                            surface_id,
                            payload: SendPayload::Bytes(bytes),
                        }
                        .from_user_shortcut("mouse_wheel"),
                    );
                } else {
                    // 일반 화면 — scrollback (UI 자체 mutate, PTY 와 무관).
                    if let Some(terminal) = self.core_state.find_terminal_by_id_mut(surface_id) {
                        if lines > 0 {
                            terminal.scroll_up(lines as usize);
                        } else if lines < 0 {
                            terminal.scroll_down((-lines) as usize);
                        }
                    }
                    self.base.dirty = true;
                }
            }
        }
    }
}

/// 마우스 휠 이벤트를 마우스 리포팅 시퀀스로 인코딩한다. `sgr` 가 true 면 SGR
/// (`ESC [ < btn ; col ; row M`), 아니면 legacy X10 (`ESC [ M` + 32-offset 3 bytes).
/// `count` 만큼 반복 발행. `btn` 은 64(up)/65(down), `col`/`row` 는 1-based.
fn encode_wheel_report(sgr: bool, btn: u32, col: usize, row: usize, count: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    for _ in 0..count {
        if sgr {
            bytes.extend_from_slice(format!("\x1b[<{btn};{col};{row}M").as_bytes());
        } else {
            // X10: 각 값에 32 를 더하고 255 로 clamp (legacy 인코딩 한계).
            let cb = (32 + btn).min(255) as u8;
            let cx = (32 + col as u32).min(255) as u8;
            let cy = (32 + row as u32).min(255) as u8;
            bytes.extend_from_slice(&[0x1b, b'[', b'M', cb, cx, cy]);
        }
    }
    bytes
}

#[cfg(test)]
mod wheel_tests {
    use super::encode_wheel_report;

    #[test]
    fn sgr_encodes_button_col_row() {
        assert_eq!(encode_wheel_report(true, 64, 3, 5, 1), b"\x1b[<64;3;5M");
        assert_eq!(encode_wheel_report(true, 65, 10, 20, 1), b"\x1b[<65;10;20M");
    }

    #[test]
    fn x10_encodes_with_32_offset() {
        // btn=64 → 96, col=1 → 33, row=1 → 33.
        assert_eq!(
            encode_wheel_report(false, 64, 1, 1, 1),
            vec![0x1b, b'[', b'M', 96, 33, 33]
        );
    }

    #[test]
    fn count_repeats_sequence() {
        assert_eq!(
            encode_wheel_report(true, 64, 1, 1, 3),
            b"\x1b[<64;1;1M\x1b[<64;1;1M\x1b[<64;1;1M"
        );
    }

    #[test]
    fn x10_clamps_large_coords() {
        // 32 + 300 = 332 → clamp 255.
        let out = encode_wheel_report(false, 64, 300, 1, 1);
        assert_eq!(out[4], 255);
    }
}
