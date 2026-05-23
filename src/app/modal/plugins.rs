//! Plugins 모달 열기.

use std::sync::Arc;

use crate::app::App;
use crate::window;

impl App {
    /// Open the plugins modal window.
    pub(crate) fn open_plugins_modal(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.engine.is_modal_active() {
            return;
        }

        use winit::window::WindowAttributes;

        let mut attrs = WindowAttributes::default()
            .with_title("Tasty Plugins")
            .with_inner_size(winit::dpi::LogicalSize::new(880, 560))
            .with_min_inner_size(winit::dpi::LogicalSize::new(720, 480))
            .with_visible(false);
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create plugins window"),
        );

        let appearance = self
            .focused_window()
            .map(|w| w.engine_state.settings.appearance.clone())
            .unwrap_or_else(|| crate::settings::Settings::load().appearance);

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &appearance,
            self.engine.proxy.clone(),
        ))
        .expect("failed to initialize GPU for plugins window");

        let snapshot = self.snapshot_plugins();
        let modal_window_id = window.id();
        let mut modal = window::PluginsWindow::new(gpu, window, snapshot);
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
        tracing::info!("opened plugins modal {:?}", modal_window_id);
    }
}
