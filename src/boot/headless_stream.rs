//! Headless 빌드 전용 attach 스트림 inbound 적용.
//!
//! `AppEvent::StreamReady` 한 발이 실어 오는 것은 [`PumpOutcome`] 하나지만, 그 안에는
//! 서로 독립한 요청 벡터가 14 개 들어 있다 — attach 결선 · 입력 · 구조 op · mirror 상태
//! · mesh · 캡처 업로드 · 파일 조회 · bulk 전송 · 연결 종료. gui 는 이 처리를 창마다
//! 나눠 갖지만 headless 는 engine 이 하나뿐이라 순회가 필요 없고, 대신 **한 함수에 전부
//! 모여 있었다**(332 줄, 인지 복잡도의 대부분).
//!
//! **적용 순서가 계약이다.** plugin manager lazy 초기화가 attach 결선보다 앞서야 하고,
//! 연결 종료 *정리*는 마지막이어야 한다 — 끊긴 client 가 죽기 직전 보낸 입력 프레임은
//! 그 client 의 점유가 살아 있는 동안 적용돼야 하기 때문이다. 다만 정리가 마지막이라는
//! 것과 **끊겼다는 사실을 마지막에 안다**는 것은 다르다: 그 사실은 배치 머리에서
//! `mark_clients_disconnected` 로 먼저 알린다. 안 그러면 같은 배치에 실린 제3자의
//! 재attach 가, 이 배치 끝에서 놓을 것이 확정된 holder 에게 막힌다. [`apply`] 의 호출 순서가 그 계약이며, 각 함수는 자기 벡터를
//! `std::mem::take` 로 비워 간다 — 벡터를 인자로 풀어 넘기지 않는 이유는 그중 하나가
//! 7-튜플이라 시그니처가 계약보다 커지기 때문이다.

#![cfg(not(feature = "gui"))]

use crate::adapters::production::stream_hub::{PumpOutcome, StreamClientId};
use crate::app::App;
use crate::core::CoreState;
use crate::state::AppState;

/// `AppEvent::StreamReady` 처리 — inbound 큐를 분류해 engine 에 적용한다.
pub(crate) fn handle_stream_ready(app: &mut App, state: &mut AppState, engine: &mut CoreState) {
    // 스트림 클라 inbound 를 분류해 attach 결선(단계 4): attach 요청 →
    // lock+스냅샷+출력 forward, 입력 Data → 점유 surface PTY, 끊김 →
    // lock free 환원(단계 3). 비-attach client 의 Data 는 debug echo.
    let mut outcome = app.stream_hub.pump_inbound(&app.stream_inbound_rx);
    apply(app, state, engine, &mut outcome);
}

/// 분류된 요청을 **선언된 순서 그대로** 적용한다. 순서 근거는 모듈 주석.
fn apply(app: &mut App, state: &mut AppState, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    // attach mesh mirror 는 plugin surface(markdown/image/mesh_demo)의 실제 plugin
    // 프로세스가 필요하다. 상시 초기화는 회귀 위험이 넓어(스코프 결정) attach 세션이
    // 실제로 시작되는 이 지점에서만 lazy 초기화한다. 이후엔 프로세스 수명 동안 유지
    // (tear-down 없음).
    if !outcome.attach_requests.is_empty() || !outcome.workspace_attach_requests.is_empty() {
        super::headless_plugins::ensure_plugin_manager(app, engine);
    }
    // 배치 **머리**에서 끊김을 *표시* 한다(해제는 여전히 마지막). 이 한 줄이 없으면
    // 같은 배치에 실린 재attach 가 배치 끝에서 사라질 holder 에게 `already_attached`
    // 로 거절된다 — 근거·대안은 `OccupancyRegistry::mark_clients_disconnected`.
    engine
        .attach
        .mark_clients_disconnected(&outcome.disconnected);
    apply_attach_requests(app, engine, outcome);
    apply_input_frames(app, engine, outcome);
    apply_structural_ops(app, state, engine, outcome);
    apply_mirror_state(engine, outcome);
    apply_mesh_requests(app, engine, outcome);
    apply_capture_uploads(app, engine, outcome);
    apply_file_requests(app, engine, outcome);
    apply_bulk_events(app, engine, outcome);
    apply_disconnects(engine, outcome);
}

/// attach 결선 — surface 단위(단계 4)와 workspace 단위(단계 6).
fn apply_attach_requests(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, surface_id) in std::mem::take(&mut outcome.attach_requests) {
        engine.attach_surface_for_stream(surface_id, client_id, &app.stream_hub);
    }
    for (client_id, workspace_id) in std::mem::take(&mut outcome.workspace_attach_requests) {
        engine.attach_workspace_for_stream(workspace_id, client_id, &app.stream_hub);
    }
}

/// 미러 입력 프레임을 점유 surface 의 PTY 로 보낸다.
fn apply_input_frames(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, bytes) in std::mem::take(&mut outcome.input_frames) {
        // workspace mode(단계 6)면 입력은 surface-prefixed → demux 후 지정
        // surface 로. 아니면 단계 4 의 bare 입력(점유 단일 surface).
        let routed = if engine.attach.client_holds_workspace(client_id) {
            match crate::ipc::stream::decode_mux(&bytes) {
                Some((sid, payload)) => {
                    engine.feed_attached_workspace_input(client_id, sid, payload)
                }
                None => false,
            }
        } else {
            engine.feed_attached_input(client_id, &bytes)
        };
        #[cfg(debug_assertions)]
        if !routed {
            // 단계 1 echo client(점유 surface 없음): debug 빌드 회신.
            let echo_frame =
                crate::ipc::stream::StreamFrame::new(crate::ipc::stream::StreamTag::Data, bytes);
            let _ = app.stream_hub.push(client_id, echo_frame); // best-effort echo — PushResult(Result 아님) 무시: client 끊김 시 무해.
        }
        #[cfg(not(debug_assertions))]
        let _ = routed; // release: echo 분기 없어 routed 미사용 — 값 drop(Result 아님).
    }
}

/// mirror client 가 forward 한 구조 op 실행 + 회신.
fn apply_structural_ops(
    app: &mut App,
    state: &mut AppState,
    engine: &mut CoreState,
    outcome: &mut PumpOutcome,
) {
    for (client_id, op_id, op) in std::mem::take(&mut outcome.structural_ops) {
        // mirror client 가 forward 한 구조 op — anchor 워크스페이스를 그
        // client 가 점유(holder)할 때만 실행하고 StructuralResult 로 회신,
        // 성공 시 StructuralDelta 로 역반영(3단계). 순서: result → delta →
        // 새 surface tap(client 가 매핑을 만든 뒤 스냅샷을 받게).
        let anchor = op.anchor_surface_id();
        let (ok, reason, delta) = match engine.attach.workspace_of_surface(anchor) {
            Some(ws) if engine.attach.workspace_holder(ws) == Some(client_id) => {
                match crate::core::attach_runtime::execute_forwarded_structural_op(
                    &mut app.core,
                    state,
                    engine,
                    &op,
                ) {
                    Ok(delta) => (true, None, delta),
                    Err(reason) => (false, Some(reason), None),
                }
            }
            Some(_) => (false, Some("not workspace holder".to_string()), None),
            None => (false, Some("workspace not found".to_string()), None),
        };
        let reply = crate::ipc::stream::StreamControl::StructuralResult { op_id, ok, reason };
        let frame = crate::ipc::stream::StreamFrame::new(
            crate::ipc::stream::StreamTag::Control,
            serde_json::to_vec(&reply).unwrap_or_default(),
        );
        let _ = app.stream_hub.push(client_id, frame); // best-effort 회신 — 무시.
        if let Some(fd) = delta {
            let delta_frame = crate::ipc::stream::StreamFrame::new(
                crate::ipc::stream::StreamTag::Control,
                serde_json::to_vec(&fd.delta).unwrap_or_default(),
            );
            let _ = app.stream_hub.push(client_id, delta_frame); // best-effort delta — 무시.
            for sid in fd.added_terminals {
                engine.tap_surface_for_stream(sid, client_id, &app.stream_hub);
            }
            // forward 된 ConvertSurface 가 실제 kind 를 바꿨으면 egui-mesh stale
            // frame 을 버린다(`app/event_handler.rs` 의 동일 처리와 짝).
            if let Some(sid) = fd.converted_surface
                && let Some(mgr) = app.plugin_manager.as_mut()
            {
                mgr.drop_egui_mesh_frame(sid);
            }
        }
    }
}

/// 미러가 되돌려 보내는 상태 변경 — attention 해제와 client 주도 resize.
fn apply_mirror_state(engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, remote_surface_id) in std::mem::take(&mut outcome.attention_clear_requests) {
        // 미러 사용자가 그 surface 를 확인(실-포커스 / 알림 읽음)했다는
        // 판정을 소유 인스턴스에 적용한다. holder 검증은 헬퍼가 담당하고,
        // 지워진 값은 다음 attention diff tick 이 `kind: null` push 로
        // 미러에 되돌려 확정한다(추가 push 없음). headless 서버가 주
        // 시나리오다.
        engine.apply_attached_attention_clear(client_id, remote_surface_id);
    }
    for (client_id, remote_surface_id, cols, rows) in std::mem::take(&mut outcome.resize_requests) {
        // client-driven mirror geometry(ADR-0045): mirror client 가
        // 요청한 크기로 원격 PTY 를 resize. holder 검증은 헬퍼가 담당,
        // 변화 시 기존 resize tap 이 server→client Resize echo 를 자동
        // fan-out 한다(추가 push 없음). headless 서버가 주 시나리오다.
        engine.apply_attached_workspace_resize(client_id, remote_surface_id, cols, rows);
    }
}

/// mesh mirror 3 종 — 구독/geometry · 전체 재전송 · 입력 역방향.
fn apply_mesh_requests(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, surface_id, width_px, height_px, pixels_per_point, theme, focused) in
        std::mem::take(&mut outcome.mesh_context_requests)
    {
        // mesh 구독/geometry 갱신(attach mesh mirror 소비 경로 — 상세
        // `docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-소비-경로`) —
        // 구독 요청 자체가 capability negotiation. holder 불일치/미점유
        // surface 는 명시 MeshError 로 회신한다(무시 대신 오류).
        let ok = engine.apply_attached_mesh_context(
            surface_id,
            client_id,
            width_px,
            height_px,
            pixels_per_point,
            theme,
            focused,
        );
        if !ok {
            push_mesh_error(app, client_id, surface_id);
        }
    }
    for (client_id, surface_id) in std::mem::take(&mut outcome.mesh_full_resend_requests) {
        let ok = engine.apply_attached_mesh_full_resend(surface_id, client_id);
        if !ok {
            push_mesh_error(app, client_id, surface_id);
        }
    }
    for (client_id, surface_id, input) in std::mem::take(&mut outcome.mesh_input_events) {
        // attach mesh mirror 입력 역방향 forward(상세
        // `docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-소비-경로`) —
        // holder 검증은 apply_attached_mesh_input 이 담당. 실제 plugin 구동은
        // headless_plugins::forward_mesh_frames 가 다음 tick 에 누적된
        // 이벤트를 소비한다.
        let ok = engine.apply_attached_mesh_input(surface_id, client_id, input);
        if !ok {
            push_mesh_error(app, client_id, surface_id);
        }
    }
}

/// mesh 요청이 holder 검증에 걸렸을 때의 명시 오류 회신(무시 대신 오류).
fn push_mesh_error(app: &App, client_id: StreamClientId, surface_id: u32) {
    let reply = crate::ipc::stream::StreamControl::MeshError {
        surface_id,
        reason: "not_attached".to_string(),
    };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = app.stream_hub.push(client_id, frame); // best-effort 오류 회신 — 무시.
}

/// (03) screenshot→remote-clipboard 업로드 청크/커밋.
fn apply_capture_uploads(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, msg) in std::mem::take(&mut outcome.capture_uploads) {
        // (03) screenshot→remote-clipboard: mirror client 가 이 headless
        // 인스턴스로 화면 캡처를 업로드 — headless 는 단일 engine 이라
        // gui 의 holder 순회가 필요 없다. holder 검증은 finalize 내부.
        use crate::adapters::production::stream_hub::CaptureUploadMsg;
        match msg {
            CaptureUploadMsg::CaptureChunk {
                upload_id,
                data_b64,
                ..
            } => {
                use base64::Engine as _;
                match base64::engine::general_purpose::STANDARD.decode(&data_b64) {
                    Ok(bytes) if engine.attach.client_holds_workspace(client_id) => {
                        engine.capture_uploads.append(
                            client_id,
                            upload_id,
                            &bytes,
                            std::time::Instant::now(),
                        );
                    }
                    Ok(_) => tracing::warn!(
                        "capture upload: client {client_id} does not hold a workspace — dropping chunk"
                    ),
                    Err(_) => tracing::warn!(
                        "capture upload: invalid base64 chunk (client {client_id}, upload {upload_id})"
                    ),
                }
            }
            CaptureUploadMsg::CaptureCommit {
                upload_id,
                file_name,
            } => {
                crate::core::attach_runtime::finalize_capture_upload(
                    engine,
                    &app.core,
                    &app.stream_hub,
                    client_id,
                    upload_id,
                    &file_name,
                );
            }
        }
    }
}

/// 미러가 이 인스턴스에 묻는 파일계 조회 — (04) file picker 와 git-viewer.
fn apply_file_requests(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, msg) in std::mem::take(&mut outcome.list_dir_requests) {
        // (04) file picker: mirror client 가 이 headless 인스턴스로
        // 디렉토리 목록을 요청 — headless 는 단일 engine 이라 gui 의
        // holder 순회가 필요 없다. holder 검증은 핸들러 내부.
        use crate::adapters::production::stream_hub::ListDirRequestMsg;
        let ListDirRequestMsg::ListDirRequest { request_id, dir } = msg;
        crate::core::attach_runtime::handle_list_dir_request(
            engine,
            &app.stream_hub,
            client_id,
            request_id,
            &dir,
        );
    }
    for (client_id, msg) in std::mem::take(&mut outcome.git_query_requests) {
        // git-viewer(`docs/adr/0056-git-viewer-remote-attach-git-query-channel.md`):
        // mirror client 가 이 headless 인스턴스로 git status/log/worktrees
        // 또는 diff 조회를 요청 — list_dir 와 동일하게 headless 는 단일
        // engine 이라 holder 순회 불요.
        use crate::adapters::production::stream_hub::GitQueryRequestMsg;
        let GitQueryRequestMsg::GitQueryRequest {
            request_id,
            surface_id,
            kind,
            worktree_path,
            diff_path,
        } = msg;
        crate::core::attach_runtime::handle_git_query_request(
            engine,
            &app.stream_hub,
            client_id,
            request_id,
            surface_id,
            kind,
            worktree_path,
            diff_path,
        );
    }
}

/// (06) native bulk 파일 전송 — begin/chunk/commit 을 도착 순서 그대로.
fn apply_bulk_events(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, event) in std::mem::take(&mut outcome.bulk_events) {
        // (06) native bulk 파일 전송: begin/chunk/commit 을 **도착 순서
        // 그대로** 처리한다(단일 벡터라 chunk 가 begin 을 앞지르지 않음 —
        // 분리 벡터 시절의 전량 폐기 + 빈 파일 성공 오보 결함 방지). 결속
        // workspace 는 연결-단위 bulk 태깅에서 조회(begin 이 ws 를 싣지 않음).
        use crate::adapters::production::stream_hub::BulkEvent;
        let Some(ws) = app.stream_hub.bulk_workspace(client_id) else {
            tracing::warn!("bulk transfer: event from non-bulk client {client_id} — ignoring");
            continue;
        };
        match event {
            BulkEvent::Begin {
                transfer_id,
                filename,
                total_size,
            } => {
                // (07) 용량 사전판정 — 초과면 등록하지 않고 capacity-exceeded
                // 회신(청크 0바이트 수신). 통과 시 begin 등록.
                crate::core::attach_runtime::begin_bulk_transfer(
                    engine,
                    &app.stream_hub,
                    client_id,
                    transfer_id,
                    filename,
                    total_size,
                );
            }
            BulkEvent::Chunk {
                transfer_id,
                seq,
                bytes,
            } => {
                if !engine
                    .bulk_transfers
                    .append(client_id, transfer_id, seq, &bytes)
                {
                    tracing::warn!(
                        "bulk transfer: chunk for unknown transfer (client {client_id}, transfer {transfer_id}) — no begin? dropping"
                    );
                }
            }
            BulkEvent::Commit { transfer_id } => {
                // (07) 저장 dir 은 설정값(빈 값이면 기본 폴더) — begin 용량
                // 판정과 같은 폴더 기준.
                let dir = crate::core::attach_runtime::resolve_bulk_transfer_dir(&engine.settings);
                crate::core::attach_runtime::finalize_bulk_transfer(
                    engine,
                    &app.stream_hub,
                    client_id,
                    transfer_id,
                    ws,
                    dir,
                );
            }
        }
    }
}

/// 연결 종료 정리 — 점유 해제 + 커밋 안 된 partial 전량 폐기.
fn apply_disconnects(engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for client_id in std::mem::take(&mut outcome.disconnected) {
        engine.attach.release_all_for_client(client_id);
        // bulk 연결 종료 시 커밋 안 된 대용량 partial 청소.
        engine.bulk_transfers.clear_client(client_id);
        // 캡처 업로드 연결 종료 시 커밋 안 된 partial 청소.
        engine.capture_uploads.clear_client(client_id);
        // mesh 구독 정리 — 불필요한 plugin CPU 낭비 방지(상세
        // `docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-소비-경로`).
        engine.mesh_mirror.remove_for_client(client_id);
    }
}
