//! 화면은 항상 로컬에서 캡처하고, 처음 요청할 때 정한 로컬·mirror 클립보드로 결과를 보낸다.
//! 사용자의 선택을 기다리는 OS 캡처는 워커에서 실행한다.

use winit::window::WindowId;

use crate::app::App;
use crate::platform::screen_capture::CaptureError;
use crate::view::ui::View as _;

pub(crate) struct ScreenshotCaptureOutcome {
    /// 요청 시점의 로컬 mirror workspace ID. None이면 로컬 클립보드를 사용한다.
    pub(crate) mirror_ws_id: Option<u32>,
    /// 실패를 알릴 원래 창. 부팅 중·parked 요청은 None이다.
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
        let mut reqs: Vec<(Option<WindowId>, Option<u32>)> = Vec::new();
        for (wid, view) in self.view.views.iter_mut() {
            let Some(main) = view.as_main_mut() else {
                continue;
            };
            for mirror_ws_id in main.core_state.pending_screenshot_captures.drain(..) {
                reqs.push((Some(*wid), mirror_ws_id));
            }
        }
        if let Some(e) = self.core_state.as_mut() {
            for mirror_ws_id in e.pending_screenshot_captures.drain(..) {
                reqs.push((None, mirror_ws_id));
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            for mirror_ws_id in engine.pending_screenshot_captures.drain(..) {
                reqs.push((None, mirror_ws_id));
            }
        }
        for (source_window, mirror_ws_id) in reqs {
            let tx = self.screenshot_capture_tx.clone();
            let proxy = self.view.proxy.clone();
            std::thread::spawn(move || {
                let result = capture_and_maybe_read(mirror_ws_id.is_some());
                // 수신자가 사라지면 캡처 결과를 전달할 곳이 없어 오류를 무시한다.
                let _ = tx.send(ScreenshotCaptureOutcome {
                    mirror_ws_id,
                    source_window,
                    result,
                });
                // 이벤트 루프가 끝났으면 깨우기 실패를 무시한다.
                let _ = proxy.send_event(crate::AppEvent::ScreenshotCaptureReady);
            });
        }
    }

    pub(crate) fn drain_screenshot_capture_results(&mut self) {
        while let Ok(outcome) = self.screenshot_capture_rx.try_recv() {
            let ScreenshotCaptureOutcome {
                mirror_ws_id,
                source_window,
                result,
            } = outcome;
            let (path, bytes) = match result {
                Ok(v) => v,
                Err(err) => {
                    self.report_capture_failure(err, source_window);
                    continue;
                }
            };
            match mirror_ws_id {
                None => self.write_capture_to_local_clipboard(&path),
                Some(ws_id) => self.upload_capture_to_mirror(ws_id, &path, bytes),
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

    /// 요청한 창이 사라졌으면 다른 MainView에 권한 안내를 표시한다. 창이 없으면 로그만 남는다.
    fn warn_screen_recording_permission(&mut self, source_window: Option<WindowId>) {
        let target = source_window.filter(|wid| {
            self.view
                .views
                .get(wid)
                .is_some_and(|view| view.as_main().is_some())
        });
        let main = match target {
            Some(wid) => self.view.views.get_mut(&wid).and_then(|v| v.as_main_mut()),
            None => self.main_windows_iter_mut().next(),
        };
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
        if let Err(e) = self.core.clipboard_arc().write_text(&path_str) {
            tracing::warn!("screenshot capture: local clipboard write failed: {e}");
        }
    }

    fn upload_capture_to_mirror(
        &mut self,
        ws_id: u32,
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
        if let Err(e) = self.forward_capture_to_remote_clipboard(ws_id, &file_name, &bytes) {
            tracing::warn!("screenshot capture: forward to mirror workspace {ws_id} failed: {e}");
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
