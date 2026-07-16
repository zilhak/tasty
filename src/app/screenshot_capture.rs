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

use crate::app::App;

/// 캡처 워커 스레드 → 메인 루프 결과.
pub(crate) struct ScreenshotCaptureOutcome {
    /// 트리거 시점에 판별된 대상. `Some(local mirror workspace id)` 면 원격 전송,
    /// `None` 이면 로컬 클립보드.
    pub(crate) mirror_ws_id: Option<u32>,
    /// 캡처 성공 시 `(로컬 파일 경로, mirror 케이스에 한해 읽어둔 파일 바이트)`.
    /// 로컬 케이스는 바이트가 필요 없어 `None`(디스크 재읽기/메모리 낭비 방지).
    pub(crate) result: anyhow::Result<(std::path::PathBuf, Option<Vec<u8>>)>,
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
        let mut reqs: Vec<Option<u32>> = Vec::new();
        for main in self.main_windows_iter_mut() {
            reqs.append(&mut main.core_state.pending_screenshot_captures);
        }
        if let Some(e) = self.core_state.as_mut() {
            reqs.append(&mut e.pending_screenshot_captures);
        }
        for (_, engine) in self.parked_states.iter_mut() {
            reqs.append(&mut engine.pending_screenshot_captures);
        }
        for mirror_ws_id in reqs {
            let tx = self.screenshot_capture_tx.clone();
            let proxy = self.view.proxy.clone();
            std::thread::spawn(move || {
                let result = capture_and_maybe_read(mirror_ws_id.is_some());
                let _ = tx.send(ScreenshotCaptureOutcome {
                    mirror_ws_id,
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
            let ScreenshotCaptureOutcome { mirror_ws_id, result } = outcome;
            let (path, bytes) = match result {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("screenshot capture failed: {e}");
                    continue;
                }
            };
            match mirror_ws_id {
                None => {
                    let path_str = path.to_string_lossy().to_string();
                    if let Err(e) = self.core.clipboard_arc().write_text(&path_str) {
                        tracing::warn!("screenshot capture: local clipboard write failed: {e}");
                    }
                }
                Some(ws_id) => {
                    let Some(bytes) = bytes else {
                        tracing::warn!(
                            "screenshot capture: mirror upload requested but no bytes were read"
                        );
                        continue;
                    };
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "screenshot.png".to_string());
                    if let Err(e) =
                        self.forward_capture_to_remote_clipboard(ws_id, &file_name, &bytes)
                    {
                        tracing::warn!(
                            "screenshot capture: forward to mirror workspace {ws_id} failed: {e}"
                        );
                    }
                }
            }
        }
    }
}

/// 워커 스레드 본체 — OS 인터랙티브 캡처(블록) 후, mirror 업로드가 필요하면
/// 파일을 읽어둔다.
fn capture_and_maybe_read(
    needs_bytes: bool,
) -> anyhow::Result<(std::path::PathBuf, Option<Vec<u8>>)> {
    let path = crate::platform::screen_capture::capture_interactive()?;
    let bytes = if needs_bytes {
        Some(std::fs::read(&path)?)
    } else {
        None
    };
    Ok((path, bytes))
}
