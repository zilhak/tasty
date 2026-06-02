mod clipboard;
mod divider_drag;
mod file_drop;
mod keyboard;
mod mouse;
mod preset_actions;
mod redraw;
mod selection;

pub(crate) mod ime;

pub(crate) use divider_drag::{DividerDrag, DividerDragKind};

use std::sync::Arc;

use winit::event::WindowEvent;
use winit::keyboard::ModifiersState;
use winit::window::CursorIcon;

use crate::gpu::{GpuState, ImePreeditState};
use crate::model::{PhysicalPx, PhysicalRect};
use crate::selection::TextSelection;
use crate::state::{AppState, FocusedSurfaceType};
use crate::view::ui::{View, sealed};
use crate::view::{
    Modality, TerminalHostView, ViewAction, ViewBase, ViewCtx, terminal_host::MODELESS_MODALITY,
};
use crate::{AppEvent, ClipboardContext};

/// 메인 터미널 윈도우. 워크스페이스/사이드바/탭을 갖고 터미널 계열 Surface를 호스팅한다.
/// `TerminalHostView` 계열의 대표 구현체.
pub struct MainWindow {
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
    pub(crate) left_mouse_down: bool,
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
    /// 현재 마우스 hover 중이고 수식키 조건을 만족한 링크. 렌더 및 클릭에 사용.
    pub(crate) hovered_link: Option<HoveredLink>,
    /// 가장 최근에 터미널에 paste한 시각. Ctrl+V 직후 사용자가 옆 키 Ctrl+C를 잘못 눌러
    /// 입력을 날려버리는 사고를 막기 위해 cooldown 구간 안의 Ctrl+C는 무시한다.
    pub(crate) last_terminal_paste_at: Option<std::time::Instant>,
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

impl MainWindow {
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
            left_mouse_down: false,
            last_click_time: None,
            last_click_pos: None,
            click_count: 0,
            ime_active: false,
            ime_cursor_advance: 0,
            ime_advance_base: (0, 0),
            double_tap: crate::double_tap::DoubleTapDetector::new(),
            webviews: std::collections::HashMap::new(),
            hovered_link: None,
            last_terminal_paste_at: None,
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

impl View for MainWindow {
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

        // Non-terminal surfaces (Explorer, Markdown) use egui widgets (TextEdit etc.)
        // that need direct keyboard events from egui's input system.
        let egui_surface = matches!(
            self.state.focused_surface_type(&self.core_state),
            FocusedSurfaceType::Kind(ref k) if k == "explorer" || k == "markdown"
        );

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
                }
                self.mark_dirty();
            }
            WindowEvent::Occluded(false) => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.base.modifiers = modifiers.state();
                if self.update_hovered_link() {
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
        // Window::render는 트레잇 디스패치 호환을 위해 존재하며 현재 Main
        // 창에서는 호출되지 않는다.
    }
}

impl TerminalHostView for MainWindow {}

impl sealed::Sealed for MainWindow {}
