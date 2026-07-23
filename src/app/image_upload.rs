//! (08) mirror 터미널 이미지 paste → 원격 bulk 업로드 → 원격 경로 삽입.
//!
//! `MainView::paste_to_terminal` 의 이미지 분기가 focused surface 가 mirror workspace
//! 소속일 때 `CoreState::pending_image_uploads` 큐에 PNG 바이트 + 대상 surface 를 push
//! 한다(mirror 판별은 paste 시점에 끝내 둔다 — 업로드 완료 전에 포커스가 바뀌어도 삽입
//! 대상이 흔들리지 않게). 실제 bulk 업로드(ADR-0053, 블로킹)는 여기서 백그라운드 스레드
//! 로 수행한다.
//!
//! ```text
//! [about_to_wait] poll_image_uploads
//!   ├─ trigger_pending_image_uploads: 큐 drain → 워커 스레드 spawn
//!   │     (워커: upload_file_over_bulk 가 begin/chunk/commit → BulkResult 수신까지 블록)
//!   └─ drain_image_upload_results: 결과 채널 drain
//!        ├─ Ok(원격경로): 대상 mirror surface 입력에 dispatch_paste(원격 경로 삽입)
//!        └─ Err(사유): 삽입 없이 Warning toast(용량 초과 등 사유 포함)
//! ```
//!
//! mirror surface 입력은 detached `input_sink` → forwarder → 원격 PTY stdin 으로 투명
//! 전달되므로, 원격 경로 삽입에 별도 API 가 필요 없다 — 로컬 paste 와 동일한
//! `dispatch_paste`(`SendToSurface`)를 그대로 재사용한다.

use crate::app::App;
use crate::view::ui::View as _;

/// 업로드 워커 스레드 → 메인 루프 결과.
pub(crate) struct ImageUploadOutcome {
    /// 트리거 시점에 판별된 로컬 mirror workspace id(실패 toast 라우팅 fallback).
    pub(crate) mirror_ws_id: u32,
    /// 원격 경로를 삽입할 로컬 mirror surface id(paste 시점 포커스).
    pub(crate) surface_id: u32,
    /// 삽입 시 bracketed paste 로 감쌀지.
    pub(crate) bracketed: bool,
    /// 성공 시 원격 절대경로, 실패 시 사유(용량 초과·전송/프로토콜 에러 등).
    pub(crate) result: anyhow::Result<String>,
}

impl App {
    /// `about_to_wait` 매 프레임 — 트리거 큐를 drain 해 업로드 워커를 spawn 하고,
    /// 완료된 워커 결과를 적용한다(둘 다 cheap — 후보 없으면 즉시 반환).
    pub(crate) fn poll_image_uploads(&mut self) {
        self.trigger_pending_image_uploads();
        self.drain_image_upload_results();
    }

    /// 모든 main window + 세션리스 engine + parked state 의 `pending_image_uploads`
    /// 큐를 drain 해 각 요청마다 bulk 업로드 워커 스레드를 spawn 한다(메인 루프 무블록
    /// — bulk 업로드는 BulkResult 수신까지 블록할 수 있다). 세션 `(port, remote_ws)` 는
    /// 메인 스레드에서 미리 뽑아 넘긴다(백그라운드는 `&self` 를 들 수 없다).
    fn trigger_pending_image_uploads(&mut self) {
        let mut reqs: Vec<crate::core::PendingImageUpload> = Vec::new();
        for main in self.main_windows_iter_mut() {
            reqs.append(&mut main.core_state.pending_image_uploads);
        }
        if let Some(e) = self.core_state.as_mut() {
            reqs.append(&mut e.pending_image_uploads);
        }
        for (_, engine) in self.parked_states.iter_mut() {
            reqs.append(&mut engine.pending_image_uploads);
        }
        for req in reqs {
            let crate::core::PendingImageUpload {
                mirror_ws_id,
                surface_id,
                bracketed,
                file_name,
                png_bytes,
            } = req;
            // 세션에서 (port, remote_ws) 추출 — 없으면(정리됨) 실패로 처리한다.
            let target = self.bulk_target_for(mirror_ws_id);
            let tx = self.image_upload_tx.clone();
            let proxy = self.view.proxy.clone();
            std::thread::spawn(move || {
                let result = match target {
                    Some((port, remote_ws)) => crate::app::attach_client::upload_file_over_bulk(
                        port, remote_ws, &file_name, &png_bytes,
                    ),
                    None => Err(anyhow::anyhow!(
                        "no attach session for mirror workspace {mirror_ws_id}"
                    )),
                };
                // 수신자(메인 루프)가 종료돼 채널이 닫힌 경우에만 실패 — 무시.
                let _ = tx.send(ImageUploadOutcome {
                    mirror_ws_id,
                    surface_id,
                    bracketed,
                    result,
                });
                // event loop 가 종료된 경우에만 실패 — 무시.
                let _ = proxy.send_event(crate::AppEvent::ImageUploadReady);
            });
        }
    }

    /// 워커가 보낸 업로드 결과를 적용한다 — 성공이면 원격 경로를 대상 mirror surface
    /// 입력에 삽입(forwarder 로 원격 전달), 실패면 삽입 없이 Warning toast.
    pub(crate) fn drain_image_upload_results(&mut self) {
        while let Ok(outcome) = self.image_upload_rx.try_recv() {
            let ImageUploadOutcome {
                mirror_ws_id,
                surface_id,
                bracketed,
                result,
            } = outcome;
            match result {
                Ok(remote_path) => {
                    let Some(wid) = self.find_main_with_surface(surface_id) else {
                        tracing::warn!(
                            "image upload: mirror surface {surface_id} gone before path insertion"
                        );
                        continue;
                    };
                    if let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
                    {
                        crate::view::main::clipboard::dispatch_paste(
                            main,
                            surface_id,
                            bracketed,
                            remote_path,
                        );
                        main.mark_dirty();
                    }
                }
                Err(e) => {
                    tracing::warn!("image upload to mirror workspace {mirror_ws_id} failed: {e}");
                    let base = crate::i18n::t("attach.toast.mirror_image_upload_failed");
                    let reason = e.to_string();
                    let msg = if reason.is_empty() {
                        base.to_string()
                    } else {
                        format!("{base} ({reason})")
                    };
                    // 실패 toast 는 대상 surface 소유 창(없으면 mirror ws 소유 창)에 띄운다.
                    let target_wid = self
                        .find_main_with_surface(surface_id)
                        .or_else(|| self.find_main_with_workspace(mirror_ws_id));
                    if let Some(wid) = target_wid
                        && let Some(main) =
                            self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
                    {
                        main.state.toasts.push(
                            msg,
                            crate::adapters::ui::ToastKind::Warning,
                            crate::adapters::ui::ToastScope::Window,
                        );
                        main.mark_dirty();
                    }
                }
            }
        }
    }
}
