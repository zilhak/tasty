//! Plugins 모달 열기.

use std::sync::Arc;

use crate::app::App;
use crate::view;

impl App {
    /// Open the plugins modal window.
    pub(crate) fn open_plugins_modal(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        if self.view.is_modal_active() {
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

        // 모달 창·GPU 생성 실패는 패닉이 아니다 — 기존 창들을 살리고 안내만 띄운 뒤
        // 모달 열기를 취소한다.
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.notify_window_creation_failed(
                    crate::app::window_lifecycle::WindowCreationTarget::Plugins,
                    crate::app::event::WindowRequestOrigin::User,
                    "failed to create plugins window",
                    e,
                );
                return;
            }
        };

        let appearance = self
            .focused_window()
            .map(|w| w.core_state.settings.appearance.clone())
            .unwrap_or_else(|| crate::settings::Settings::load().appearance);

        let gpu = match self.create_gpu_state(window.clone(), &appearance) {
            Ok(g) => g,
            Err(e) => {
                self.notify_window_creation_failed(
                    crate::app::window_lifecycle::WindowCreationTarget::Plugins,
                    crate::app::event::WindowRequestOrigin::User,
                    "failed to initialize GPU for plugins window",
                    e,
                );
                return;
            }
        };

        let snapshot = self.snapshot_plugins();
        let modal_window_id = window.id();
        let mut modal = view::PluginsView::new(gpu, window, snapshot);
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
        tracing::info!("opened plugins modal {:?}", modal_window_id);
    }
}
