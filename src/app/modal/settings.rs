//! Settings 모달 열기.

use std::sync::Arc;

use crate::app::App;
use crate::view;

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

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create settings window"),
        );

        let settings = if let Some(w) = self.focused_window() {
            w.core_state.settings.clone()
        } else {
            crate::settings::Settings::load()
        };

        let gpu = self
            .create_gpu_state(window.clone(), &settings.appearance)
            .expect("failed to initialize GPU for settings");

        let modal_window_id = window.id();
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
        let mut modal = view::SettingsView::new(
            gpu,
            window,
            settings,
            file_format,
            file_handler,
            user_config_path,
        );
        modal.set_plugin_shortcuts(self.snapshot_plugin_shortcuts());
        let plugin_pages: Vec<tasty_host_plugin::SettingsPageEntry> = self
            .plugin_manager
            .as_ref()
            .map(|mgr| mgr.settings_pages.iter().cloned().collect())
            .unwrap_or_default();
        modal.set_plugin_settings_pages(plugin_pages);
        // Plugins 모달의 Configure 진입점이 요청했으면 Plugin 탭으로 진입.
        if std::mem::take(&mut self.pending_settings_plugin_tab) {
            modal.focus_plugin_tab();
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
}
