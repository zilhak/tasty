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

    /// Plugin 이 contribute 한 settings sub-page 스냅샷을 주입한다. 모달 오픈 직전에
    /// host App 이 호출. 빈 vec 으로 호출하면 plugin sub-tab 이 사라진다.
    pub fn set_plugin_settings_pages(&mut self, pages: Vec<tasty_host_plugin::SettingsPageEntry>) {
        self.settings_ui_state.set_settings_pages(pages);
    }

    /// Save 시 bashrc 저장이 실패했으면 그 사유를 가져간다(1 회). 호출 후에는
    /// `None` 이 남는다.
    ///
    /// 이 창이 아니라 host App 이 회수해 main window 토스트로 올린다 — Save 는
    /// 곧바로 이 창을 닫으므로(`should_close`) 여기에 띄운 토스트는 화면에
    /// 남지 않는다. 회수 경로는 [`take_plugin_shortcut_draft`](Self::take_plugin_shortcut_draft)
    /// 와 같다.
    pub fn take_bashrc_save_error(&mut self) -> Option<String> {
        self.settings_ui_state.bashrc_save_error.take()
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

        // RedrawRequested 를 egui 에 전달하면 항상 repaint=true 를 반환해
        // mark_dirty → request_redraw → RedrawRequested 무한 루프(120fps busy-loop)가
        // 된다. egui 렌더는 아래 RedrawRequested arm 의 render() 가 담당하므로
        // 이 이벤트는 egui input 으로 넘기지 않는다 (MainView 와 동일 정책).
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
            WindowEvent::Focused(_) => {
                // MainView 와 **별개 인스턴스**의 detector 라 여기서도 따로 지운다.
                // 이유는 `DoubleTapDetector::reset` 주석 참조.
                self.double_tap.reset();
            }
            WindowEvent::KeyboardInput { ref event, .. } => {
                use winit::event::ElementState;

                // 설정을 여는 바인딩이 이 창을 닫기도 한다 — 필드 이름이 `toggle_settings`
                // 이고, 여는 쪽만 구현돼 있어 이름과 동작이 어긋나 있었다. 키보드로 이 창을
                // 벗어나는 길이 없다는 것이 그 어긋남의 사용자 쪽 얼굴이다.
                //
                // 왜 여기서 닫는가: 모달 창은 자기 이벤트를 자기가 받으므로 메인 창의 단축키
                // 표에 이 키가 도달하지 않는다. 그리고 닫는 길로 `ViewAction::Close` 를 쓰는
                // 것은 **창 닫기 버튼과 정확히 같은 경로**다 — 저장/취소 cascade 가 그대로
                // 흐르므로 "키보드로 닫으면 변경이 조용히 버려지는가" 라는 물음이 생기지
                // 않는다.
                //
                // 녹화 중에는 안 한다. 그때 키는 바인딩 캡처로 가야 하고, 여기서 먹으면
                // 사용자가 그 조합을 단축키로 등록할 수 없다.
                //
                // ★ 키를 여기 박지 않는다. 무엇으로 닫히는가는 `KeybindingSettings` 가
                // 정하고, 사용자가 그 바인딩을 바꾸면 닫는 키도 함께 바뀐다.
                //
                // Escape 는 여기 **안 넣는다 — 결정이다.** 넣으려면 대응 필드가 있어야 하는데
                // `KeybindingSettings` 에 "모달 닫기" 는 없고, 그 필드를 세우는 것은 이 창만의
                // 물음이 아니다. 이 창 안에서만 봐도 미해결 충돌이 하나 있다: 설정에는 편집
                // 가능한 텍스트 필드가 여러 탭에 걸쳐 있고(`ui/file_handler_tab/` ·
                // `ui/tabs/appearance.rs` · `ui/tabs/misc.rs` · `ui/keybindings_tab/plugins.rs`),
                // 여기서 먼저 먹으면 편집 중 Escape 가 "편집 취소" 가 아니라 "창 닫기" 가 된다.
                // 그 물음이 값으로 정해지기 전에는 안 넣는다. 넣는 날 고칠 곳은 아래 조건 하나뿐
                // 이다 — 이 자리가 키가 아니라 **바인딩**을 보기 때문이다.
                //
                // ★ 메인 창의 `try_consume_escape_key` 가 이 자리를 대신하고 있지 않다. 그쪽
                // 첫 소비자의 조건 `settings_open_requested` 는 "열려 있는가" 가 아니라 **열기
                // 요청 래치**이고(`src/view/main/redraw.rs` 가 같은 프레임에 소비해 false 로
                // 되돌린다), 모달이 떠 있는 동안은 false 다. 즉 Escape 로 이 창을 닫는 경로는
                // 어디에도 배선돼 있지 않다.
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
