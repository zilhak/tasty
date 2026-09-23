pub mod ui;

use std::sync::Arc;

use winit::event::WindowEvent;

use crate::adapters::ui::{LayoutContext, ToastManager, ToastScope};
use crate::file::format::FileFormatRegistry;
use crate::file::handler::FileHandlerRegistry;
use crate::gpu::GpuState;
use crate::i18n::t;
use crate::settings::Settings;
use crate::settings_ui::{self, PluginShortcutSnapshot, SettingsUiState};
use crate::view::ui::{View, sealed};
use crate::view::{ModalView, ViewAction, ViewBase, ViewCtx};

/// 설정 모달 윈도우. egui 기반 설정 UI를 렌더한다.
pub struct SettingsView {
    pub base: ViewBase,
    pub settings: Settings,
    settings_ui_state: SettingsUiState,
    /// `FileHandler` 탭의 Extension Mapping sub-tab 에서 사용. Save 시 user TOML 에 직접 저장.
    file_format: Arc<FileFormatRegistry>,
    /// 같은 사용자 TOML의 handler 섹션을 함께 내보내 저장 시 보존한다.
    file_handler: Arc<FileHandlerRegistry>,
    /// user TOML 저장 경로. CI/CD 등 홈 디렉토리가 없으면 `None` 으로 들어와 저장 skip.
    user_config_path: Option<std::path::PathBuf>,
    shown: bool,
    double_tap: crate::double_tap::DoubleTapDetector,
    captured_double_tap: Option<String>,
    should_close: bool,
    /// footer Save 로 닫혔는가. plugin override draft 는 이것이 참일 때만 회수된다 —
    /// Cancel · 창 닫기 · 설정 토글 키로 닫으면 그 draft 는 버려진다.
    committed: bool,
    toasts: ToastManager,
}

impl SettingsView {
    pub fn new(
        gpu: GpuState,
        winit: Arc<winit::window::Window>,
        settings: Settings,
        file_format: Arc<FileFormatRegistry>,
        file_handler: Arc<FileHandlerRegistry>,
        user_config_path: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            base: ViewBase::new(gpu, winit),
            settings,
            settings_ui_state: SettingsUiState::new(),
            file_format,
            file_handler,
            user_config_path,
            shown: false,
            double_tap: crate::double_tap::DoubleTapDetector::new(),
            captured_double_tap: None,
            should_close: false,
            committed: false,
            toasts: ToastManager::new(),
        }
    }

    /// Plugins 서브탭에서 표시할 plugin command snapshot을 주입한다.
    pub fn set_plugin_shortcuts(&mut self, snapshot: PluginShortcutSnapshot) {
        self.settings_ui_state.plugin_shortcuts = snapshot;
    }

    /// 첫 진입 탭을 `Plugin` 으로 설정 (Plugins 모달의 `Configure` 진입점).
    pub fn focus_plugin_tab(&mut self) {
        self.settings_ui_state.select_plugin_tab();
    }

    /// 첫 진입 탭을 `FileHandler` 로 설정 (file handler picker popup 의
    /// "설정에서 핸들러 등록" 진입점).
    pub fn focus_file_handler_tab(&mut self) {
        self.settings_ui_state.select_file_handler_tab();
    }

    /// 첫 진입을 일반 > 권한으로 설정 (부팅 권한 안내의 [권한 설정 열기] 진입점).
    /// L1 만 정하는 위 둘과 달리 L2 까지 함께 정한다 — L1 만 맞추면 권한 화면이 아니라
    /// 일반 탭 기본 화면이 열리고, 그 오답은 "설정 창이 열렸다" 로는 안 보인다.
    pub fn focus_macos_permissions_tab(&mut self) {
        self.settings_ui_state.select_macos_permissions_tab();
    }

    /// debug 전용 — 첫 진입 탭을 키 문자열로 지정 (`debug.settings.open` 의 `tab` 인자).
    /// 알 수 없는 키면 `false` 를 반환하고 탭을 바꾸지 않는다.
    #[cfg(debug_assertions)]
    pub fn focus_tab(&mut self, key: &str) -> bool {
        self.settings_ui_state.select_tab_by_key(key)
    }

    /// debug 전용 — 현재 활성 L1 탭의 L2 섹션(하위탭)을 키 문자열로 지정
    /// (`debug.settings.open` 의 `subtab` 인자). [`focus_tab`] 로 L1 을 먼저
    /// 정한 뒤 호출한다. 알 수 없는 키면 `false` 를 반환하고 섹션을 바꾸지 않는다.
    #[cfg(debug_assertions)]
    pub fn focus_subtab(&mut self, key: &str) -> bool {
        self.settings_ui_state.select_section_by_key(key)
    }

    /// 단축키 가져오기/내보내기가 쓰는 plugin override 원본 · 설치 plugin 을 주입한다.
    pub fn set_plugin_bundle_context(&mut self, ctx: crate::settings_ui::PluginBundleContext) {
        self.settings_ui_state.set_plugin_bundle_context(ctx);
    }

    /// Plugin 이 contribute 한 settings sub-page 스냅샷을 주입한다. 모달 오픈 직전에
    /// host App 이 호출. 빈 vec 으로 호출하면 plugin sub-tab 이 사라진다.
    pub fn set_plugin_settings_pages(&mut self, pages: Vec<tasty_host_plugin::SettingsPageEntry>) {
        self.settings_ui_state.set_settings_pages(pages);
    }

    /// Save 중 bashrc 저장 오류를 한 번 가져온다.
    /// 설정 창은 닫히므로 App이 메인 창에 오류를 표시한다.
    pub fn take_bashrc_save_error(&mut self) -> Option<String> {
        self.settings_ui_state.bashrc_save_error.take()
    }

    /// Save로 닫았을 때만 plugin 단축키 변경 초안을 반환한다.
    /// 취소·닫기 버튼·설정 단축키로 닫으면 초안을 버린다.
    pub fn take_plugin_shortcut_draft(
        &mut self,
    ) -> std::collections::BTreeMap<
        (String, String),
        Option<crate::plugin::registry_state::ShortcutOverride>,
    > {
        let draft = std::mem::take(&mut self.settings_ui_state.plugin_shortcuts_draft);
        if self.committed {
            draft
        } else {
            std::collections::BTreeMap::new()
        }
    }
}

impl View for SettingsView {
    fn base(&self) -> &ViewBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ViewBase {
        &mut self.base
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn handle_event(&mut self, event: WindowEvent, _ctx: &mut ViewCtx<'_>) -> ViewAction {
        // 녹화 중이면 키보드 이벤트를 egui에 전달하지 않는다.
        // egui가 Cmd+C 등을 시맨틱 커맨드(Copy)로 소비하면 캡처가 안 되기 때문.
        let is_recording = self.settings_ui_state.is_recording();
        let skip_egui = is_recording && matches!(&event, WindowEvent::KeyboardInput { .. });

        // RedrawRequested는 아래에서 렌더링한다. egui 입력에도 넣으면 재그리기가 반복된다.
        let is_redraw = matches!(&event, WindowEvent::RedrawRequested);
        if !skip_egui && !is_redraw {
            let (_, egui_repaint) = self.base.gpu.handle_egui_event(&self.base.winit, &event);
            if egui_repaint {
                self.mark_dirty();
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                self.should_close = true;
                return ViewAction::Close;
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
            WindowEvent::Focused(focused) => {
                // MainView 와 **별개 인스턴스**의 detector 라 여기서도 따로 지운다.
                // 이유는 `DoubleTapDetector::reset` 주석 참조.
                self.double_tap.reset();
                if focused {
                    // 권한(TCC) 상태를 바꾸는 유일한 길은 시스템 설정이고, 이 창으로
                    // 돌아오는 것이 그 왕복의 끝이다. 권한 화면이 낡은 값을 들고 있지
                    // 않도록 여기서 다시 잰다 — macOS 외에서는 no-op.
                    crate::macos_permissions::refresh_permission_snapshot();
                    self.mark_dirty();
                }
            }
            WindowEvent::KeyboardInput { ref event, .. } => {
                use winit::event::ElementState;

                // 모달은 자체 키 이벤트를 받으므로 설정 열기 단축키로 여기서 닫는다.
                // 단축키 녹화 중에는 키를 소비하지 않는다.
                // Escape는 텍스트 편집 취소와 충돌하므로 별도 닫기 키로 처리하지 않는다.
                // 메인 창의 Escape 처리는 아직 열지 않은 설정 요청의 취소만 담당한다.
                if !is_recording
                    && event.state == ElementState::Pressed
                    && crate::adapters::ui::input::shortcuts::matches_any_binding(
                        &self.settings.keybindings.toggle_settings,
                        &event.logical_key,
                        self.base.modifiers,
                    )
                {
                    self.should_close = true;
                    return ViewAction::Close;
                }

                self.double_tap
                    .on_key_event(&event.logical_key, event.state == ElementState::Pressed);
                if event.state == ElementState::Pressed
                    && let Some(dt) = self.double_tap.take()
                {
                    self.captured_double_tap = Some(dt.binding_str().to_string());
                    self.mark_dirty();
                }

                // 녹화 중이면 winit에서 직접 키 조합 캡처. quick-switch bare-key 슬롯은
                // modifier 금지 규칙(capture_bare_key), 그 외 일반 콤보 슬롯은 modifier
                // 필수 규칙(capture_winit_key_combo)으로 분기.
                if is_recording {
                    let combo = if self.settings_ui_state.recording_is_bare_key() {
                        crate::settings_ui::capture_bare_key(event, self.base.modifiers)
                    } else {
                        crate::settings_ui::capture_winit_key_combo(event, self.base.modifiers)
                    };
                    if !matches!(combo, crate::settings_ui::KeyCapture::None) {
                        self.settings_ui_state.captured_winit_combo = Some(combo);
                        self.mark_dirty();
                    }
                }
            }
            _ => {}
        }

        if self.should_close {
            ViewAction::Close
        } else {
            ViewAction::None
        }
    }

    fn render(&mut self) {
        if !self.base.dirty {
            return;
        }
        self.base.begin_frame();

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
                settings_ui::SettingsPanelCtx {
                    settings: &mut settings,
                    ui_state,
                    captured_double_tap: captured_dt,
                    file_format: file_format.as_ref(),
                    file_handler: file_handler.as_ref(),
                    user_config_path: user_config_path.as_deref(),
                },
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
            self.committed = action == Some(true);
        }
        if let Some(msg) = self.settings_ui_state.take_import_export_toast() {
            self.toasts.push(
                msg,
                crate::adapters::ui::ToastKind::Success,
                ToastScope::Window,
            );
            self.mark_dirty();
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

impl ModalView for SettingsView {
    fn shown(&self) -> bool {
        self.shown
    }
    fn set_shown(&mut self, v: bool) {
        self.shown = v;
    }
}

impl sealed::Sealed for SettingsView {}
