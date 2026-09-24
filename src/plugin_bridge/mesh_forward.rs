//! GUI와 헤드리스에서 원격 attach에 mesh 프레임을 전달한다.
//!
//! 창이 없는 헤드리스·parked engine은 forward_mesh_frames_for_engine으로
//! 플러그인 렌더도 요청한다. 창이 있는 engine은 로컬 화면 크기로 이미 렌더하므로
//! relay_mesh_frame_if_new로 만들어진 프레임만 전달한다.
//! attach 클라이언트의 크기로 다시 렌더하면 로컬 화면의 해상도까지 바뀔 수 있다.
//! 동작: docs/dev-guide/attach-behavior.md.

use crate::core::CoreState;
use crate::core::attach::AttachClientId;
use crate::ipc::stream::{StreamFrame, StreamTag};
use crate::plugin::PluginManager;
use tasty_ipc::stream_hub::{PushResult, StreamHub};

/// 창이 없는 engine의 mesh 구독을 처리한다.
/// 구독이 dirty이면 context를 보내고, 전송하지 않은 프레임이 있으면 client에 전달한다.
/// 두 조건은 독립적으로 확인한다. 창이 있는 engine에는 사용하지 않는다.
pub(crate) fn forward_mesh_frames_for_engine(
    engine: &mut CoreState,
    mgr: &PluginManager,
    stream_hub: &StreamHub,
) {
    for sid in engine.mesh_mirror.active_surface_ids() {
        if !engine.attach.is_hard_occupied(sid) {
            engine.mesh_mirror.remove(sid);
            continue;
        }
        let Some(ms) = engine.find_egui_mesh_surface(sid) else {
            engine.mesh_mirror.remove(sid);
            continue;
        };
        let plugin_id = ms.plugin_id.clone();
        let kind = ms.kind_static;
        let file = ms.file.clone();
        let display_name = ms.display_name.clone();

        let Some(ctx) = engine.mesh_mirror.get(sid) else {
            continue;
        };
        let client_id = ctx.client_id;
        let width_px = ctx.width_px;
        let height_px = ctx.height_px;
        let pixels_per_point = ctx.pixels_per_point;
        let theme = ctx.theme.clone();
        let focused = ctx.focused;
        let modifiers = ctx.last_modifiers;

        let dirty = engine.mesh_mirror.take_dirty(sid);
        let need_full = engine.mesh_mirror.take_need_full_textures(sid);
        // 입력 추가도 dirty를 설정하므로 크기나 테마가 그대로여도 아래에서 전달한다.
        let events = engine.mesh_mirror.take_pending_events(sid);

        if dirty {
            let has_frame = mgr.egui_mesh_frame(sid).is_some();
            if !has_frame {
                mgr.send_egui_mesh_surface_create(
                    &plugin_id,
                    sid,
                    kind,
                    file.as_deref(),
                    &display_name,
                );
            }
            let params = tasty_plugin_protocol::SurfaceSetContextParams {
                surface_id: sid,
                width_px,
                height_px,
                pixels_per_point,
                raw_input: tasty_plugin_protocol::RawInputWire {
                    time: None,
                    focused,
                    modifiers,
                    events,
                },
                theme,
                need_full_textures: need_full,
            };
            mgr.send_surface_set_context(&plugin_id, &params);
        }

        relay_mesh_frame_if_new(engine, mgr, stream_hub, sid, client_id);
    }
}

/// 이미 만들어진 새 프레임을 attach client에 전달한다. context는 보내지 않는다.
pub(crate) fn relay_mesh_frame_if_new(
    engine: &mut CoreState,
    mgr: &PluginManager,
    stream_hub: &StreamHub,
    sid: u32,
    client_id: AttachClientId,
) {
    let Some(frame) = mgr.egui_mesh_frame(sid) else {
        return;
    };
    if !engine
        .mesh_mirror
        .should_forward_generation(sid, frame.generation)
    {
        return;
    }
    let Some(mem) = mgr.plugin_buffer(&frame.plugin_id, frame.buffer_id) else {
        return;
    };
    // SAFETY: mem이 슬라이스를 읽는 동안 매핑을 유지한다.
    // payload를 읽는 동안 다른 프로세스의 쓰기도 배제해야 하지만,
    // 아래 generation 비교만으로는 배제되지 않으며 이 경로에 별도 배제 절차는 없다.
    let raw = unsafe { mem.as_slice() };
    if raw.len() < tasty_shm::footer::SIZE {
        return;
    }
    // SAFETY: 길이는 위에서 확인했고 매핑 시작은 페이지 정렬돼 있다.
    // 첫 8바이트는 양쪽 프로세스가 atomic footer로 사용한다.
    let gen_now = unsafe { tasty_shm::footer::load(raw, std::sync::atomic::Ordering::Acquire) };
    if gen_now != frame.generation {
        // 메타데이터와 버퍼 세대가 다르면 다음 호출로 미룬다. 일치해도 이후 쓰기를 배제하지 않는다.
        return;
    }
    let user = tasty_shm::footer::user_slice(raw);
    let byte_len = frame.byte_len as usize;
    let bytes = if byte_len > 0 && byte_len <= user.len() {
        &user[..byte_len]
    } else {
        // byte_len이 없거나 범위를 벗어나면 전체 payload 용량을 사용한다.
        user
    };

    let Some(frame_id) = engine.mesh_mirror.mark_forwarded(sid, frame.generation) else {
        return;
    };
    for payload in tasty_ipc::mesh_stream::split_mesh_frame(
        sid,
        frame_id,
        frame.generation,
        frame.frame_seq,
        frame.full_textures,
        bytes,
    ) {
        let result = stream_hub.push(client_id, StreamFrame::new(StreamTag::MeshData, payload));
        if matches!(result, PushResult::Unknown | PushResult::Disconnected) {
            // 연결이 사라졌거나 끊겼으면 이 프레임의 나머지 chunk 전송을 중단한다.
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::egui_mesh_surface::EguiMeshSurface;
    use std::sync::Arc;
    use tasty_terminal::waker_factory::NoopWakerFactory;

    /// 터미널 대신 mesh surface를 만들고 client가 hard 점유·구독한 engine을 준비한다.
    fn make_parked_engine(client_id: AttachClientId) -> (CoreState, u32) {
        let waker: tasty_terminal::Waker = Arc::new(|| {});
        let mut engine = CoreState::new(80, 24, waker).expect("core state");

        let pane_id = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        let surface_id = engine.workspaces[0]
            .pane_layout()
            .find_pane(pane_id)
            .and_then(|pane| pane.tabs.first())
            .and_then(|tab| tab.layout_if_initialized())
            .and_then(|layout| layout.first_surface_id())
            .expect("seed terminal surface");

        let pane = engine.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .expect("pane");
        let tab = pane.tabs.first_mut().expect("tab");
        tab.layout_mut().replace_surface(
            surface_id,
            Box::new(EguiMeshSurface::new(
                surface_id,
                "mesh_demo",
                "com.tasty.mesh-demo".to_string(),
                "Demo".to_string(),
                None,
            )),
        );

        engine
            .attach
            .acquire(surface_id, client_id)
            .expect("hard-occupy for mesh mirror subscription");
        engine
            .mesh_mirror
            .upsert(surface_id, client_id, 800, 600, 2.0, None, true);

        (engine, surface_id)
    }

    // 여러 parked engine의 구독이 각각 처리되는지 검사한다.
    #[test]
    fn multiple_parked_engines_are_each_serviced_independently() {
        let stream_hub = StreamHub::new();

        let mut parked: Vec<(CoreState, u32)> =
            vec![make_parked_engine(101), make_parked_engine(202)];
        // 플러그인 프로세스는 실행하지 않으며 구독 상태가 처리되는지만 검사한다.
        let mgr = PluginManager::with_registries(
            Arc::new(NoopWakerFactory),
            parked[0].0.file_format.clone(),
            parked[0].0.file_handler.clone(),
        );

        for (engine, _sid) in parked.iter_mut() {
            forward_mesh_frames_for_engine(engine, &mgr, &stream_hub);
        }

        for (engine, sid) in parked.iter_mut() {
            assert!(
                !engine.mesh_mirror.take_dirty(*sid),
                "parked engine's mesh mirror subscription should have been driven"
            );
            assert!(
                !engine.mesh_mirror.take_need_full_textures(*sid),
                "parked engine's need_full_textures should have been consumed"
            );
            assert!(engine.attach.is_hard_occupied(*sid));
        }
    }
}
