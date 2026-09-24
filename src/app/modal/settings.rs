//! Settings 모달을 열고 닫을 때 저장 실패를 알린다.

use std::sync::Arc;

use crate::app::App;
use crate::view;
use crate::view::ui::View as _;

struct SettingsInitData {
    settings: crate::settings::Settings,
    file_format: Arc<crate::file::format::FileFormatRegistry>,
    file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    user_config_path: Option<std::path::PathBuf>,
    plugin_pages: Vec<tasty_host_plugin::SettingsPageEntry>,
}

impl App {
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

        // 생성 실패 시 기존 창은 유지하며 설정 모달 열기만 취소한다.
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
        modal.set_plugin_bundle_context(self.plugin_bundle_context());
        modal.set_plugin_settings_pages(init.plugin_pages);
        self.apply_pending_tab_overrides(&mut modal);
        crate::view::ui::present_first_frame(&mut modal);
        self.open_modal(
            Box::new(modal),
            modal_window_id,
            crate::state::ModalKind::Settings,
        );
        tracing::info!("opened settings modal {:?}", modal_window_id);
    }

    /// 포커스 창의 설정·레지스트리를 사용한다. 없으면 설정 파일을 읽고 빈 파일 레지스트리를 만든다.
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

    /// 실패 사유를 번역 문구에 넣되 경로 가운데를 줄여 대상과 OS 오류를 함께 남긴다.
    /// 새 확인 모달 대신 toast로 알리며 성공은 따로 알리지 않는다.
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

    /// 일반 Configure 요청과 debug가 지정한 초기 탭·하위 탭을 적용한다.
    fn apply_pending_tab_overrides(&mut self, modal: &mut view::SettingsView) {
        if std::mem::take(&mut self.pending_settings_plugin_tab) {
            modal.focus_plugin_tab();
        }
        if std::mem::take(&mut self.pending_settings_file_handler_tab) {
            modal.focus_file_handler_tab();
        }
        #[cfg(debug_assertions)]
        if let Some(tab_key) = self.pending_settings_tab.take()
            && !modal.focus_tab(&tab_key)
        {
            tracing::warn!("debug.settings.open: unknown settings tab '{tab_key}'");
        }
        // 상위 탭을 고른 뒤 하위 탭을 적용한다. 알 수 없는 키면 기본 선택을 유지한다.
        #[cfg(debug_assertions)]
        if let Some(subtab_key) = self.pending_settings_subtab.take()
            && !modal.focus_subtab(&subtab_key)
        {
            tracing::warn!("debug.settings.open: unknown settings subtab '{subtab_key}'");
        }
    }
}

fn bashrc_save_failure_message(reason: &str) -> String {
    crate::i18n::t_fmt_fit("toast.bashrc_save_failed", reason)
}

#[cfg(test)]
mod tests {
    /// 전역 i18n 초기화 순서와 무관하게 세 언어의 파일을 직접 읽는다.
    const LANGS: &[(&str, &str)] = &[
        ("en", include_str!("../../../lang/en.toml")),
        ("ko", include_str!("../../../lang/ko.toml")),
        ("ja", include_str!("../../../lang/ja.toml")),
    ];

    /// 호스트 toast의 문자 수 상한과 비교한다.
    const TOAST_MAX_CHARS: usize = tasty_i18n::TOAST_MAX_CHARS;

    fn frame(lang_toml: &str) -> String {
        let v: toml::Value = toml::from_str(lang_toml).expect("lang toml");
        v.get("toast")
            .and_then(|t| t.get("bashrc_save_failed"))
            .and_then(toml::Value::as_str)
            .expect("toast.bashrc_save_failed")
            .to_string()
    }

    /// Windows 쓰기 오류의 대상 경로와 OS 오류가 함께 있는 입력.
    fn a_long_windows_reason() -> String {
        let deep = std::iter::repeat_n("VeryLongDirectoryName", 20)
            .collect::<Vec<_>>()
            .join("\\");
        format!("write C:\\{deep}\\.tasty\\bashrc.user: Access is denied. (os error 5)")
    }

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

    /// 경로 가운데를 줄여도 대상 경로 앞부분과 OS 오류 끝부분은 남아야 한다.
    #[test]
    fn a_long_reason_keeps_the_target_and_the_os_error_in_every_locale() {
        let reason = a_long_windows_reason();
        for (lang, toml_src) in LANGS {
            let f = frame(toml_src);
            let msg = tasty_i18n::fit_fragment(&reason, |r| f.replacen("{}", r, 1));

            let n = msg.chars().count();
            assert!(n <= TOAST_MAX_CHARS, "{lang}: {n} 자 — 상한 초과: {msg}");
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

    #[test]
    fn a_short_reason_is_not_elided() {
        let reason = "tasty home directory unresolved — the edit was not persisted";
        for (lang, toml_src) in LANGS {
            let f = frame(toml_src);
            let msg = tasty_i18n::fit_fragment(reason, |r| f.replacen("{}", r, 1));
            assert!(msg.contains(reason), "{lang}: {msg}");
            assert!(
                !msg.contains('\u{2026}'),
                "{lang}: 짧은 사유가 줄어들었다: {msg}"
            );
        }
    }

    /// 실제 문구 생성 함수가 길이 보정을 사용하는지도 확인한다.
    #[test]
    fn the_call_site_fits_the_reason_to_the_toast_cap() {
        // 이미 다른 언어로 초기화됐어도 길이와 오류 꼬리 검사는 같아야 한다.
        crate::i18n::init("en");

        let reason = a_long_windows_reason();
        let msg = super::bashrc_save_failure_message(&reason);

        let n = msg.chars().count();
        assert!(n <= TOAST_MAX_CHARS, "{n} 자 — 상한 초과: {msg}");
        assert!(
            msg.ends_with("(os error 5)"),
            "왜 실패했는지가 남아야 한다: {msg}"
        );
    }
}
