//! attach 런타임 결선 (attach/detach 단계 4).
//!
//! 단계 3 의 [`AttachRegistry`](crate::core::attach) 는 순수 lock 테이블이고,
//! 단계 2 의 mirror(`new_detached`/`feed_bytes`/output tap/input sink)는 터미널
//! 메커니즘이다. 이 모듈은 둘을 **실제 바이트 파이프로 결선**한다:
//!
//! - [`CoreState::attach_surface_for_stream`]: stream client 의 attach 요청을
//!   처리 — lock 획득 → 초기 화면 bulk 스냅샷 push → 출력 tap 을 forwarder 스레드로
//!   client 에 흘림(서버 PTY 출력 → tap → StreamHub → client mirror).
//! - [`CoreState::feed_attached_input`]: client 입력 Data 프레임을 점유 surface 의
//!   PTY 로 전달(holder 검증 후, 서버 로컬 입력 차단 우회).
//!
//! 메인루프(gui `event_handler` / headless `boot`)의 `StreamReady` arm 이
//! `pump_inbound` 가 분류한 [`PumpOutcome`](crate::adapters::production::stream_hub::PumpOutcome)
//! 를 받아 이 메서드들을 호출한다.
//!
//! 범위: surface 단위(터미널 1개). workspace 단위는 단계 6.

use std::thread;

use crate::adapters::production::stream_hub::{PushResult, StreamHub};
use crate::core::attach::{AttachClientId, AttachError};
use crate::core::CoreState;
use crate::ipc::stream::{StreamFrame, StreamTag};
use crate::model::SurfaceId;

impl CoreState {
    /// stream client 의 attach 요청 처리(`stream.open` 의 `target`). 성공 시 그 client
    /// 가 surface 를 배타 점유하고, 서버는 현재 화면을 1 회 bulk 스냅샷으로 push 한 뒤
    /// 이후 PTY 출력을 forwarder 스레드로 계속 흘린다. 거부/실패 시 `attach_error`
    /// Control + Detach 로 연결을 닫는다.
    ///
    /// `hub` 는 메인루프가 보유한 StreamHub(= client sink 등록처). forwarder 스레드는
    /// 그 clone 을 들고 client 끊김(push Unknown/Disconnected) 시 자동 종료한다.
    pub fn attach_surface_for_stream(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        // 대상 검증: 실재(또는 deferred) 터미널 surface 만 점유 대상.
        if !self.terminals.contains(surface_id) && !self.is_surface_deferred(surface_id) {
            reject_attach(hub, client_id, "not_found", None);
            return;
        }

        // 배타 lock 획득(동시 attach 거부).
        match self.attach.acquire(surface_id, client_id) {
            Ok(_) => {}
            Err(AttachError::AlreadyAttached { holder }) => {
                reject_attach(hub, client_id, "already_attached", Some(holder));
                return;
            }
            Err(_) => {
                reject_attach(hub, client_id, "lock_error", None);
                return;
            }
        }

        // deferred 면 여기서 PTY spawn.
        self.ensure_surface_initialized(surface_id);

        let Some(terminal) = self.terminals.get_mut(surface_id) else {
            // 점유는 됐으나 터미널이 없다(spawn 실패) → lock 환원 + 에러.
            let _ = self.attach.release(surface_id, client_id);
            reject_attach(hub, client_id, "spawn_failed", None);
            return;
        };

        let cols = terminal.cols();
        let rows = terminal.rows();
        // 현재 화면 bulk 스냅샷 → tap 등록(메인루프 단일소유라 그 사이 ingest 없음 →
        // 누락/중복 없음). 이후 tap 바이트가 delta.
        let snapshot = terminal.snapshot_as_vt();
        let tap_rx = terminal.add_output_tap();

        // attach 성공 통지(client 가 cols/rows 로 mirror 생성) + 초기 스냅샷.
        let attached = serde_json::json!({
            "event": "attached",
            "surface_id": surface_id,
            "cols": cols,
            "rows": rows,
        });
        let _ = hub.push(
            client_id,
            StreamFrame::new(
                StreamTag::Control,
                serde_json::to_vec(&attached).unwrap_or_default(),
            ),
        );
        let _ = hub.push(client_id, StreamFrame::new(StreamTag::Data, snapshot));

        // forwarder: 서버 PTY 출력 tap → client. client 끊김 시 자동 종료(다음 출력
        // 때 terminal 의 tap 도 prune 됨, design 단계 4 §8-R4).
        let hub2 = hub.clone();
        thread::spawn(move || {
            for chunk in tap_rx {
                match hub2.push(client_id, StreamFrame::new(StreamTag::Data, chunk)) {
                    PushResult::Unknown | PushResult::Disconnected => break,
                    _ => {}
                }
            }
        });

        tracing::debug!("attach: surface {surface_id} -> client {client_id}");
    }

    /// client 입력 Data 프레임을 그 client 가 점유한 surface 의 PTY 로 전달한다.
    /// 서버 로컬 입력 차단(`apply_send_to_surface` 의 is_attached 거부)을 우회하는
    /// 유일한 정규 경로 — holder 검증을 거치므로 점유자만 입력할 수 있다.
    /// 반환: 라우팅 성공 여부(false = 점유 surface 없음 = 비-attach client).
    pub fn feed_attached_input(&mut self, client_id: AttachClientId, bytes: &[u8]) -> bool {
        let Some(surface_id) = self.attach.surface_held_by(client_id) else {
            return false;
        };
        if let Some(terminal) = self.terminals.get_mut(surface_id) {
            terminal.send_bytes(bytes);
            true
        } else {
            false
        }
    }
}

/// attach 거부/실패 통지: `attach_error` Control + Detach 로 연결 종료 유도.
/// 어떤 engine 도 대상 surface 를 소유하지 않을 때 메인루프(gui)도 호출한다.
pub(crate) fn reject_attach(
    hub: &StreamHub,
    client_id: AttachClientId,
    reason: &str,
    holder: Option<AttachClientId>,
) {
    let msg = serde_json::json!({
        "event": "attach_error",
        "reason": reason,
        "holder": holder,
    });
    let _ = hub.push(
        client_id,
        StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&msg).unwrap_or_default(),
        ),
    );
    let _ = hub.push(client_id, StreamFrame::new(StreamTag::Detach, Vec::new()));
}
