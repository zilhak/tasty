//! 화면은 항상 로컬에서 캡처하고, 처음 요청할 때 정한 로컬·mirror 클립보드로 결과를 보낸다.
//! 사용자의 선택을 기다리는 OS 캡처는 워커에서 실행한다.

use winit::window::WindowId;

use crate::app::App;
use crate::platform::screen_capture::CaptureError;
use crate::view::ui::View as _;

pub(crate) struct ScreenshotCaptureOutcome {
    pub(crate) engine: crate::runtime::engine_session::EngineId,
    pub(crate) source_view: Option<std::sync::Weak<()>>,
    pub(crate) remote_target: Option<crate::app::attach_client::RemoteTarget>,
    /// 요청 시점의 로컬 mirror workspace ID. None이면 로컬 클립보드를 사용한다.
    pub(crate) mirror_ws_id: Option<u32>,
    /// 요청의 원 View에 해당하는 창. View 없는 요청은 worker 시작 전에 버린다.
    pub(crate) source_window: Option<WindowId>,
    /// 로컬 파일 경로와, 원격 전송에만 필요한 파일 바이트.
    pub(crate) result: Result<(std::path::PathBuf, Option<Vec<u8>>), CaptureError>,
}

impl App {
    pub(crate) fn poll_screenshot_captures(&mut self) {
        self.trigger_pending_screenshot_captures();
        self.drain_screenshot_capture_results();
    }

    fn trigger_pending_screenshot_captures(&mut self) {
        self.screenshot_workers.reap();
        let mut reqs = Vec::new();
        for session in self.engines.all_sessions_mut() {
            reqs.extend(
                std::mem::take(&mut session.remote.pending_screenshot_captures)
                    .into_iter()
                    .map(|workspace| (session.id, workspace)),
            );
        }
        for (engine, (mirror_ws_id, origin_view)) in reqs {
            self.start_screenshot_capture(engine, mirror_ws_id, origin_view);
        }
    }

    fn screenshot_view_is_current(
        &self,
        source_window: Option<WindowId>,
        origin_view: &std::sync::Weak<()>,
    ) -> bool {
        source_window
            .and_then(|window| self.view.views.get(&window))
            .and_then(|view| view.as_main())
            .is_some_and(|main| main.base.state.matches_identity(origin_view))
    }

    fn start_screenshot_capture(
        &mut self,
        engine: crate::runtime::engine_session::EngineId,
        mirror_ws_id: Option<u32>,
        origin_view: std::sync::Weak<()>,
    ) {
        let source_window = self.engines.window_of(engine);
        if !self.screenshot_view_is_current(source_window, &origin_view) {
            tracing::debug!("discarding screenshot request from retired View");
            return;
        }
        let source_view = Some(origin_view);

        let remote_target =
            mirror_ws_id.and_then(|workspace| self.capture_remote_target(workspace, None));
        if mirror_ws_id.is_some() && remote_target.is_none() {
            tracing::debug!("capture mirror disappeared before start");
            return;
        }
        self.spawn_screenshot_capture(
            engine,
            source_view,
            remote_target,
            mirror_ws_id,
            source_window,
        );
    }

    fn spawn_screenshot_capture(
        &mut self,
        engine: crate::runtime::engine_session::EngineId,
        source_view: Option<std::sync::Weak<()>>,
        remote_target: Option<crate::app::attach_client::RemoteTarget>,
        mirror_ws_id: Option<u32>,
        source_window: Option<WindowId>,
    ) {
        let tx = self.screenshot_capture_tx.clone();
        let proxy = self.view.proxy.clone();
        if let Err(error) = self.screenshot_workers.spawn(move || {
            let result = capture_and_maybe_read(mirror_ws_id.is_some());
            // 수신자가 사라지면 캡처 결과를 전달할 곳이 없어 오류를 무시한다.
            let _ = tx.send(ScreenshotCaptureOutcome {
                engine,
                source_view,
                remote_target,
                mirror_ws_id,
                source_window,
                result,
            });
            // 이벤트 루프가 끝났으면 깨우기 실패를 무시한다.
            let _ = proxy.send_event(crate::AppEvent::ScreenshotCaptureReady);
        }) {
            tracing::warn!(%error,"screenshot capture admission failed");
        }
    }

    pub(crate) fn drain_screenshot_capture_results(&mut self) {
        while let Ok(outcome) = self.screenshot_capture_rx.try_recv() {
            let ScreenshotCaptureOutcome {
                engine,
                source_view,
                remote_target,
                mirror_ws_id,
                source_window,
                result,
            } = outcome;
            if self.engines.get(engine).is_none()
                || source_window.is_some_and(|window| {
                    self.engines.of_window(window) != Some(engine)
                        || self
                            .view
                            .views
                            .get(&window)
                            .and_then(|view| view.as_main())
                            .is_none_or(|main| {
                                source_view.as_ref().is_none_or(|identity| {
                                    !main.base.state.matches_identity(identity)
                                })
                            })
                })
            {
                tracing::debug!("discarding screenshot result for retired origin");
                continue;
            }
            let (path, bytes) = match result {
                Ok(v) => v,
                Err(err) => {
                    self.report_capture_failure(err, source_window);
                    continue;
                }
            };
            match mirror_ws_id {
                None => self.write_capture_to_local_clipboard(&path),
                Some(_) => {
                    if let Some(target) =
                        remote_target.filter(|target| self.remote_target_is_current(target))
                    {
                        self.upload_capture_to_mirror(&target, &path, bytes);
                    } else {
                        tracing::debug!("discarding capture for retired remote connection");
                    }
                }
            }
        }
    }

    /// 취소는 debug로만 남긴다. 권한 거절은 사용자가 해결할 수 있도록 toast도 표시한다.
    fn report_capture_failure(&mut self, err: CaptureError, source_window: Option<WindowId>) {
        if matches!(err, CaptureError::Cancelled) {
            tracing::debug!("screenshot capture cancelled by the user");
            return;
        }
        if matches!(err, CaptureError::PermissionDenied) {
            self.warn_screen_recording_permission(source_window);
        }
        tracing::warn!("screenshot capture failed: {err}");
    }

    /// 원 요청 창에만 권한 안내를 표시한다. 원 View가 없으면 로그만 남긴다.
    fn warn_screen_recording_permission(&mut self, source_window: Option<WindowId>) {
        let target = source_window.filter(|wid| {
            self.view
                .views
                .get(wid)
                .is_some_and(|view| view.as_main().is_some())
        });
        let main = target
            .and_then(|wid| self.view.views.get_mut(&wid))
            .and_then(|view| view.as_main_mut());
        let Some(main) = main else {
            return;
        };
        main.state.toasts.push(
            crate::i18n::t("toast.screen_recording_permission_required"),
            crate::adapters::ui::ToastKind::Warning,
            crate::adapters::ui::ToastScope::Window,
        );
        main.mark_dirty();
    }

    fn write_capture_to_local_clipboard(&mut self, path: &std::path::Path) {
        let path_str = path.to_string_lossy().to_string();
        if let Err(e) = self.services.clipboard_arc().write_text(&path_str) {
            tracing::warn!("screenshot capture: local clipboard write failed: {e}");
        }
    }

    fn upload_capture_to_mirror(
        &mut self,
        target: &crate::app::attach_client::RemoteTarget,
        path: &std::path::Path,
        bytes: Option<Vec<u8>>,
    ) {
        let Some(bytes) = bytes else {
            tracing::warn!("screenshot capture: mirror upload requested but no bytes were read");
            return;
        };
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "screenshot.png".to_string());
        if let Err(e) = self.forward_capture_to_remote_clipboard(target, &file_name, &bytes) {
            tracing::warn!(
                "screenshot capture: forward to mirror workspace {} failed: {e}",
                target.workspace
            );
        }
    }
}

fn capture_and_maybe_read(
    needs_bytes: bool,
) -> Result<(std::path::PathBuf, Option<Vec<u8>>), CaptureError> {
    let path = crate::platform::screen_capture::capture_interactive()?;
    let bytes = if needs_bytes {
        // 캡처 뒤 파일 읽기 실패는 사용자 취소·캡처 권한 거절과 구별한다.
        Some(std::fs::read(&path).map_err(|e| CaptureError::Tool(e.into()))?)
    } else {
        None
    };
    Ok((path, bytes))
}

/// Native capture may wait for the user. No timeout is treated as completion or OS cancellation.
#[derive(Default)]
pub(crate) struct ScreenshotWorkers {
    jobs: Vec<std::thread::JoinHandle<()>>,
    stopping: bool,
}
impl ScreenshotWorkers {
    fn spawn(&mut self, work: impl FnOnce() + Send + 'static) -> Result<(), String> {
        if self.stopping {
            return Err("screenshot capture is shutting down".into());
        }
        if self.jobs.len() >= 4 {
            return Err("screenshot capture capacity exhausted".into());
        }
        let job = std::thread::Builder::new()
            .name("screenshot-capture".into())
            .spawn(work)
            .map_err(|error| error.to_string())?;
        self.jobs.push(job);
        Ok(())
    }
    fn reap(&mut self) {
        let mut pending = Vec::new();
        for job in std::mem::take(&mut self.jobs) {
            if job.is_finished() {
                if job.join().is_err() {
                    tracing::warn!("screenshot capture worker panicked");
                }
            } else {
                pending.push(job);
            }
        }
        self.jobs = pending;
    }
    pub(crate) fn has_pending(&self) -> bool {
        !self.jobs.is_empty()
    }
    pub(crate) fn begin_shutdown(&mut self) {
        self.stopping = true;
    }
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        self.begin_shutdown();
        self.reap();
        self.jobs.len()
    }
}
impl Drop for ScreenshotWorkers {
    fn drop(&mut self) {
        let remaining = self.poll_shutdown();
        if remaining != 0 {
            tracing::warn!(
                remaining,
                "screenshot capture workers still unjoined at owner drop"
            );
        }
    }
}
