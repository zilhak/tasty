//! Plugins modal window. Mirrors `SettingsWindow` structure.
//!
//! `App`이 `PluginManager`를 소유하므로, 모달은 직접 manager를 들고 있지 않고
//! 읽기 전용 `PluginsSnapshot` + `pending_actions` 큐만 보유한다. 매 tick에서
//! `App::process_plugins_window_actions()`가 큐를 비우고 manager에 적용한 뒤,
//! 새 snapshot을 모달에 다시 주입한다.

pub mod ui;

use std::sync::Arc;

use winit::event::WindowEvent;

use crate::gpu::GpuState;
use crate::plugins_ui::{self, PluginsAction, PluginsSnapshot, PluginsUiState};
use crate::ui::{LayoutContext, ToastManager};
use crate::window::{
    ModalWindow, Modality, Window, WindowAction, WindowBase, WindowCtx, modal::MODAL_MODALITY,
    sealed,
};

pub struct PluginsWindow {
    pub base: WindowBase,
    pub snapshot: PluginsSnapshot,
    pub pending_actions: Vec<PluginsAction>,
    ui_state: PluginsUiState,
    shown: bool,
    should_close: bool,
    toasts: ToastManager,
}

impl PluginsWindow {
    pub fn new(
        gpu: GpuState,
        winit: Arc<winit::window::Window>,
        snapshot: PluginsSnapshot,
    ) -> Self {
        Self {
            base: WindowBase::new(gpu, winit),
            snapshot,
            pending_actions: Vec::new(),
            ui_state: PluginsUiState::default(),
            shown: false,
            should_close: false,
            toasts: ToastManager::new(),
        }
    }

    /// 메인 루프가 actions를 적용한 뒤 호출하여 화면을 새 데이터로 갱신.
    pub fn refresh_snapshot(&mut self, snapshot: PluginsSnapshot) {
        self.snapshot = snapshot;
        self.mark_dirty();
    }

    /// 모달 윈도우 영역에 toast를 띄운다. `Add` 탭에서 설치 성공/실패 알림 등에 사용.
    pub fn push_toast(&mut self, message: impl Into<String>, kind: crate::ui::ToastKind) {
        self.toasts
            .push(message, kind, crate::ui::ToastScope::Window);
        self.mark_dirty();
    }
}

impl Window for PluginsWindow {
    fn base(&self) -> &WindowBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut WindowBase {
        &mut self.base
    }
    fn modality(&self) -> Modality {
        MODAL_MODALITY
    }

    fn as_modal(&self) -> Option<&dyn ModalWindow> {
        Some(self)
    }
    fn as_modal_mut(&mut self) -> Option<&mut dyn ModalWindow> {
        Some(self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_event(&mut self, event: WindowEvent, _ctx: &mut WindowCtx<'_>) -> WindowAction {
        let (_, egui_repaint) = self.base.gpu.handle_egui_event(&self.base.winit, &event);
        if egui_repaint {
            self.mark_dirty();
        }

        match event {
            WindowEvent::CloseRequested => {
                self.should_close = true;
                return WindowAction::Close;
            }
            WindowEvent::Resized(new_size) => {
                self.base.gpu.resize(new_size);
                self.mark_dirty();
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            WindowEvent::CursorMoved { .. } => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.base.modifiers = modifiers.state();
            }
            _ => {}
        }

        if self.should_close {
            WindowAction::Close
        } else {
            WindowAction::None
        }
    }

    fn render(&mut self) {
        if !self.base.dirty {
            return;
        }
        self.base.dirty = false;

        let raw_input = self.base.gpu.take_egui_input(&self.base.winit);
        let snapshot = &self.snapshot;
        let ui_state = &mut self.ui_state;
        let actions = &mut self.pending_actions;
        let toasts = &mut self.toasts;

        let full_output = self.base.gpu.run_egui(raw_input, |ctx| {
            plugins_ui::draw_plugins_panel(ctx, snapshot, ui_state, actions);

            let empty_layout = LayoutContext {
                active_workspace: 0,
                pane_rects: Vec::new(),
                surface_rects: Vec::new(),
                active_tabs: Vec::new(),
            };
            toasts.draw(ctx, &empty_layout, false);
        });

        self.base
            .gpu
            .finish_egui_frame(&self.base.winit, full_output);

        self.reveal_after_first_render();

        if !self.pending_actions.is_empty() {
            // actions를 메인 루프에서 처리하도록 윈도우를 다시 깨움.
            self.base.winit.request_redraw();
        }

        if self.base.dirty {
            self.base.winit.request_redraw();
        }
    }
}

impl ModalWindow for PluginsWindow {
    fn shown(&self) -> bool {
        self.shown
    }
    fn set_shown(&mut self, v: bool) {
        self.shown = v;
    }
}

impl sealed::Sealed for PluginsWindow {}
