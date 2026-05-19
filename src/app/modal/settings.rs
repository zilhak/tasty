//! Settings 모달 열기.

use std::sync::Arc;

use crate::app::App;
use crate::window;

impl App {
    /// Open settings as a modal window.
    pub(crate) fn open_settings_modal(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        if self.engine.is_modal_active() {
            return; // Another modal is already open
        }

        use winit::window::WindowAttributes;

        let mut attrs = WindowAttributes::default()
            .with_title("Tasty Settings")
            .with_inner_size(winit::dpi::LogicalSize::new(960, 640))
            .with_min_inner_size(winit::dpi::LogicalSize::new(960, 640))
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
            w.state.engine.settings.clone()
        } else {
            crate::settings::Settings::load()
        };

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &settings.appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU for settings");

        let modal_window_id = window.id();
        let (file_format, file_handler) = if let Some(w) = self.focused_window() {
            (
                w.state.engine.file_format.clone(),
                w.state.engine.file_handler.clone(),
            )
        } else {
            // Settings 윈도우가 main 창 없이 열리는 경로는 거의 없지만, fallback 으로 빈 registry 를 만든다.
            // 이 경로에서는 Settings 의 FileHandler 탭이 비어 보이고 저장도 의미가 없다.
            (
                Arc::new(crate::file_format::FileFormatRegistry::new()),
                Arc::new(crate::file_handler::FileHandlerRegistry::new()),
            )
        };
        let user_config_path = tasty_core::paths::tasty_home().map(|d| d.join("file-handlers.toml"));
        let mut modal = window::SettingsWindow::new(
            gpu,
            window,
            settings,
            file_format,
            file_handler,
            user_config_path,
        );
        modal.set_plugin_shortcuts(self.snapshot_plugin_shortcuts());
        // On Windows, hidden windows do not receive RedrawRequested events,
        // so render the first frame immediately instead of waiting for the event loop.
        // On other platforms, mark_dirty() + request_redraw() is sufficient.
        #[cfg(windows)]
        {
            use window::Window as _;
            modal.render();
        }
        #[cfg(not(windows))]
        {
            use window::Window as _;
            modal.mark_dirty();
        }
        self.open_modal(Box::new(modal), modal_window_id);
        tracing::info!("opened settings modal {:?}", modal_window_id);
    }
}
