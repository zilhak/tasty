//! attach 스트림 요청을 헤드리스의 단일 engine에 적용한다.
//! attach 전에 플러그인을 준비하고 끊긴 client를 표시해 같은 배치의 재attach를 허용한다.
//! 입력을 먼저 적용할 수 있도록 점유 해제·연결별 정리는 마지막에 수행한다.

#![cfg(not(feature = "gui"))]

use crate::app::App;
use crate::core::CoreState;
use crate::state::AppState;
use tasty_ipc::stream_hub::{PumpOutcome, StreamClientId};

pub(crate) fn handle_stream_ready(app: &mut App, state: &mut AppState, engine: &mut CoreState) {
    let mut outcome = app.stream_hub.pump_inbound(&app.stream_inbound_rx);
    apply(app, state, engine, &mut outcome);
}

fn apply(app: &mut App, state: &mut AppState, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    if !outcome.attach_requests.is_empty() || !outcome.workspace_attach_requests.is_empty() {
        super::headless_plugins::ensure_plugin_manager(app, engine);
    }
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
    // 직접 구조 op 외의 변경도 점유 client에 전달한다.
    engine.push_structure_changes();
}

fn apply_attach_requests(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, surface_id) in std::mem::take(&mut outcome.attach_requests) {
        engine.attach_surface_for_stream(surface_id, client_id, &app.stream_hub);
    }
    for (client_id, workspace_id) in std::mem::take(&mut outcome.workspace_attach_requests) {
        engine.attach_workspace_for_stream(workspace_id, client_id, &app.stream_hub);
    }
}

fn apply_input_frames(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, bytes) in std::mem::take(&mut outcome.input_frames) {
        // workspace 입력은 surface ID로 나누고 단일 surface 입력은 그대로 전달한다.
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
            let echo_frame =
                crate::ipc::stream::StreamFrame::new(crate::ipc::stream::StreamTag::Data, bytes);
            let _ = app.stream_hub.push(client_id, echo_frame); // 연결 종료·손실은 허브가 처리하며 echo는 재시도하지 않는다.
        }
        #[cfg(not(debug_assertions))]
        let _ = routed; // release에는 echo 분기가 없어 라우팅 여부를 사용하지 않는다.
    }
}

fn apply_structural_ops(
    app: &mut App,
    state: &mut AppState,
    engine: &mut CoreState,
    outcome: &mut PumpOutcome,
) {
    for (client_id, op_id, op, origin) in std::mem::take(&mut outcome.structural_ops) {
        // 점유자를 확인한 뒤 result → delta → 새 surface tap 순으로 보낸다.
        // client가 ID 매핑을 만든 뒤 스냅샷을 받아야 한다.
        let anchor = op.anchor_surface_id();
        let (ok, reason, delta) = match engine.attach.workspace_of_surface(anchor) {
            Some(ws) if engine.attach.workspace_holder(ws) == Some(client_id) => {
                match crate::core::attach_runtime::execute_forwarded_structural_op(
                    &mut app.core,
                    state,
                    engine,
                    &op,
                    origin,
                ) {
                    Ok(delta) => (true, None, delta),
                    Err(reason) => (false, Some(reason), None),
                }
            }
            Some(_) => (false, Some("not workspace holder".to_string()), None),
            None => (
                false,
                Some(
                    crate::core::attach_structure_sync::unresolved_forward_reason(
                        [&*engine],
                        client_id,
                        &op,
                    ),
                ),
                None,
            ),
        };
        let reply = crate::ipc::stream::StreamControl::StructuralResult { op_id, ok, reason };
        let frame = crate::ipc::stream::StreamFrame::new(
            crate::ipc::stream::StreamTag::Control,
            serde_json::to_vec(&reply).unwrap_or_default(),
        );
        let _ = app.stream_hub.push(client_id, frame); // 연결 종료·손실은 허브가 처리하며 응답은 재시도하지 않는다.
        if let Some(fd) = delta {
            let delta_frame = crate::ipc::stream::StreamFrame::new(
                crate::ipc::stream::StreamTag::Control,
                serde_json::to_vec(&fd.delta).unwrap_or_default(),
            );
            let _ = app.stream_hub.push(client_id, delta_frame); // 손실 복구는 스트림 경로에 맡기며 여기서 delta를 재전송하지 않는다.
            for sid in fd.added_terminals {
                engine.tap_surface_for_stream(sid, client_id, &app.stream_hub);
            }
            // kind 변환 뒤 이전 mesh frame이 남아 표시되지 않게 버린다.
            if let Some(sid) = fd.converted_surface
                && let Some(mgr) = app.plugin_manager.as_mut()
            {
                mgr.drop_egui_mesh_frame(sid);
            }
        }
    }
}

fn apply_mirror_state(engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, remote_surface_id) in std::mem::take(&mut outcome.attention_clear_requests) {
        // attention 해제는 다음 상태 diff에서 mirror로 전달한다.
        engine.apply_attached_attention_clear(client_id, remote_surface_id);
    }
    for (client_id, remote_surface_id, cols, rows) in std::mem::take(&mut outcome.resize_requests) {
        // PTY resize tap이 변경을 전송하므로 별도 echo를 추가하지 않는다.
        engine.apply_attached_workspace_resize(client_id, remote_surface_id, cols, rows);
    }
}

fn apply_mesh_requests(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, surface_id, width_px, height_px, pixels_per_point, theme, focused) in
        std::mem::take(&mut outcome.mesh_context_requests)
    {
        // 구독의 점유 검증에 실패하면 MeshError로 회신한다.
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
        // 입력의 점유 검증은 apply_attached_mesh_input이, 누적 입력 소비는 다음 mesh 전달이 맡는다.
        let ok = engine.apply_attached_mesh_input(surface_id, client_id, input);
        if !ok {
            push_mesh_error(app, client_id, surface_id);
        }
    }
}

fn push_mesh_error(app: &App, client_id: StreamClientId, surface_id: u32) {
    let reply = crate::ipc::stream::StreamControl::MeshError {
        surface_id,
        reason: "not_attached".to_string(),
    };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = app.stream_hub.push(client_id, frame); // 연결 종료·손실은 허브가 처리하며 오류 응답은 재시도하지 않는다.
}

fn apply_capture_uploads(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, msg) in std::mem::take(&mut outcome.capture_uploads) {
        use tasty_ipc::stream_hub::CaptureUploadMsg;
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

fn apply_file_requests(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, msg) in std::mem::take(&mut outcome.list_dir_requests) {
        use tasty_ipc::stream_hub::ListDirRequestMsg;
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
        use tasty_ipc::stream_hub::GitQueryRequestMsg;
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
    for (client_id, msg) in std::mem::take(&mut outcome.markdown_content_requests) {
        use tasty_ipc::stream_hub::MarkdownContentRequestMsg;
        let MarkdownContentRequestMsg::MarkdownContentRequest {
            request_id,
            surface_id,
        } = msg;
        crate::core::attach_runtime::handle_markdown_content_request(
            engine,
            &app.stream_hub,
            client_id,
            request_id,
            surface_id,
        );
    }
}

/// begin·chunk·commit의 도착 순서를 유지한다. workspace는 연결의 bulk 태그에서 찾는다.
fn apply_bulk_events(app: &mut App, engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for (client_id, event) in std::mem::take(&mut outcome.bulk_events) {
        use tasty_ipc::stream_hub::BulkEvent;
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
                // 용량 사전판정과 같은 저장 폴더를 사용한다.
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

fn apply_disconnects(engine: &mut CoreState, outcome: &mut PumpOutcome) {
    for client_id in std::mem::take(&mut outcome.disconnected) {
        engine.attach.release_all_for_client(client_id);
        engine.bulk_transfers.clear_client(client_id);
        engine.capture_uploads.clear_client(client_id);
        engine.mesh_mirror.remove_for_client(client_id);
    }
}
