//! Quit 모달 — close_behavior=="ask" 경로의 확인 다이얼로그.

use winit::event_loop::ActiveEventLoop;

use crate::AppEvent;
use crate::app::App;

impl App {
    pub(crate) fn handle_quit_requested(&mut self, event_loop: &ActiveEventLoop) {
        // If a quit modal is already open, treat as immediate quit
        let quit_modal_open = self
            .view
            .active_modal_id
            .and_then(|id| self.view.windows.get(&id))
            .map(|m| {
                m.as_any()
                    .downcast_ref::<crate::adapters::ui::window::QuitWindow>()
                    .is_some()
            })
            .unwrap_or(false);
        if quit_modal_open {
            self.close_active_modal();
            self.flush_layout_persistence(true);
            event_loop.exit();
            return;
        }

        // Get close behavior from settings
        let behavior = self
            .view
            .windows
            .values()
            .find_map(|w| {
                w.as_main()
                    .map(|m| m.engine_state.settings.general.close_behavior.clone())
            })
            .or_else(|| {
                self.parked_states
                    .first()
                    .map(|(_, e)| e.settings.general.close_behavior.clone())
            })
            .or_else(|| {
                self.engine_state
                    .as_ref()
                    .map(|e| e.settings.general.close_behavior.clone())
            })
            .unwrap_or_else(|| "ask".to_string());

        match behavior.as_str() {
            "quit" => {
                self.flush_layout_persistence(true);
                event_loop.exit();
            }
            "minimize" => {
                crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::Minimize);
            }
            _ => {
                // "ask" — close any existing modal, then show quit modal
                self.close_active_modal();
                self.open_quit_modal(event_loop);
            }
        }
    }

    pub(crate) fn open_quit_modal(&mut self, event_loop: &ActiveEventLoop) {
        use winit::window::WindowAttributes;

        let mut attrs = WindowAttributes::default()
            .with_title("Tasty")
            .with_inner_size(winit::dpi::LogicalSize::new(400, 200))
            .with_resizable(false)
            .with_visible(false);
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = std::sync::Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create quit modal window"),
        );

        let gpu = pollster::block_on(crate::gpu::GpuState::new(
            window.clone(),
            &crate::settings::Settings::load().appearance,
            self.view.proxy.clone(),
        ))
        .expect("failed to initialize GPU for quit modal");

        let window_id = window.id();
        let mut modal = crate::adapters::ui::window::QuitWindow::new(gpu, window);
        // On Windows, hidden windows do not receive RedrawRequested events,
        // so render the first frame immediately to make the modal visible.
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
        self.open_modal(Box::new(modal), window_id);
    }
}
