//! Modeless 에디터 윈도우 — Workspace/Tab/Pane preset 편집.
//!
//! `SettingsWindow` 구조를 따르되 modal 이 아닌 modeless 로 동작:
//! - 다른 윈도우 입력을 차단하지 않음
//! - Esc 로 닫히지 않음
//! - 엔진 전역 단일 인스턴스는 `App.preset_window_id` 가 관리
//! - 편집 즉시 store 가 디스크 동기화 (별도 save 버튼 없음)

use std::sync::Arc;

use winit::event::WindowEvent;

use tasty_presets::{PresetKind, PresetStore};

use crate::gpu::GpuState;
use crate::i18n::t;
use crate::ui::{LayoutContext, ToastManager, ToastScope};
use crate::window::{
    Modality, Window, WindowAction, WindowBase, WindowCtx,
    editor::{EDITOR_MODALITY, EditorWindow},
    sealed,
};

pub struct PresetWindow {
    pub base: WindowBase,
    store: PresetStore,
    active_kind: PresetKind,
    selected_workspace: Option<String>,
    selected_tab: Option<String>,
    selected_pane: Option<String>,
    toasts: ToastManager,
    shown: bool,
}

impl PresetWindow {
    pub fn new(gpu: GpuState, winit: Arc<winit::window::Window>, store: PresetStore) -> Self {
        Self {
            base: WindowBase::new(gpu, winit),
            store,
            active_kind: PresetKind::Workspace,
            selected_workspace: None,
            selected_tab: None,
            selected_pane: None,
            toasts: ToastManager::new(),
            shown: false,
        }
    }

    /// 우클릭/IPC 진입 시 특정 preset 선택 상태로 열기 위한 helper.
    pub fn select(&mut self, kind: PresetKind, name: String) {
        self.active_kind = kind;
        match kind {
            PresetKind::Workspace => self.selected_workspace = Some(name),
            PresetKind::Tab => self.selected_tab = Some(name),
            PresetKind::Pane => self.selected_pane = Some(name),
        }
        self.mark_dirty();
    }

    pub fn store(&self) -> &PresetStore {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut PresetStore {
        &mut self.store
    }

    /// 윈도우 close 시 store 회수 (변경된 캐시를 host 측으로 반영).
    pub fn into_store(self) -> PresetStore {
        self.store
    }
}

impl Window for PresetWindow {
    fn base(&self) -> &WindowBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut WindowBase {
        &mut self.base
    }
    fn modality(&self) -> Modality {
        EDITOR_MODALITY
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_event(&mut self, event: WindowEvent, _ctx: &mut WindowCtx<'_>) -> WindowAction {
        let (_, repaint) = self.base.gpu.handle_egui_event(&self.base.winit, &event);
        if repaint {
            self.mark_dirty();
        }

        match event {
            WindowEvent::CloseRequested => {
                return WindowAction::Close;
            }
            WindowEvent::Resized(size) => {
                self.base.gpu.resize(size);
                self.mark_dirty();
            }
            WindowEvent::RedrawRequested => {
                self.render();
            }
            WindowEvent::CursorMoved { .. } => {
                self.mark_dirty();
            }
            WindowEvent::ModifiersChanged(m) => {
                self.base.modifiers = m.state();
            }
            _ => {}
        }
        WindowAction::None
    }

    fn render(&mut self) {
        if !self.base.dirty {
            return;
        }
        self.base.dirty = false;

        let raw_input = self.base.gpu.take_egui_input(&self.base.winit);
        let store = &mut self.store;
        let active_kind = &mut self.active_kind;
        let sel_ws = &mut self.selected_workspace;
        let sel_tab = &mut self.selected_tab;
        let sel_pane = &mut self.selected_pane;
        let toasts = &mut self.toasts;

        let full_output = self.base.gpu.run_egui(raw_input, |ctx| {
            crate::preset_ui::draw_preset_panel(
                ctx, store, active_kind, sel_ws, sel_tab, sel_pane,
            );

            let empty_layout = LayoutContext {
                active_workspace: 0,
                pane_rects: Vec::new(),
                surface_rects: Vec::new(),
                active_tabs: Vec::new(),
            };
            toasts.draw(ctx, &empty_layout, false);
        });

        let has_copy = full_output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c, egui::OutputCommand::CopyText(_)));
        if has_copy {
            self.toasts.push_info(t("toast.copied"), ToastScope::Window);
            self.mark_dirty();
        }

        self.base
            .gpu
            .finish_egui_frame(&self.base.winit, full_output);

        if !self.shown {
            self.base.winit.set_visible(true);
            self.shown = true;
        }

        if self.base.dirty {
            self.base.winit.request_redraw();
        }
    }
}

impl EditorWindow for PresetWindow {}
impl sealed::Sealed for PresetWindow {}
