//! (03) 원격 attach(mirror) surface 대상 스크린샷→클립보드.
//!
//! 신규 키바인딩(`KeybindingSettings::screenshot_to_clipboard`) 트리거 → 포커스된
//! surface 기준으로 로컬/mirror 를 판별(`match_capture_bindings`, 키바인딩 시점에
//! 끝낸다 — 캡처가 끝나기 전에 포커스가 바뀌어도 판정이 흔들리지 않게) →
//! `CoreState::pending_screenshot_captures` 큐에 `Option<u32>`(mirror 라면
//! 로컬 mirror workspace id) push.
//!
//! ```text
//! [about_to_wait] poll_screenshot_captures
//!   ├─ trigger_pending_screenshot_captures: 큐 drain → 워커 스레드 spawn
//!   │     (워커: OS 네이티브 인터랙티브 캡처 = 사용자가 선택을 마칠 때까지 블록
//!   │      → 메인 루프는 무블록)
//!   └─ drain_screenshot_capture_results: 결과 채널 drain
//!        ├─ 로컬(mirror_ws_id=None): 로컬 클립보드에 경로 기록
//!        └─ mirror(mirror_ws_id=Some(ws)): attach 세션으로 업로드
//!           (`App::forward_capture_to_remote_clipboard`, attach_client.rs)
//! ```
//!
//! 캡처 자체(OS 프로세스 spawn+대기)는 항상 **로컬**(사용자가 지금 보고 있는 화면)
//! 에서 일어난다 — mirror 여부는 "찍은 파일을 어디 클립보드에 연결할지"만 가른다.

use winit::window::WindowId;

use crate::app::App;
use crate::platform::screen_capture::CaptureError;
use crate::view::ui::View as _;

/// 캡처 워커 스레드 → 메인 루프 결과.
pub(crate) struct ScreenshotCaptureOutcome {
    /// 트리거 시점에 판별된 대상. `Some(local mirror workspace id)` 면 원격 전송,
    /// `None` 이면 로컬 클립보드.
    pub(crate) mirror_ws_id: Option<u32>,
    /// 요청이 올라온 윈도우. 실패 안내 토스트를 그 윈도우에 띄우기 위한 것으로,
    /// 부팅 중/parked 상태의 큐에서 온 요청은 소속 윈도우가 없어 `None`.
    pub(crate) source_window: Option<WindowId>,
    /// 캡처 성공 시 `(로컬 파일 경로, mirror 케이스에 한해 읽어둔 파일 바이트)`.
    /// 로컬 케이스는 바이트가 필요 없어 `None`(디스크 재읽기/메모리 낭비 방지).
    pub(crate) result: Result<(std::path::PathBuf, Option<Vec<u8>>), CaptureError>,
}

impl App {
    /// `about_to_wait` 매 프레임 — 트리거 큐를 drain 해 캡처 워커를 spawn 하고,
    /// 완료된 워커 결과를 적용한다(둘 다 cheap — 후보 없으면 즉시 반환).
    pub(crate) fn poll_screenshot_captures(&mut self) {
        self.trigger_pending_screenshot_captures();
        self.drain_screenshot_capture_results();
    }

    /// 모든 main window + parked state 의 `pending_screenshot_captures` 큐를
    /// drain 해 각 요청마다 캡처 워커 스레드를 spawn 한다(메인 루프 무블록 — OS
    /// 인터랙티브 캡처는 사용자가 선택을 마칠 때까지 블록할 수 있다).
    fn trigger_pending_screenshot_captures(&mut self) {
        // 요청과 함께 **어느 윈도우에서 왔는지**를 들고 다닌다 — 실패 안내를 트리거한
        // 윈도우에 돌려주기 위함(다중 윈도우 세션에서 엉뚱한 창에 뜨지 않게).
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
                let _ = tx.send(ScreenshotCaptureOutcome {
                    mirror_ws_id,
                    source_window,
                    result,
                }); // 수신자(메인 루프) drop 시에만 실패 — 무시.
                let _ = proxy.send_event(crate::AppEvent::ScreenshotCaptureReady); // event loop 종료 시에만 실패 — 무시
            });
        }
    }

    /// 워커가 보낸 캡처 결과를 적용한다 — 로컬이면 로컬 클립보드에 경로를 기록,
    /// mirror 면 그 mirror workspace 의 attach 세션으로 파일을 업로드한다.
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

    /// 캡처 실패를 사유별로 처리한다 — 취소는 정상 흐름이라 debug 로만, 나머지는
    /// warn. 권한 미승인은 사용자가 손쓸 수 있는 상태라 안내 토스트까지 띄운다.
    fn report_capture_failure(&mut self, err: CaptureError, source_window: Option<WindowId>) {
        // 사용자가 의도적으로 취소한 정상 흐름 — 경고할 일이 아니다.
        if matches!(err, CaptureError::Cancelled) {
            tracing::debug!("screenshot capture cancelled by the user");
            return;
        }
        if matches!(err, CaptureError::PermissionDenied) {
            // 조용히 넘기면 사용자에겐 취소와 똑같이 "아무 일도 안 일어남" 으로 보인다.
            self.warn_screen_recording_permission(source_window);
        }
        tracing::warn!("screenshot capture failed: {err}");
    }

    /// 화면 기록 권한 미승인을 사용자에게 알린다. 요청이 올라온 윈도우에 띄우고,
    /// 그 윈도우를 못 찾으면(부팅 중/parked 큐에서 온 요청, 창이 이미 닫힘) 아무
    /// 메인 윈도우에나 띄운다 — 안내를 통째로 잃는 것보다 낫다.
    fn warn_screen_recording_permission(&mut self, source_window: Option<WindowId>) {
        // 대상 결정과 가변 대여를 분리한다 — 두 후보를 한 체인에서 잇으면 `self` 를
        // 두 번 가변 대여하게 된다.
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
            return; // 띄울 창이 없다 — 위 warn! 로그가 유일한 흔적.
        };
        main.state.toasts.push(
            crate::i18n::t("toast.screen_recording_permission_required"),
            crate::adapters::ui::ToastKind::Warning,
            crate::adapters::ui::ToastScope::Window,
        );
        main.mark_dirty();
    }

    /// 로컬 클립보드에 캡처 파일 경로를 기록한다.
    fn write_capture_to_local_clipboard(&mut self, path: &std::path::Path) {
        let path_str = path.to_string_lossy().to_string();
        if let Err(e) = self.core.clipboard_arc().write_text(&path_str) {
            tracing::warn!("screenshot capture: local clipboard write failed: {e}");
        }
    }

    /// mirror workspace 의 attach 세션으로 캡처 파일을 업로드한다.
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

/// 워커 스레드 본체 — OS 인터랙티브 캡처(블록) 후, mirror 업로드가 필요하면
/// 파일을 읽어둔다.
fn capture_and_maybe_read(
    needs_bytes: bool,
) -> Result<(std::path::PathBuf, Option<Vec<u8>>), CaptureError> {
    let path = crate::platform::screen_capture::capture_interactive()?;
    let bytes = if needs_bytes {
        // 캡처는 끝났는데 읽기가 실패한 것이라 취소도 권한 문제도 아니다.
        Some(std::fs::read(&path).map_err(|e| CaptureError::Tool(e.into()))?)
    } else {
        None
    };
    Ok((path, bytes))
}
