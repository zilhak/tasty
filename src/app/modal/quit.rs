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
            .and_then(|id| self.view.views.get(&id))
            .map(|m| m.as_any().downcast_ref::<crate::view::QuitView>().is_some())
            .unwrap_or(false);
        if quit_modal_open {
            self.close_active_modal();
            self.begin_shutdown(event_loop);
            return;
        }

        // Get close behavior from settings
        let behavior = self
            .view
            .views
            .values()
            .find_map(|w| {
                w.as_main()
                    .map(|m| m.core_state.settings.general.close_behavior.clone())
            })
            .or_else(|| {
                self.parked_states
                    .first()
                    .map(|(_, e)| e.settings.general.close_behavior.clone())
            })
            .or_else(|| {
                self.core_state
                    .as_ref()
                    .map(|e| e.settings.general.close_behavior.clone())
            })
            .unwrap_or_else(|| "ask".to_string());

        match behavior.as_str() {
            "quit" => {
                self.begin_shutdown(event_loop);
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

    /// 종료 확인 모달을 띄우지 못했을 때의 폴백 — 확인을 건너뛰고 종료한다.
    ///
    /// 확인 절차가 생략됐다는 사실을 toast 로 알리고 `tracing::error!` 로도 남긴다.
    /// toast 는 종료 화면이 곧바로 덮으므로 best-effort 이고, 사후 진단의 실체는
    /// 파일 로그에 남는 error 라인이다 (ADR-0117).
    fn quit_without_confirmation(
        &mut self,
        context: &str,
        err: impl std::fmt::Display,
        event_loop: &ActiveEventLoop,
    ) {
        tracing::error!("{context}: {err} — quitting without the confirmation step");
        if let Some(view) = self.notice_window_mut() {
            view.state.toasts.push(
                crate::i18n::t("window_error.quit_confirm.skipped"),
                crate::adapters::ui::ToastKind::Error,
                crate::adapters::ui::ToastScope::Window,
            );
        }
        self.begin_shutdown(event_loop);
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

        // 종료 확인 모달은 다른 창들과 다르게 다룬다 — 안내 후 취소하면 사용자가
        // **앱을 끌 수 없는 상태**에 갇힌다(창 생성이 실패하는 환경은 이미 degraded 라
        // 남는 수단이 프로세스 강제 종료뿐이다). 사용자가 표명한 의도는 이미 "종료"이고
        // 확인 모달은 그 의도를 되묻는 장치일 뿐 종료를 막는 장치가 아니므로, 확인을
        // 건너뛰고 종료로 폴백한다. 생략됐다는 사실은 반드시 알린다 (ADR-0117).
        let window = match event_loop.create_window(attrs) {
            Ok(w) => std::sync::Arc::new(w),
            Err(e) => {
                self.quit_without_confirmation("failed to create quit modal window", e, event_loop);
                return;
            }
        };

        let gpu = match self.create_gpu_state(
            window.clone(),
            &crate::settings::Settings::load().appearance,
        ) {
            Ok(g) => g,
            Err(e) => {
                self.quit_without_confirmation(
                    "failed to initialize GPU for quit modal",
                    e,
                    event_loop,
                );
                return;
            }
        };

        let window_id = window.id();
        let mut modal = crate::view::QuitView::new(gpu, window);
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
