//! Settings 모달 열기와, 모달이 닫힐 때 회수되는 저장 실패의 사용자 표면.

use std::sync::Arc;

use crate::app::App;
use crate::view;
use crate::view::ui::View as _;

/// Settings 모달을 새로 만드는 데 필요한, 현재 focused window(있다면) 로부터
/// 뽑아낸 초기 데이터. `focused_window()` 유무에 따른 분기와 fallback 을
/// [`App::resolve_settings_init_data`] 안에 모아둔다.
struct SettingsInitData {
    settings: crate::settings::Settings,
    file_format: Arc<crate::file::format::FileFormatRegistry>,
    file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    user_config_path: Option<std::path::PathBuf>,
    plugin_pages: Vec<tasty_host_plugin::SettingsPageEntry>,
}

impl App {
    /// Open settings as a modal window.
    pub(crate) fn open_settings_modal(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.view.is_modal_active() {
            return; // Another modal is already open
        }

        use winit::window::WindowAttributes;

        let mut attrs = WindowAttributes::default()
            .with_title("Tasty Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(1100, 700))
            .with_min_inner_size(winit::dpi::LogicalSize::new(1100, 700))
            .with_visible(false); // Start hidden, show after first render
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        // 모달 창·GPU 생성 실패는 패닉이 아니다 — 기존 창들을 살리고 안내만 띄운 뒤
        // 모달 열기를 취소한다.
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.notify_window_creation_failed(
                    crate::app::window_lifecycle::WindowCreationTarget::Settings,
                    crate::app::event::WindowRequestOrigin::User,
                    "failed to create settings window",
                    e,
                );
                return;
            }
        };

        let init = self.resolve_settings_init_data();

        let gpu = match self.create_gpu_state(window.clone(), &init.settings.appearance) {
            Ok(g) => g,
            Err(e) => {
                self.notify_window_creation_failed(
                    crate::app::window_lifecycle::WindowCreationTarget::Settings,
                    crate::app::event::WindowRequestOrigin::User,
                    "failed to initialize GPU for settings",
                    e,
                );
                return;
            }
        };

        let modal_window_id = window.id();
        let mut modal = view::SettingsView::new(
            gpu,
            window,
            init.settings,
            init.file_format,
            init.file_handler,
            init.user_config_path,
        );
        modal.set_plugin_shortcuts(self.snapshot_plugin_shortcuts());
        modal.set_plugin_settings_pages(init.plugin_pages);
        self.apply_pending_tab_overrides(&mut modal);
        // On Windows, hidden windows do not receive RedrawRequested events,
        // so render the first frame immediately instead of waiting for the event loop.
        // On other platforms, mark_dirty() + request_redraw() is sufficient.
        #[cfg(windows)]
        {
            use crate::view::ui::View as _;
            modal.render();
        }
        #[cfg(not(windows))]
        {
            use crate::view::ui::View as _;
            modal.mark_dirty();
        }
        self.open_modal(Box::new(modal), modal_window_id);
        tracing::info!("opened settings modal {:?}", modal_window_id);
    }

    /// [`open_settings_modal`](Self::open_settings_modal) 이 필요로 하는 settings/file
    /// registry/plugin pages 초기값을 focused window(있다면) 로부터 뽑아낸다. focused
    /// window 가 없는 fallback 경로(거의 없지만 main 창 없이 settings 가 열리는 경우)는
    /// 빈 registry 와 기본 Settings 를 사용한다.
    fn resolve_settings_init_data(&self) -> SettingsInitData {
        let settings = if let Some(w) = self.focused_window() {
            w.core_state.settings.clone()
        } else {
            crate::settings::Settings::load()
        };

        let (file_format, file_handler) = if let Some(w) = self.focused_window() {
            (
                w.core_state.file_format.clone(),
                w.core_state.file_handler.clone(),
            )
        } else {
            // Settings 윈도우가 main 창 없이 열리는 경로는 거의 없지만, fallback 으로 빈 registry 를 만든다.
            // 이 경로에서는 Settings 의 FileHandler 탭이 비어 보이고 저장도 의미가 없다.
            (
                Arc::new(crate::file::format::FileFormatRegistry::new()),
                Arc::new(crate::file::handler::FileHandlerRegistry::new()),
            )
        };
        let user_config_path =
            tasty_utils::path::tasty_home().map(|d| d.join("file-handlers.toml"));
        let plugin_pages: Vec<tasty_host_plugin::SettingsPageEntry> = self
            .plugin_manager
            .as_ref()
            .map(|mgr| mgr.settings_pages.iter().cloned().collect())
            .unwrap_or_default();

        SettingsInitData {
            settings,
            file_format,
            file_handler,
            user_config_path,
            plugin_pages,
        }
    }

    /// bashrc 저장 실패를 사용자에게 보이는 표면으로 올린다.
    ///
    /// 토스트인 이유: 사용자가 방금 Save 를 눌렀으니 결과를 기다리고 있지만, 실패해도
    /// 되돌릴 조작이 없다 — 확인 버튼을 요구하는 모달은 포커스만 가져간다(ADR-0117 이
    /// 같은 기준으로 에이전트 실패를 토스트로 보낸다). 성공 토스트는 넣지 않는다:
    /// 저장은 기본 기대 동작이라 매번 알리면 소음이다.
    ///
    /// 사유를 **문구에 싣는다.** 사유에는 대상 경로와 OS 에러가 들어 있고, 그것이
    /// 빠지면 사용자가 취할 수 있는 다음 행동이 "로그를 보라" 하나로 줄어든다 — 이
    /// 티켓이 지적한 상황 그대로다. 대신 사유는 영어 개발자 문구이고 길이가 무제한이라
    /// 번역된 틀 안에 넣고 [`tasty_i18n::t_fmt_fit`] 으로 토스트 캡(200자)에 맞춘다.
    /// 그냥 넘기면 호스트가 **꼬리를 잘라**(`truncate_message`) 왜 실패했는지가
    /// 사라진다. OS 에러 원문을 번역 틀 안에 그대로 두는 것은 창 생성 실패
    /// (`window_lifecycle.rs`) 와 같은 관례다.
    pub(crate) fn surface_bashrc_save_failure(&mut self, reason: &str) {
        let Some(view) = self.notice_window_mut() else {
            tracing::error!("no main window to surface the bashrc save failure ({reason})");
            return;
        };
        view.state.toasts.push(
            bashrc_save_failure_message(reason),
            crate::adapters::ui::ToastKind::Error,
            crate::adapters::ui::ToastScope::Window,
        );
        view.mark_dirty();
    }

    /// 모달 생성 시점에 대기 중이던 탭/서브탭 진입 요청을 적용한다: Plugins 모달의
    /// Configure, file handler picker 의 "설정에서 핸들러 등록", 그리고
    /// `debug.settings.open` 이 지정한 탭/서브탭(시각 검증용, debug 빌드 전용).
    fn apply_pending_tab_overrides(&mut self, modal: &mut view::SettingsView) {
        // Plugins 모달의 Configure 진입점이 요청했으면 Plugin 탭으로 진입.
        if std::mem::take(&mut self.pending_settings_plugin_tab) {
            modal.focus_plugin_tab();
        }
        // file handler picker popup 의 "설정에서 핸들러 등록" 이 요청했으면
        // FileHandler 탭으로 진입.
        if std::mem::take(&mut self.pending_settings_file_handler_tab) {
            modal.focus_file_handler_tab();
        }
        // debug.settings.open 이 탭을 지정했으면 그 탭으로 진입 (시각 검증용).
        #[cfg(debug_assertions)]
        if let Some(tab_key) = self.pending_settings_tab.take()
            && !modal.focus_tab(&tab_key)
        {
            tracing::warn!("debug.settings.open: unknown settings tab '{tab_key}'");
        }
        // debug.settings.open 이 L2 섹션(subtab)을 지정했으면 그 섹션으로 진입.
        // L1 (focus_tab) 이후에 적용해야 활성 L1 에 맞는 섹션이 선택된다. 알 수
        // 없는 키면 해당 L1 의 기본 L2 가 유지된다.
        #[cfg(debug_assertions)]
        if let Some(subtab_key) = self.pending_settings_subtab.take()
            && !modal.focus_subtab(&subtab_key)
        {
            tracing::warn!("debug.settings.open: unknown settings subtab '{subtab_key}'");
        }
    }
}

/// 토스트 본문. [`App::surface_bashrc_save_failure`] 에서 떼어내 둔 이유는 창 없이
/// 실물 문구를 단정하기 위해서다 — 창을 요구하면 이 문구에 대한 테스트가 사라진다.
fn bashrc_save_failure_message(reason: &str) -> String {
    crate::i18n::t_fmt_fit("toast.bashrc_save_failed", reason)
}

#[cfg(test)]
mod tests {
    /// lang 파일을 전역 i18n `OnceLock`(`crate::i18n::init`) 대신 직접 읽는다 —
    /// `cargo test` 는 모든 테스트를 한 프로세스에서 돌리므로 전역 초기화는 실행
    /// 순서에 따라 다른 언어로 덮여 재현성이 없다(`keybindings_tab::label_width` 와
    /// 같은 이유). 대신 여기서는 **세 언어를 모두** 볼 수 있게 된다.
    const LANGS: &[(&str, &str)] = &[
        ("en", include_str!("../../../lang/en.toml")),
        ("ko", include_str!("../../../lang/ko.toml")),
        ("ja", include_str!("../../../lang/ja.toml")),
    ];

    /// 토스트 본문 캡(`src/adapters/ui/toast.rs` `MAX_MESSAGE_CHARS`). 넘으면 호스트가
    /// **꼬리를 자르고** "(문자 제한)" 접미를 붙인다.
    const TOAST_MAX_CHARS: usize = 200;

    fn frame(lang_toml: &str) -> String {
        let v: toml::Value = toml::from_str(lang_toml).expect("lang toml");
        v.get("toast")
            .and_then(|t| t.get("bashrc_save_failed"))
            .and_then(toml::Value::as_str)
            .expect("toast.bashrc_save_failed")
            .to_string()
    }

    /// 실제 Windows 실패가 내는 모양의 사유 — 대상 경로 + OS 에러.
    /// `save_user_bashrc_in` 의 `write {path}: {e}` 분기 그대로다.
    fn a_long_windows_reason() -> String {
        let deep = std::iter::repeat_n("VeryLongDirectoryName", 20)
            .collect::<Vec<_>>()
            .join("\\");
        format!("write C:\\{deep}\\.tasty\\bashrc.user: Access is denied. (os error 5)")
    }

    /// 세 언어의 문구가 모두 사유를 **한 번** 받는다. 자리표시자가 없으면 사유가
    /// 통째로 사라지고(`t_fmt` 는 `{}` 가 없으면 인자를 버린다) 토스트는 "저장 실패"
    /// 만 남아 이 티켓 이전과 같아진다.
    #[test]
    fn every_locale_takes_the_reason_exactly_once() {
        for (lang, toml_src) in LANGS {
            let f = frame(toml_src);
            assert_eq!(
                f.matches("{}").count(),
                1,
                "{lang}: 자리표시자가 정확히 하나여야 한다: {f}"
            );
        }
    }

    /// 어느 언어에서도 긴 사유가 캡 안에 들어가고, **어느 파일인지(머리)와 왜인지
    /// (꼬리)가 둘 다 남는다.** 호스트의 잘림에 맡기면 꼬리 — 즉 OS 에러 — 가 사라져
    /// 사용자에게 "실패했다" 만 남는다.
    #[test]
    fn a_long_reason_keeps_the_target_and_the_os_error_in_every_locale() {
        let reason = a_long_windows_reason();
        for (lang, toml_src) in LANGS {
            let f = frame(toml_src);
            let msg = tasty_i18n::fit_fragment(&reason, |r| f.replacen("{}", r, 1));

            let n = msg.chars().count();
            assert!(n <= TOAST_MAX_CHARS, "{lang}: {n} 자 — 캡 초과: {msg}");
            assert!(
                msg.contains("write C:\\VeryLongDirectoryName"),
                "{lang}: 어느 작업·어느 루트인지가 남아야 한다: {msg}"
            );
            assert!(
                msg.ends_with("(os error 5)"),
                "{lang}: 왜 실패했는지가 남아야 한다: {msg}"
            );
        }
    }

    /// 짧은 사유는 손대지 않는다 — 흔한 경우에 말줄임이 끼어들면 안 된다.
    #[test]
    fn a_short_reason_is_not_elided() {
        let reason = "tasty home directory unresolved — the edit was not persisted";
        for (lang, toml_src) in LANGS {
            let f = frame(toml_src);
            let msg = tasty_i18n::fit_fragment(reason, |r| f.replacen("{}", r, 1));
            assert!(msg.contains(reason), "{lang}: {msg}");
            assert!(!msg.contains('\u{2026}'), "{lang}: 말줄임이 끼었다: {msg}");
        }
    }

    /// **프로덕션 호출 지점**이 캡 맞춤을 실제로 거치는지. 위 세 테스트는 렌더링
    /// *방식*을 고정하지만, 호출 지점이 `t_fmt_fit` 대신 `t_fmt` 로 돌아가면 아무것도
    /// 잡지 못한다.
    ///
    /// 단정은 로케일 무관이다 — 전역 표가 어떤 언어로 초기화돼 있든(테스트 순서에
    /// 좌우된다) 세 언어 모두 캡 안에 들어가고 꼬리를 남긴다. `t_fmt` 로 되돌리면
    /// 어느 언어에서도 캡을 넘고 꼬리가 잘린다.
    #[test]
    fn the_call_site_fits_the_reason_to_the_toast_cap() {
        // 이 바이너리에서 전역 표를 세우는 유일한 곳(부팅 경로는 테스트에서 돌지
        // 않는다). 이미 세워져 있으면 no-op 이라 다른 언어여도 아래 단정은 성립한다.
        crate::i18n::init("en");

        let reason = a_long_windows_reason();
        let msg = super::bashrc_save_failure_message(&reason);

        let n = msg.chars().count();
        assert!(n <= TOAST_MAX_CHARS, "{n} 자 — 캡 초과: {msg}");
        assert!(
            msg.ends_with("(os error 5)"),
            "왜 실패했는지가 남아야 한다: {msg}"
        );
    }
}
