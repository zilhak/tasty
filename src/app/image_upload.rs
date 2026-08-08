//! (08) mirror 터미널 이미지 paste → 원격 bulk 업로드 → 원격 경로 삽입.
//! (09) 그 업로드에 진행 팝업(determinate) + 실패 팝업(승격)을 배선한다.
//!
//! `MainView::paste_to_terminal` 의 이미지 분기가 focused surface 가 mirror workspace
//! 소속일 때 `CoreState::pending_image_uploads` 큐에 PNG 바이트 + 대상 surface 를 push
//! 한다(mirror 판별은 paste 시점에 끝내 둔다 — 업로드 완료 전에 포커스가 바뀌어도 삽입
//! 대상이 흔들리지 않게). 실제 bulk 업로드(ADR-0054, 블로킹)는 여기서 백그라운드 스레드
//! 로 수행한다.
//!
//! ```text
//! [about_to_wait] poll_image_uploads
//!   ├─ trigger_pending_image_uploads: 큐 drain → (09) 진행 행 추가 + 진행 팝업 open →
//!   │     워커 스레드 spawn (워커: upload_file_over_bulk 가 begin/chunk/commit →
//!   │     BulkResult 수신까지 블록, 청크마다 on_progress → 진행 채널)
//!   └─ drain_image_upload_results: 결과 채널 drain
//!        ├─ (09) 진행 행 제거(비면 진행 팝업 close)
//!        ├─ Ok(원격경로): 대상 mirror surface 입력에 dispatch_paste(원격 경로 삽입)
//!        └─ Err(사유): (09) 실패 팝업으로 승격(전송 중 실패=Retry / 원격 거부=Dismiss)
//! [AppEvent::TransferProgressTick] drain_transfer_progress: 진행 이벤트 → 행 갱신
//! ```
//!
//! mirror surface 입력은 detached `input_sink` → forwarder → 원격 PTY stdin 으로 투명
//! 전달되므로, 원격 경로 삽입에 별도 API 가 필요 없다 — 로컬 paste 와 동일한
//! `dispatch_paste`(`SendToSurface`)를 그대로 재사용한다.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::adapters::ui::popup::transfer::{
    TRANSFER_ERROR_POPUP_ID, TRANSFER_PROGRESS_POPUP_ID, TransferError, TransferProgress,
    TransferRow,
};
use crate::app::App;
use crate::view::ui::View as _;

/// (09) UI 진행 행 상관 id 발급기 — bulk transfer_id 와 독립(순수 UI 상관용). 진행
/// 채널 메시지가 이 id 로 행을 지목한다.
static NEXT_UI_TRANSFER_ID: AtomicU64 = AtomicU64::new(1);

fn next_ui_transfer_id() -> u64 {
    NEXT_UI_TRANSFER_ID.fetch_add(1, Ordering::Relaxed)
}

/// 업로드 워커 스레드 → 메인 루프 결과.
pub(crate) struct ImageUploadOutcome {
    /// 트리거 시점에 판별된 로컬 mirror workspace id(팝업 라우팅 fallback).
    pub(crate) mirror_ws_id: u32,
    /// 원격 경로를 삽입할 로컬 mirror surface id(paste 시점 포커스).
    pub(crate) surface_id: u32,
    /// 삽입 시 bracketed paste 로 감쌀지.
    pub(crate) bracketed: bool,
    /// (09) 이 업로드의 UI 진행 행 id — 완료 시 행 제거에 사용.
    pub(crate) transfer_id: u64,
    /// (09) 실패 팝업 표시 + 재시도 재구성용 파일명.
    pub(crate) file_name: String,
    /// (09) 재시도 재전송용 원본 바이트(Ok 면 드롭, Err+retryable 이면 재큐잉).
    pub(crate) png_bytes: Vec<u8>,
    /// 성공 시 원격 절대경로, 실패 시 사유(용량 초과·전송/프로토콜 에러 등).
    pub(crate) result: anyhow::Result<String>,
}

/// (09) 업로드 워커 → 메인 루프 진행 이벤트. `on_progress` 콜백이 청크마다 보낸다.
pub(crate) struct TransferProgressMsg {
    /// 대상 UI 진행 행 id.
    pub(crate) id: u64,
    /// 지금까지 전송한 바이트.
    pub(crate) sent: u64,
    /// 총 바이트.
    pub(crate) total: u64,
    /// 표시용 전송 속도(워커가 평균으로 계산).
    pub(crate) rate: String,
}

impl App {
    /// `about_to_wait` 매 프레임 — 트리거 큐를 drain 해 업로드 워커를 spawn 하고,
    /// 완료된 워커 결과를 적용한다(둘 다 cheap — 후보 없으면 즉시 반환).
    pub(crate) fn poll_image_uploads(&mut self) {
        self.trigger_pending_image_uploads();
        self.drain_image_upload_results();
    }

    /// 모든 main window + 세션리스 engine + parked state 의 `pending_image_uploads`
    /// 큐를 drain 해 각 요청마다 (09) 진행 행을 추가·진행 팝업을 열고 bulk 업로드 워커
    /// 스레드를 spawn 한다(메인 루프 무블록 — bulk 업로드는 BulkResult 수신까지 블록할
    /// 수 있다). 세션 `(port, remote_ws)` 는 메인 스레드에서 미리 뽑아 넘긴다(백그라운드는
    /// `&self` 를 들 수 없다).
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
            let transfer_id = next_ui_transfer_id();
            let total = png_bytes.len() as u64;
            // (09) 진행 행 추가 + 진행 팝업 open(대상 surface 소유 창에).
            self.begin_transfer_progress_row(surface_id, transfer_id, &file_name, total);
            // 세션에서 (port, remote_ws) 추출 — 없으면(정리됨) 실패로 처리한다.
            let target = self.bulk_target_for(mirror_ws_id);
            let tx = self.image_upload_tx.clone();
            let progress_tx = self.transfer_progress_tx.clone();
            let proxy = self.view.proxy.clone();
            std::thread::spawn(move || {
                let start = std::time::Instant::now();
                let result = match target {
                    Some((port, remote_ws)) => {
                        let progress_tx = progress_tx.clone();
                        let proxy = proxy.clone();
                        crate::app::attach_client::upload_file_over_bulk(
                            port,
                            remote_ws,
                            &file_name,
                            &png_bytes,
                            move |sent, tot| {
                                // 평균 전송률(누적/경과) — 순간율의 노이즈를 피한다.
                                let rate = format_rate(sent, start.elapsed());
                                // 수신자 종료/이벤트루프 종료 시에만 실패 — 무시.
                                let _ = progress_tx.send(TransferProgressMsg {
                                    id: transfer_id,
                                    sent,
                                    total: tot,
                                    rate,
                                });
                                // 이벤트 루프 종료 시에만 실패 — 무시.
                                let _ = proxy.send_event(crate::AppEvent::TransferProgressTick);
                            },
                        )
                    }
                    None => Err(anyhow::anyhow!(
                        "no attach session for mirror workspace {mirror_ws_id}"
                    )),
                };
                // 수신자(메인 루프)가 종료돼 채널이 닫힌 경우에만 실패 — 무시.
                let _ = tx.send(ImageUploadOutcome {
                    mirror_ws_id,
                    surface_id,
                    bracketed,
                    transfer_id,
                    file_name,
                    png_bytes,
                    result,
                });
                // event loop 가 종료된 경우에만 실패 — 무시.
                let _ = proxy.send_event(crate::AppEvent::ImageUploadReady);
            });
        }
    }

    /// (09) `AppEvent::TransferProgressTick` — 워커 진행 이벤트를 해당 행에 적용한다.
    /// 어느 창의 `transfer_progress` 가 그 행 id 를 갖는지 순회로 찾는다(행 id 는 전역
    /// 유일). 취소돼 행이 없으면(팝업 dismiss) 조용히 무시.
    pub(crate) fn drain_transfer_progress(&mut self) {
        let mut msgs: Vec<TransferProgressMsg> = Vec::new();
        while let Ok(m) = self.transfer_progress_rx.try_recv() {
            msgs.push(m);
        }
        if msgs.is_empty() {
            return;
        }
        for m in msgs {
            for w in self.view.views.values_mut() {
                if let Some(main) = w.as_main_mut()
                    && let Some(prog) = main.state.dialogs.transfer_progress.as_mut()
                    && let Some(row) = prog.row_by_id(m.id)
                {
                    row.sent = m.sent;
                    row.total = m.total;
                    row.rate = m.rate;
                    main.mark_dirty();
                    break;
                }
            }
        }
    }

    /// (09) 진행 행 하나를 대상 surface 소유 창에 추가하고 진행 팝업을 연다(없으면 새로).
    /// 팝업은 focus 를 훔치지 않게 `open_centered`(사용자가 계속 타이핑 가능; 클릭은
    /// focus 없이도 동작).
    fn begin_transfer_progress_row(
        &mut self,
        surface_id: u32,
        transfer_id: u64,
        file_name: &str,
        total: u64,
    ) {
        let Some(wid) = self.find_main_with_surface(surface_id) else {
            return;
        };
        if let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) {
            let prog = main
                .state
                .dialogs
                .transfer_progress
                .get_or_insert_with(TransferProgress::default);
            prog.rows.push(TransferRow {
                id: transfer_id,
                name: file_name.to_string(),
                sent: 0,
                total,
                rate: String::new(),
            });
            main.state.popups.open_centered(TRANSFER_PROGRESS_POPUP_ID);
            main.mark_dirty();
        }
    }

    /// 워커가 보낸 업로드 결과를 적용한다 — (09) 진행 행 제거(비면 진행 팝업 close) 후
    /// 성공이면 원격 경로를 대상 mirror surface 입력에 삽입(forwarder 로 원격 전달),
    /// 실패면 (09) 실패 팝업으로 승격한다(원격 거부=Dismiss / 전송 중 실패=Retry).
    pub(crate) fn drain_image_upload_results(&mut self) {
        while let Ok(outcome) = self.image_upload_rx.try_recv() {
            let ImageUploadOutcome {
                mirror_ws_id,
                surface_id,
                bracketed,
                transfer_id,
                file_name,
                png_bytes,
                result,
            } = outcome;
            // (09) 진행 행 제거 + 비면 진행 팝업 self-close.
            self.finish_transfer_progress_row(surface_id, mirror_ws_id, transfer_id);
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
                    // (09) 원격 거부(BULK_REJECT_PREFIX)면 재시도 무의미(Dismiss 단독) — 접두
                    // 를 벗겨 clean reason 만 표시. 그 외(전송/프로토콜 에러)는 재시도 가능.
                    let raw = e.to_string();
                    let (retryable, reason) =
                        match raw.strip_prefix(crate::app::attach_client::BULK_REJECT_PREFIX) {
                            Some(clean) => (false, clean.to_string()),
                            None => (true, raw),
                        };
                    let name = file_name.clone();
                    let retry = if retryable {
                        Some(crate::core::PendingImageUpload {
                            mirror_ws_id,
                            surface_id,
                            bracketed,
                            file_name,
                            png_bytes,
                        })
                    } else {
                        // 재시도 불가 — 원본 바이트/파일명은 여기서 드롭.
                        None
                    };
                    self.push_transfer_error(surface_id, mirror_ws_id, name, reason, retry);
                }
            }
        }
    }

    /// (09) 완료된 업로드의 진행 행을 제거하고, 남은 행이 없으면 진행 팝업을 닫는다.
    fn finish_transfer_progress_row(
        &mut self,
        surface_id: u32,
        mirror_ws_id: u32,
        transfer_id: u64,
    ) {
        let wid = self
            .find_main_with_surface(surface_id)
            .or_else(|| self.find_main_with_workspace(mirror_ws_id));
        if let Some(wid) = wid
            && let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
        {
            if let Some(prog) = main.state.dialogs.transfer_progress.as_mut() {
                prog.rows.retain(|r| r.id != transfer_id);
                if prog.rows.is_empty() {
                    // `on_close_transfer_progress` 훅이 `dialogs.transfer_progress`
                    // 정리를 담당 — 여기서 `= None` 을 직접 하면 그 훅과 중복이다.
                    main.state.popups.close(TRANSFER_PROGRESS_POPUP_ID); // intent-exempt: popup lifecycle.
                }
            }
            main.mark_dirty();
        }
    }

    /// (09) 실패를 실패 팝업 큐에 push 하고 팝업을 연다(대상 surface 소유 창, 없으면 mirror
    /// ws 소유 창). `retry` 가 Some 이면 Retry 버튼 + 재전송 페이로드.
    fn push_transfer_error(
        &mut self,
        surface_id: u32,
        mirror_ws_id: u32,
        name: String,
        reason: String,
        retry: Option<crate::core::PendingImageUpload>,
    ) {
        let wid = self
            .find_main_with_surface(surface_id)
            .or_else(|| self.find_main_with_workspace(mirror_ws_id));
        if let Some(wid) = wid
            && let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
        {
            main.state.dialogs.transfer_error.push_back(TransferError {
                name,
                reason,
                retry,
            });
            main.state
                .popups
                .open_centered_focused(TRANSFER_ERROR_POPUP_ID);
            main.mark_dirty();
        }
    }
}

/// (09) 평균 전송률 문자열 — `sent / elapsed` 을 "12.3 MiB/s" 로. 경과 0 이면 "—".
fn format_rate(sent: u64, elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return "—".to_string();
    }
    let bps = sent as f64 / secs;
    const UNITS: &[&str] = &["B/s", "KiB/s", "MiB/s", "GiB/s"];
    let mut v = bps;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}
