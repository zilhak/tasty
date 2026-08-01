//! `CoreState::mesh_mirror`(구독 상태, `src/core/mesh_mirror.rs`) 구동 + relay —
//! GUI/headless 공용(`docs/dev-guide/attach-behavior.md` "frame 소비·forward" 절).
//!
//! `mesh_mirror.rs` 자신은 `PluginManager` 접근권이 없어(설계상 registry 전용) 실제
//! plugin 구동은 이 모듈이 맡는다. 세 호출부가 있다:
//!
//! - **headless**(`src/boot/headless_plugins.rs::pump_plugins`) — 단일 engine, 로컬
//!   렌더가 아예 없으므로 [`forward_mesh_frames_for_engine`]이 dirty 를 직접 읽어
//!   구동까지 전담한다.
//! - **parked**(GUI, `App::about_to_wait` — macOS 최소화로 window 가 파괴되고
//!   `CoreState` 만 `App::parked_states` 에 남은 engine) — 살아있는 window 가 없다는
//!   점에서 headless 와 처지가 같으므로 동일하게 [`forward_mesh_frames_for_engine`]을
//!   그대로 재사용한다.
//! - **GUI 살아있는 window**(`view/main/egui_mesh.rs::forward_mesh_to_attach_subscribers`)
//!   — `forward_egui_mesh_context` 가 이미 매 프레임 그 window 의 실제 로컬 렌더
//!   해상도로 권위있게 `set_context` 를 구동하므로, [`forward_mesh_frames_for_engine`]을
//!   쓰면 안 된다 — mesh_mirror ctx 의 (attach client 가 요청한, 로컬과 다를 수 있는)
//!   width_px/height_px 로 다시 구동하면 로컬 화면이 attach client 해상도로 튈 수
//!   있다. 이 경로는 자체 판단(bootstrap-if-never-rendered / need_full 은 로컬
//!   pending_full 메커니즘에 위임)만 하고, 이미 만들어진 frame 을 client 로 흘리는
//!   꼬리 로직만 [`relay_mesh_frame_if_new`]로 공유한다.

use crate::adapters::production::stream_hub::{PushResult, StreamHub};
use crate::core::CoreState;
use crate::core::attach::AttachClientId;
use crate::ipc::stream::{StreamFrame, StreamTag};
use crate::plugin::PluginManager;

/// `CoreState::mesh_mirror`(구독 상태)를 읽어 plugin 을 구동하고, 새 frame 을 attach
/// client 에 chunk forward 한다(`docs/dev-guide/attach-behavior.md` "frame 소비·forward
/// (headless-as-server / gui parked engine)" 절). headless 는 `pump_plugins` 호출 tick 마다,
/// parked engine 은 GUI 의 App-level tick(`about_to_wait`, `mgr.pump()` 호출 지점)
/// 마다 실행돼 `PaintFrame` 도착 즉시(또는 1Hz busy-poll 안전망 tick 에) 반응한다 —
/// 별도 wake 채널 불필요(headless_plugins 모듈 문서 §pump 트리거 참조).
///
/// 구독당 두 가지 독립 동작:
/// 1. **구독 상태가 dirty**(신규 구독/geometry·theme·focus 변경) — plugin 에
///    `surface.set_context` 재전송(첫 호출이면 `surface.create` bootstrap 선행).
/// 2. **아직 이 client 에 안 보낸 새 generation 의 frame 존재** — `SharedBuffer`에서
///    바이트를 읽어 chunk 로 쪼개 client 에 push([`relay_mesh_frame_if_new`]).
/// 두 동작은 서로 독립이다 — geometry 변경 없이도 plugin 이 새 frame 을 밀 수 있고
/// (markdown 내부 애니메이션 등), 반대로 이번 tick 에 새 frame 이 없어도 geometry 변경은
/// 즉시 반영해야 한다.
///
/// **살아있는 GUI window 의 engine 에는 쓰지 않는다** — 모듈 문서 참조.
pub(crate) fn forward_mesh_frames_for_engine(
    engine: &mut CoreState,
    mgr: &PluginManager,
    stream_hub: &StreamHub,
) {
    for sid in engine.mesh_mirror.active_surface_ids() {
        // 방어적 정리: 정상 경로는 disconnected 처리(boot.rs)가 동기적으로
        // `mesh_mirror.remove_for_client` 를 부르므로 거의 항상 일치하지만, surface
        // 자체가 닫힌 경우(구조 변경 등)는 여기서만 감지된다.
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
        // attach client → plugin 입력 forward(`docs/dev-guide/attach-behavior.md`
        // "MeshInput 누적" 절) — `MeshInput`으로 누적된
        // 이벤트를 이번 set_context 에 실어 보낸다. dirty(위 take_dirty)는
        // push_input 이 이미 세워두므로, 입력만 있고 geometry/theme/focus 변경이
        // 없어도 이 블록에 진입한다.
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

/// mesh mirror 구독 1개에 대해, plugin 이 이미 만들어 둔 최신 `EguiMeshFrame`(있다면)을
/// attach client 로 relay 한다 — 새 `set_context` 는 보내지 않는 순수 byte relay다
/// (`docs/dev-guide/attach-behavior.md` "frame 소비·forward" 절). headless/parked
/// ([`forward_mesh_frames_for_engine`])와 GUI 살아있는
/// window(`view/main/egui_mesh.rs::forward_mesh_to_attach_subscribers`) 양쪽의 공용
/// 꼬리 로직 — 두 호출부 모두 "이 tick 에 새로 구동할지"는 각자 판단하고, "구동
/// 후(또는 무관하게) 이미 있는 frame 을 client 에 흘리는" 이 부분만 공유한다.
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
    // SAFETY: `buffer_id`는 이 plugin 이 `host.shared_buffer.create`로 만든 뒤
    // `PaintFrame` 이벤트로 알려온 값이라 `mem`은 유효한 매핑이다. footer
    // 프로토콜은 plugin(writer)/host(reader) 양쪽이 합의한 8B 헤더 + user data
    // 레이아웃이며(`tasty_shm::footer` 문서), 이 host 프로세스가 이 버퍼의 유일한
    // reader 다 — `gfx/gpu/egui_mesh_prepare.rs::decode_mesh_into_target`의 동일
    // 패턴과 동형(그쪽은 GPU 디코드까지 하지만 여기는 raw 바이트 forward 만 한다).
    let raw = unsafe { mem.as_slice() };
    if raw.len() < tasty_shm::footer::SIZE {
        return;
    }
    // SAFETY: 위에서 `raw.len() >= footer::SIZE`를 검증했고, mmap 매핑의 시작
    // 주소는 항상 페이지 정렬(≥8B)이라 `AtomicU64` 재해석이 안전하다(`footer_atomic`
    // 안전 조건 문서 참조).
    let gen_now = unsafe { tasty_shm::footer::load(raw, std::sync::atomic::Ordering::Acquire) };
    if gen_now != frame.generation {
        // writer 가 다음 frame 을 쓰는 중(tear) — 다음 tick 에 재시도(GPU 경로와 동형).
        return;
    }
    let user = tasty_shm::footer::user_slice(raw);
    let byte_len = frame.byte_len as usize;
    let bytes = if byte_len > 0 && byte_len <= user.len() {
        &user[..byte_len]
    } else {
        // 구버전 plugin(byte_len=0) 또는 길이 불일치 — capacity 전체 fallback.
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
            // client 가 끊겼다 — 다음 StreamReady tick 의 disconnected 처리가
            // `remove_for_client`로 구독을 정리한다. 남은 chunk 전송은 낭비이므로
            // 이 surface 는 여기서 중단.
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_bridge::egui_mesh_surface::EguiMeshSurface;
    use std::sync::Arc;
    use tasty_terminal::waker_factory::NoopWakerFactory;

    /// headless 와 동형의 단일 mesh surface 를 가진 "parked engine" 하나를 만든다 —
    /// 기본 workspace/pane/tab 의 시드 터미널 surface 를 `EguiMeshSurface` 로
    /// 교체하고, 그 surface 를 `client_id` 가 hard-occupy + mesh mirror 구독 중인
    /// 상태로 세팅한다(`handle_minimize` macOS 분기가 parked 로 옮기기 직전의 실제
    /// 상태와 동형 — 구독/hard-occupy 자체는 owning-engine 패턴으로 이미 parked
    /// engine 에도 정상 반영된다, `apply_mesh_context_on_owning_engine` 참조).
    fn make_parked_engine(client_id: AttachClientId) -> (CoreState, u32) {
        let waker: crate::terminal::Waker = Arc::new(|| {});
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
        // 신규 구독 — `upsert` 는 dirty=true/need_full_textures=true 로 seed 한다
        // (mesh_mirror.rs::new_subscribe_is_dirty_and_needs_full_textures 와 동일 전제).
        engine
            .mesh_mirror
            .upsert(surface_id, client_id, 800, 600, 2.0, None, true);

        (engine, surface_id)
    }

    /// 핵심 회귀 시나리오(`docs/dev-guide/attach-behavior.md` "frame 소비·forward" 절):
    /// macOS 에서 window 여러 개가 동시에 minimize 되어 `App::parked_states` 에
    /// engine 이 2개 이상 쌓인 경우.
    /// `window_lifecycle.rs` 의 복원이 `remove(0)` 으로 1개씩만 꺼내므로, 이 구동
    /// 함수는 owning-engine 순회 패턴처럼 **첫 매치에서 멈추면 안 되고** parked
    /// 전부를 독립적으로 서비스해야 한다.
    #[test]
    fn multiple_parked_engines_are_each_serviced_independently() {
        let stream_hub = StreamHub::new();

        let mut parked: Vec<(CoreState, u32)> =
            vec![make_parked_engine(101), make_parked_engine(202)];
        // 등록된 plugin 은 없다(dirty-driven set_context/create 호출이 해당
        // plugin_id 를 찾지 못해 조용히 no-op 되는 경로만 검증) — `with_registries`
        // 는 실제 plugin process 없이도 안전하게 만들 수 있는 유일한 공개 생성자다
        // (`PluginManager::new` 는 tasty-host-plugin crate 내부 전용 `#[cfg(test)]`).
        let mgr = PluginManager::with_registries(
            Arc::new(NoopWakerFactory),
            parked[0].0.file_format.clone(),
            parked[0].0.file_handler.clone(),
        );

        // App::about_to_wait 의 parked 순회 스텝과 동일한 shape.
        for (engine, _sid) in parked.iter_mut() {
            forward_mesh_frames_for_engine(engine, &mgr, &stream_hub);
        }

        for (engine, sid) in parked.iter_mut() {
            // 수정 전에는 이 구동 함수 자체가 parked engine 을 전혀 호출하지 않아
            // dirty/need_full_textures 가 계속 true 로 남는다(= "구독은 되지만
            // frame 이 갱신되지 않는" 버그 증상) — 수정 후에는 두 engine 모두
            // 방문돼 소비돼 있어야 한다.
            assert!(
                !engine.mesh_mirror.take_dirty(*sid),
                "parked engine's mesh mirror subscription should have been driven"
            );
            assert!(
                !engine.mesh_mirror.take_need_full_textures(*sid),
                "parked engine's need_full_textures should have been consumed"
            );
            // 방어적 정리(`is_hard_occupied` 체크)가 살아있는 구독을 잘못 지우지
            // 않아야 한다.
            assert!(engine.attach.is_hard_occupied(*sid));
        }
    }
}
