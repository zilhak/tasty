pub mod ui;

use std::sync::Arc;

use winit::event::WindowEvent;

use crate::adapters::ui::window::{
    ModalWindow, Modality, Window, WindowAction, WindowBase, WindowCtx, modal::MODAL_MODALITY,
    sealed,
};
use crate::adapters::ui::{LayoutContext, ToastManager, ToastScope};
use crate::file::format::FileFormatRegistry;
use crate::file::handler::FileHandlerRegistry;
use crate::gpu::GpuState;
use crate::i18n::t;
use crate::settings::Settings;
use crate::settings_ui::{self, PluginShortcutSnapshot, SettingsUiState};

/// 설정 모달 윈도우. egui 기반 설정 UI를 렌더한다.
pub struct SettingsWindow {
    pub base: WindowBase,
    pub settings: Settings,
    settings_ui_state: SettingsUiState,
    /// `FileHandler` 탭의 Extension Mapping sub-tab 에서 사용. Save 시 user TOML 에 직접 저장.
    file_format: Arc<FileFormatRegistry>,
    /// 동일 user TOML 파일 (`~/.tasty/file-handlers.toml`) 의 `[[handler]]` 섹션을 보존하기
    /// 위해 combined save 시 함께 export.
    file_handler: Arc<FileHandlerRegistry>,
    /// user TOML 저장 경로. CI/CD 등 홈 디렉토리가 없으면 `None` 으로 들어와 저장 skip.
    user_config_path: Option<std::path::PathBuf>,
    shown: bool,
    double_tap: crate::double_tap::DoubleTapDetector,
    captured_double_tap: Option<String>,
    should_close: bool,
    toasts: ToastManager,
}

impl SettingsWindow {
    pub fn new(
        gpu: GpuState,
        winit: Arc<winit::window::Window>,
        settings: Settings,
        file_format: Arc<FileFormatRegistry>,
        file_handler: Arc<FileHandlerRegistry>,
        user_config_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            base: WindowBase::new(gpu, winit),
            settings,
            settings_ui_state: SettingsUiState::new(),
            file_format,
            file_handler,
            user_config_path,
            shown: false,
            double_tap: crate::double_tap::DoubleTapDetector::new(),
            captured_double_tap: None,
            should_close: false,
            toasts: ToastManager::new(),
        }
    }

    pub fn render_settings(&mut self) {
        self.render();
    }

    /// Plugins 서브탭에서 표시할 plugin command snapshot을 주입한다.
    pub fn set_plugin_shortcuts(&mut self, snapshot: PluginShortcutSnapshot) {
        self.settings_ui_state.plugin_shortcuts = snapshot;
    }

    /// 사용자가 Plugins 서브탭에서 변경한 override draft를 가져간다.
    /// 호출 후에는 빈 draft가 남는다. 모달 close 시 main App이 회수.
    pub fn take_plugin_shortcut_draft(
        &mut self,
    ) -> std::collections::BTreeMap<
        (String, String),
        Option<crate::plugin::registry_state::ShortcutOverride>,
    > {
        std::mem::take(&mut self.settings_ui_state.plugin_shortcuts_draft)
    }
}

impl Window for SettingsWindow {
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
        // 녹화 중이면 키보드 이벤트를 egui에 전달하지 않는다.
        // egui가 Cmd+C 등을 시맨틱 커맨드(Copy)로 소비하면 캡처가 안 되기 때문.
        let is_recording = self.settings_ui_state.is_recording();
        let skip_egui = is_recording && matches!(&event, WindowEvent::KeyboardInput { .. });

        if !skip_egui {
            let (_, egui_repaint) = self.base.gpu.handle_egui_event(&self.base.winit, &event);
            if egui_repaint {
                self.mark_dirty();
            }
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
            WindowEvent::KeyboardInput { ref event, .. } => {
                use winit::event::ElementState;

                self.double_tap
                    .on_key_event(&event.logical_key, event.state == ElementState::Pressed);
                if event.state == ElementState::Pressed {
                    if let Some(dt) = self.double_tap.take() {
                        self.captured_double_tap = Some(dt.binding_str().to_string());
                        self.mark_dirty();
                    }
                }

                // 녹화 중이면 winit에서 직접 키 조합 캡처
                if is_recording {
                    let combo =
                        crate::settings_ui::capture_winit_key_combo(event, self.base.modifiers);
                    if !matches!(combo, crate::settings_ui::KeyCapture::None) {
                        self.settings_ui_state.captured_winit_combo = Some(combo);
                        self.mark_dirty();
                    }
                }
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
        let mut settings = self.settings.clone();
        let reduced_motion = settings.accessibility.reduced_motion;
        let ui_state = &mut self.settings_ui_state;
        let captured_dt = &mut self.captured_double_tap;
        let toasts = &mut self.toasts;
        let mut action: Option<bool> = None;

        let file_format = self.file_format.clone();
        let file_handler = self.file_handler.clone();
        let user_config_path = self.user_config_path.clone();
        let full_output = self.base.gpu.run_egui(raw_input, |ctx| {
            action = settings_ui::draw_settings_panel(
                ctx,
                &mut settings,
                ui_state,
                captured_dt,
                file_format.as_ref(),
                file_handler.as_ref(),
                user_config_path.as_deref(),
            );

            let empty_layout = LayoutContext {
                active_workspace: 0,
                pane_rects: Vec::new(),
                surface_rects: Vec::new(),
                active_tabs: Vec::new(),
            };
            toasts.draw(ctx, &empty_layout, reduced_motion);
        });

        self.settings = settings;
        if action.is_some() {
            self.should_close = true;
        }

        let has_copy = full_output
            .platform_output
            .commands
            .iter()
            .any(|cmd| matches!(cmd, egui::OutputCommand::CopyText(_)));
        if has_copy {
            self.toasts.push_info(t("toast.copied"), ToastScope::Window);
            self.mark_dirty();
        }

        self.base
            .gpu
            .finish_egui_frame(&self.base.winit, full_output);

        self.reveal_after_first_render();

        if self.base.dirty {
            self.base.winit.request_redraw();
        }
    }
}

impl ModalWindow for SettingsWindow {
    fn shown(&self) -> bool {
        self.shown
    }
    fn set_shown(&mut self, v: bool) {
        self.shown = v;
    }
}

impl sealed::Sealed for SettingsWindow {}
