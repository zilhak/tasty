mod attach_mesh_input;
pub(crate) mod clipboard;
#[cfg(debug_assertions)]
pub(crate) mod debug_input;
mod divider_drag;
mod egui_mesh;
mod file_drop;
mod keyboard;
mod mouse;
mod preset_actions;
mod redraw;
mod selection;
pub(crate) mod vi_copy;

pub(crate) mod ime;

pub(crate) use divider_drag::{DividerDrag, DividerDragKind};

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::keyboard::ModifiersState;
use winit::window::CursorIcon;

use crate::gpu::{GpuState, ImePreeditState};
use crate::model::{PhysicalPx, PhysicalRect};
use crate::selection::TextSelection;
use crate::state::AppState;
use crate::view::ui::{View, sealed};
use crate::view::{
    Modality, TerminalHostView, ViewAction, ViewBase, ViewCtx, terminal_host::MODELESS_MODALITY,
};
use crate::{AppEvent, ClipboardContext};

/// 메인 터미널 윈도우. 워크스페이스/사이드바/탭을 갖고 터미널 계열 Surface를 호스팅한다.
/// `TerminalHostView` 계열의 대표 구현체.
pub struct MainView {
    pub base: ViewBase,
    pub(crate) state: AppState,
    /// 본 윈도우 전용 CoreState. self.state 와 disjoint 한 field 로 두어
    /// `let engine = &mut self.core_state;` 식 접근을 가능하게 한다.
    pub(crate) core_state: crate::core::CoreState,
    pub(crate) cursor_position: Option<winit::dpi::PhysicalPosition<f64>>,
    pub(crate) dragging_divider: Option<DividerDrag>,
    pub(crate) clipboard: Option<ClipboardContext>,
    pub(crate) ime_preedit: Option<ImePreeditState>,
    pub(crate) proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    pub(crate) text_selection: Option<TextSelection>,
    /// vi-style 키보드 복사 모드. Some 일 때 키 입력이 PTY 로 전달되지 않고
    /// vi_copy::handle_vi_key 가 가로채 cursor/visual/yank 등을 처리.
    pub(crate) vi_copy: Option<vi_copy::ViCopyMode>,
    pub(crate) left_mouse_down: bool,
    /// 트래킹 ON 에서 Shift+좌클릭으로 시작한 "마우스 리포팅 우회 로컬 선택" 시퀀스인지.
    /// press 시점에 1회만 판정해 release 까지 유지한다 — motion/release 는 이 플래그로
    /// 라우팅하며, 드래그 도중 Shift 를 떼도(또는 멀티클릭으로 dragging=false 여도)
    /// 선택이 깨지지 않는다 (iTerm 동작). press 에서 set, release 에서 clear.
    pub(crate) left_select_bypass: bool,
    /// 마우스 리포팅(트래킹 앱)으로 마지막 보고한 셀 좌표. 드래그 motion 을 셀 단위로만
    /// 보고(중복 억제)하기 위해 사용. press/release/motion 보고 시 갱신.
    pub(crate) last_mouse_report_cell: Option<(usize, usize)>,
    pub(crate) last_click_time: Option<std::time::Instant>,
    pub(crate) last_click_pos: Option<(usize, usize)>,
    pub(crate) click_count: u8,
    /// Whether IME composition is active (set by Ime::Enabled/Disabled).
    /// When true, KeyboardInput text is ignored — only Ime::Commit sends text.
    pub(crate) ime_active: bool,
    /// Accumulated cursor advance from IME commits (in terminal columns).
    /// After Ime::Commit, the PTY echo hasn't been processed yet, so
    /// cursor_position() returns a stale value. This offset compensates
    /// so the next Preedit anchor appears after the committed text.
    pub(crate) ime_cursor_advance: usize,
    /// Raw cursor position when ime_cursor_advance was last updated.
    /// Used to reconcile: if the raw cursor moved past this point, PTY
    /// echo has caught up and advance should be reduced accordingly.
    pub(crate) ime_advance_base: (usize, usize),
    /// Detector for double-tap modifier shortcuts (e.g. Shift+Shift).
    pub(crate) double_tap: crate::double_tap::DoubleTapDetector,
    /// Native WebView instances keyed by surface ID.
    pub(crate) webviews: std::collections::HashMap<u32, crate::webview::PlatformWebView>,
    /// surface 별 마지막으로 webview 에 적용한 HTML 설정 — 변경 시에만 재적용(매 프레임 호출 회피).
    pub(crate) webview_applied_settings:
        std::collections::HashMap<u32, crate::webview::HtmlWebViewSettings>,
    /// 현재 마우스 hover 중이고 수식키 조건을 만족한 링크. 렌더 및 클릭에 사용.
    pub(crate) hovered_link: Option<HoveredLink>,
    /// 가장 최근에 터미널에 paste한 시각. Ctrl+V 직후 사용자가 옆 키 Ctrl+C를 잘못 눌러
    /// 입력을 날려버리는 사고를 막기 위해 cooldown 구간 안의 Ctrl+C는 무시한다.
    pub(crate) last_terminal_paste_at: Option<std::time::Instant>,
    /// egui-mesh surface 별 host→plugin set_context forward 추적 (A1-S7).
    pub(crate) egui_mesh: std::collections::HashMap<u32, egui_mesh::MeshForwardState>,
    /// attach mesh mirror surface(`AttachMeshSurface`) 별 client→server MeshContext/
    /// MeshInput forward 추적(TODO 20). `egui_mesh`의 attach 대응 —
    /// `attach_mesh_input.rs` 참고.
    pub(crate) attach_mesh_input:
        std::collections::HashMap<u32, attach_mesh_input::AttachMeshForwardState>,
    /// debug 마우스 주입이 세운 컨텍스트 메뉴를 포획해 둔 슬롯 (release 미노출).
    /// 실제 우클릭은 `process_pending_native_menu` 가 `TrackPopupMenu`(Windows) 등
    /// **블로킹 모달** 로 즉시 소비하므로, 헤드리스 주입 테스트에서 우클릭 라우팅을
    /// 관찰하려면 redraw 가 메뉴를 띄우기 전에 가로채야 한다. 주입 핸들러가 실행 직후
    /// live `pending_native_menu` 를 이 슬롯으로 옮겨 (a) redraw 블로킹을 막고
    /// (b) `debug.pending_menu` 가 결정적으로 읽게 한다. **주입 경로 전용** — 실제
    /// 사용자 우클릭은 이 경로를 타지 않아 메뉴가 정상 표시된다(원칙 1·3 격리).
    #[cfg(debug_assertions)]
    pub(crate) debug_captured_menu: Option<crate::state::PendingNativeMenu>,
}

/// Ctrl+V 직후 Ctrl+C를 SIGINT로 흘려보내지 않을 보호 시간.
pub(crate) const PASTE_CTRL_C_COOLDOWN: std::time::Duration = std::time::Duration::from_millis(500);

/// 마우스가 위에 있고 설정된 수식키 조건을 만족한 링크.
#[derive(Debug, Clone)]
pub(crate) struct HoveredLink {
    pub surface_id: u32,
    pub uri: String,
    pub highlight: crate::terminal_link::LinkHighlight,
}

impl MainView {
    pub(crate) fn new(
        gpu: GpuState,
        state: AppState,
        core_state: crate::core::CoreState,
        window: Arc<winit::window::Window>,
        proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    ) -> Self {
        Self {
            base: ViewBase::new(gpu, window),
            state,
            core_state,
            cursor_position: None,
            dragging_divider: None,
            clipboard: ClipboardContext::new(),
            ime_preedit: None,
            proxy,
            text_selection: None,
            vi_copy: None,
            left_mouse_down: false,
            left_select_bypass: false,
            last_mouse_report_cell: None,
            last_click_time: None,
            last_click_pos: None,
            click_count: 0,
            ime_active: false,
            ime_cursor_advance: 0,
            ime_advance_base: (0, 0),
            double_tap: crate::double_tap::DoubleTapDetector::new(),
            webviews: std::collections::HashMap::new(),
            webview_applied_settings: std::collections::HashMap::new(),
            hovered_link: None,
            last_terminal_paste_at: None,
            egui_mesh: std::collections::HashMap::new(),
            attach_mesh_input: std::collections::HashMap::new(),
            #[cfg(debug_assertions)]
            debug_captured_menu: None,
        }
    }

    /// Request this window to close (will be handled by the event loop).
    pub(crate) fn request_close(&mut self) {
        self.base.close_requested = true;
    }

    pub fn compute_terminal_rect(&self) -> PhysicalRect {
        let size = self.base.gpu.size();
        crate::model::compute_terminal_rect(
            PhysicalPx(size.width as f32),
            PhysicalPx(size.height as f32),
            self.state.sidebar_width,
            crate::adapters::ui::titlebar::top_inset(self.base.gpu.scale_factor()),
            crate::adapters::ui::status_bar_bottom_inset(self.base.gpu.scale_factor()),
            self.base.gpu.scale_factor(),
        )
    }

    /// 현재 preedit이 있으면 원래 surface에 확정 전송하고 IME 상태를 리셋한다.
    /// 단축키 소비/포커스 전환 직전에 호출.
    pub(crate) fn flush_ime_preedit(&mut self) {
        ime::flush_preedit(self);
    }

    /// 현재 preedit을 PTY로 보내지 않고 버린다.
    /// 팝업/오버레이가 열릴 때 사용.
    pub(crate) fn clear_ime_preedit(&mut self) {
        ime::clear_preedit(self);
    }

    /// PTY 출력 처리 후 cursor가 움직였을 수 있을 때 preedit anchor를 재계산한다.
    pub(crate) fn recalc_ime_preedit_anchor(&mut self) {
        ime::recalc_anchor(self);
    }

    pub(crate) fn update_ime_cursor_area(&self) {
        let Some(preedit) = &self.ime_preedit else {
            return;
        };
        let terminal_rect = self.compute_terminal_rect();
        let Some(cell_rect) = self.state.surface_cell_rect(
            &self.core_state,
            terminal_rect,
            preedit.surface_id,
            preedit.anchor_col,
            preedit.anchor_row,
            self.base.gpu.cell_width(),
            self.base.gpu.cell_height(),
        ) else {
            return;
        };

        use winit::dpi::{PhysicalPosition, PhysicalSize};
        self.base.winit.set_ime_cursor_area(
            PhysicalPosition::new(
                cell_rect.x.value().round() as i32,
                cell_rect.y.value().round() as i32,
            ),
            PhysicalSize::new(
                cell_rect.width.value().max(1.0).round() as u32,
                cell_rect.height.value().max(1.0).round() as u32,
            ),
        );
    }
}

impl View for MainView {
    fn base(&self) -> &ViewBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ViewBase {
        &mut self.base
    }
    fn modality(&self) -> Modality {
        MODELESS_MODALITY
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_event(&mut self, event: WindowEvent, ctx: &mut ViewCtx<'_>) -> ViewAction {
        // If a modal is active, block all input events before they reach egui.
        // Only allow non-input events (resize, redraw, scale factor, focus) through.
        if ctx.modal_active {
            match &event {
                WindowEvent::Resized(_)
                | WindowEvent::RedrawRequested
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::ModifiersChanged(_)
                | WindowEvent::Focused(_) => {}
                _ => return ViewAction::None,
            }
        }

        // ── Keyboard/IME routing ──
        // Keyboard and IME events are only forwarded to egui when an overlay
        // (settings, dialog, focused popup) is active. Otherwise the central
        // keyboard dispatcher in keyboard.rs handles routing to the correct
        // surface, and egui never sees the key event.
        let is_keyboard_event = matches!(
            &event,
            WindowEvent::KeyboardInput { .. } | WindowEvent::Ime(_)
        );
        let is_modifiers_event = matches!(&event, WindowEvent::ModifiersChanged(_));

        let overlay_open = self.state.settings_open
            || self.state.has_input_dialog_open()
            || self.state.popups.has_focused();

        // host-egui 위젯(TextEdit 등)으로 렌더되는 surface 만 winit 키/IME 를 egui 입력
        // 시스템으로 직접 넘긴다. markdown/image 는 plugin egui-mesh 로 렌더되므로 host
        // egui 에 대응 위젯이 없다 — 대신 중앙 키 디스패처(keyboard.rs)가 surface 로
        // Key/Text 를, ime.rs 가 IME 를 forward 한다. 여기서 빼야 host egui 가 그 키/IME 를
        // 삼켜 forward(특히 IME preedit)를 막지 않는다. 어느 kind 가 host egui 를 소비하는지는
        // registry 의 consumes_egui_input capability 로 판정(kind 하드코딩 없음).
        let egui_surface = self
            .state
            .focused_surface_type(&self.core_state)
            .kind_capability(&self.core_state, |d| d.consumes_egui_input);

        let is_redraw_event = matches!(&event, WindowEvent::RedrawRequested);

        let (egui_consumed, egui_repaint) = if is_redraw_event {
            // RedrawRequested를 egui에 전달하면 항상 repaint=true를 반환하여
            // dirty → request_redraw → RedrawRequested 무한 루프가 발생한다.
            // egui 렌더링은 handle_redraw의 run_egui_frame에서 별도로 수행하므로
            // 이 이벤트를 egui에 전달할 필요가 없다.
            (false, false)
        } else if is_keyboard_event {
            if overlay_open || egui_surface {
                self.base.gpu.handle_egui_event(&self.base.winit, &event)
            } else {
                (false, false)
            }
        } else if is_modifiers_event {
            let (_, repaint) = self.base.gpu.handle_egui_event(&self.base.winit, &event);
            (false, repaint)
        } else {
            self.base.gpu.handle_egui_event(&self.base.winit, &event)
        };

        if egui_repaint {
            self.mark_dirty();
        }

        let was_dirty = self.base.dirty;

        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.base.gpu.sync_scale_factor(&self.base.winit);
                let new_size = self.base.winit.inner_size();
                self.base.gpu.resize(new_size);
                let terminal_rect = self.compute_terminal_rect();
                let (cols, rows) = self.base.gpu.grid_size_for_rect(&terminal_rect);
                self.core_state.update_grid_size(cols, rows);
                let cell_w = self.base.gpu.cell_width();
                let cell_h = self.base.gpu.cell_height();
                self.state
                    .resize_all(&mut self.core_state, terminal_rect, cell_w, cell_h);
                self.mark_dirty();
            }
            WindowEvent::Focused(focused) => {
                self.base.focused = focused;
                if !focused {
                    if self.ime_preedit.is_some() {
                        self.flush_ime_preedit();
                    }
                    self.base.modifiers = ModifiersState::empty();
                    // modifier 가 비워지므로 switch-number overlay 스냅샷도 함께 clear —
                    // 안 하면 창을 벗어난 동안에도 키캡 오버레이가 남는다.
                    self.state.clear_switch_overlay();
                    // modifier-hint 오버레이도 홀드 상태 clear(창 밖에서 modifier 를 떼도
                    // ModifiersChanged 가 안 오므로 명시적으로 비운다). switch-overlay 와 동반.
                    self.state.modifier_hint.clear();
                }
                self.mark_dirty();
            }
            WindowEvent::Occluded(false) => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.base.modifiers = modifiers.state();
                let mut dirty = self.update_hovered_link();
                // switch-number overlay 스냅샷 갱신. modifier press/release 마다 대상이
                // 바뀌면 명시적으로 redraw 해야 키캡이 즉시 뜨고/사라진다(hovered link
                // 변화와 독립). 플랫폼 정규화는 dispatch.rs 와 동일 규칙으로 맞춘다.
                let mods = self.base.modifiers;
                let ctrl = mods.control_key();
                let shift = mods.shift_key();
                // `alt` = "alt" 토큰(macOS super/그 외 alt), `option` = "option" 토큰
                // (macOS 물리 ⌥/그 외 항상 false). switch-overlay·modifier-hint 공통 축.
                #[cfg(target_os = "macos")]
                let (alt, option) = (mods.super_key(), mods.alt_key());
                #[cfg(not(target_os = "macos"))]
                let (alt, option) = (mods.alt_key(), false);
                let kb = &self.core_state.settings.keybindings;
                if self
                    .state
                    .update_switch_overlay(&self.core_state, kb, ctrl, shift, alt, option)
                {
                    dirty = true;
                }
                // modifier-hint 오버레이 홀드 갱신. anchor 가 바뀌면 dirty(콘텐츠 갱신).
                // 표시 게이트(500ms)·페이드 재그리기는 draw_modifier_hint 가 스스로 예약한다.
                if self
                    .state
                    .modifier_hint
                    .update_hold(ctrl, alt, option, shift)
                {
                    dirty = true;
                }
                if dirty {
                    self.mark_dirty();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.handle_keyboard_input(&event, egui_consumed);
            }
            WindowEvent::Ime(ime_event) => {
                self.handle_ime(ime_event, egui_consumed);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(position, egui_consumed);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
                self.base.winit.set_cursor(CursorIcon::Default);
            }
            WindowEvent::MouseInput {
                state: button_state,
                button,
                ..
            } => {
                self.handle_mouse_input(button_state, button, egui_consumed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(delta, egui_consumed);
            }
            WindowEvent::HoveredFile(path) => {
                self.handle_hovered_file(path);
            }
            WindowEvent::HoveredFileCancelled => {
                self.handle_hovered_file_cancelled();
            }
            WindowEvent::DroppedFile(path) => {
                self.handle_dropped_file(path);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw(ctx.event_loop, ctx.plugin_manager);
            }
            _ => {}
        }

        if self.base.dirty && !was_dirty {
            self.base.winit.request_redraw();
        }

        ViewAction::None
    }

    fn render(&mut self) {
        // 메인 윈도우는 별도 진입점인 handle_redraw 경로로 렌더한다.
        // View::render는 트레잇 디스패치 호환을 위해 존재하며 현재 Main
        // 창에서는 호출되지 않는다.
    }
}

impl TerminalHostView for MainView {}

impl sealed::Sealed for MainView {}
