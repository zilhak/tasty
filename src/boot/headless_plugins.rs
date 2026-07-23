//! 헤드리스 전용 PluginManager 부트스트랩 + pump (attach mesh mirror 선행조건,
//! `.claude-workspace/todo/17-attach-headless-plugin-bootstrap.md`).
//!
//! `App::new_headless` 는 `plugin_manager: None` 으로 뜬다 — 헤드리스는 "GUI 없음"을
//! 전제한 코드 경로가 넓어 상시 초기화는 회귀 위험이 크다. 대신 attach 세션이 실제로
//! (mesh mirror 후보를 포함할 수 있는) workspace 를 mirror 하려 할 때만 lazy 초기화하고,
//! 이후로는 프로세스 수명 동안 유지한다(GUI 도 PluginManager 를 재생성하지 않는 것과
//! 동일한 정책 — 세션당 attach 종료 시 tear-down 하지 않음, 스코프 결정).
//!
//! pump 트리거 (busy-poll 편승 없이 이벤트 기반):
//! - plugin 프로세스의 수신 스레드는 매 라인마다 `waker.make_default_waker()()`
//!   를 호출한다(`tasty-host-plugin` `process.rs`). 이 waker 는 `PluginManager` 를
//!   만들 때 넘긴 `SharedWakerFactory` — 즉 `CoreState::waker_factory` 와 **동일
//!   인스턴스**를 공유한다. 헤드리스에서 default waker 는 `AppEvent::TerminalOutput(None)`
//!   을 발화하므로, plugin 이벤트(hello 응답, `PaintFrame` 등)는 이미 이 이벤트로 host 를
//!   깨운다 — 별도 wake 채널이 필요 없다(18번 TODO 의 "PaintFrame 도착 시 즉시 wake"
//!   요구도 이 경로가 충족한다).
//! - 1Hz busy ticker(`AppEvent::BusyPoll`)에도 안전망으로 편승 — plugin 소켓이 조용해도
//!   healthcheck/재시작 타이머가 진행되도록.

use crate::app::App;
use crate::core::CoreState;
use crate::state::AppState;

/// attach 세션이 mesh mirror 후보를 mirror 하려 할 때 호출 — 이미 초기화돼 있으면
/// no-op. `engine.waker_factory` 가 없으면(불변식 위반 — headless 는 부팅 시 항상
/// 설정) 경고만 남기고 스킵한다.
pub(crate) fn ensure_plugin_manager(app: &mut App, engine: &CoreState) {
    if app.plugin_manager.is_some() {
        return;
    }
    let Some(factory) = engine.waker_factory.clone() else {
        tracing::warn!(
            "headless plugin manager bootstrap skipped: engine has no waker_factory (invariant violated)"
        );
        return;
    };
    let mut mgr = crate::plugin::PluginManager::with_registries(
        factory,
        engine.file_format.clone(),
        engine.file_handler.clone(),
    );
    mgr.set_surface_registry(engine.surface_registry.clone());
    mgr.set_i18n_registrar(std::sync::Arc::new(crate::i18n::BinI18nRegistrar));
    mgr.set_hook_handler_registry(std::sync::Arc::new(
        crate::hook_handler::HostHookHandlerPort,
    ));
    crate::plugin::install_builtins_if_needed(&mut mgr);
    mgr.refresh_packages();
    mgr.discover_and_start();
    tracing::info!("headless plugin manager bootstrapped (attach mesh mirror session)");
    app.plugin_manager = Some(mgr);
}

/// GUI `about_to_wait()` plugin 블록의 헤드리스 등가. hello 마무리(surface_kind
/// 등록) + pending plugin IPC 호출의 최소 처리(shared_buffer.create 인터셉트 +
/// 나머지는 기본 라우터로 dispatch)만 수행한다 — popup/banner 인터셉트, namespace
/// forward, host event bus 브로드캐스트 등 GUI 전용/부가 통지는 이 스코프(attach mesh
/// mirror 렌더에 필요한 최소 집합)에서 의도적으로 생략한다(스코프 결정, Gate4 검토 대상).
/// `plugin_manager` 가 `None` 이면 no-op.
pub(crate) fn pump_plugins(app: &mut App, state: &mut AppState, engine: &mut CoreState) {
    if app.plugin_manager.is_none() {
        return;
    }
    let hello_pairs = {
        let mgr = app.plugin_manager.as_mut().expect("checked Some above");
        mgr.pump()
    };
    if !hello_pairs.is_empty() {
        finalize_plugin_hello_headless(app, engine, hello_pairs);
    }
    dispatch_plugin_ipc_calls_headless(app, state, engine);
    forward_mesh_frames(app, engine);
}

/// `CoreState::mesh_mirror`(구독 상태)를 읽어 plugin 을 구동하고, 새 frame 을 attach
/// client 에 chunk forward 한다(TODO 18). `pump_plugins` 호출 tick 마다 실행돼
/// `PaintFrame` 도착 즉시(또는 1Hz busy-poll 안전망 tick 에) 반응한다 — 별도 wake
/// 채널 불필요(모듈 문서 §pump 트리거 참조).
///
/// 구독당 두 가지 독립 동작:
/// 1. **구독 상태가 dirty**(신규 구독/geometry·theme·focus 변경) — plugin 에
///    `surface.set_context` 재전송(첫 호출이면 `surface.create` bootstrap 선행).
/// 2. **아직 이 client 에 안 보낸 새 generation 의 frame 존재** — `SharedBuffer`에서
///    바이트를 읽어 chunk 로 쪼개 client 에 push.
/// 두 동작은 서로 독립이다 — geometry 변경 없이도 plugin 이 새 frame 을 밀 수 있고
/// (markdown 내부 애니메이션 등), 반대로 이번 tick 에 새 frame 이 없어도 geometry 변경은
/// 즉시 반영해야 한다.
fn forward_mesh_frames(app: &mut App, engine: &mut CoreState) {
    let Some(mgr) = app.plugin_manager.as_ref() else {
        return;
    };
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

        let dirty = engine.mesh_mirror.take_dirty(sid);
        let need_full = engine.mesh_mirror.take_need_full_textures(sid);

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
                    modifiers: Default::default(),
                    // attach client → plugin 입력 forward 는 20번 TODO(마지막 구현
                    // 단계) — 그 전까지는 항상 비운다(geometry/theme/focus 변경
                    // 반영은 여기서 이미 동작).
                    events: Vec::new(),
                },
                theme,
                need_full_textures: need_full,
            };
            mgr.send_surface_set_context(&plugin_id, &params);
        }

        let Some(frame) = mgr.egui_mesh_frame(sid) else {
            continue;
        };
        if !engine
            .mesh_mirror
            .should_forward_generation(sid, frame.generation)
        {
            continue;
        }
        let Some(mem) = mgr.plugin_buffer(&plugin_id, frame.buffer_id) else {
            continue;
        };
        // SAFETY: `buffer_id`는 이 plugin 이 `host.shared_buffer.create`로 만든 뒤
        // `PaintFrame` 이벤트로 알려온 값이라 `mem`은 유효한 매핑이다. footer
        // 프로토콜은 plugin(writer)/host(reader) 양쪽이 합의한 8B 헤더 + user data
        // 레이아웃이며(`tasty_shm::footer` 문서), 이 host 프로세스가 이 버퍼의 유일한
        // reader 다 — `gfx/gpu/egui_mesh_prepare.rs::decode_mesh_into_target`의 동일
        // 패턴과 동형(그쪽은 GPU 디코드까지 하지만 여기는 raw 바이트 forward 만 한다).
        let raw = unsafe { mem.as_slice() };
        if raw.len() < tasty_shm::footer::SIZE {
            continue;
        }
        // SAFETY: 위에서 `raw.len() >= footer::SIZE`를 검증했고, mmap 매핑의 시작
        // 주소는 항상 페이지 정렬(≥8B)이라 `AtomicU64` 재해석이 안전하다(`footer_atomic`
        // 안전 조건 문서 참조).
        let gen_now = unsafe { tasty_shm::footer::load(raw, std::sync::atomic::Ordering::Acquire) };
        if gen_now != frame.generation {
            // writer 가 다음 frame 을 쓰는 중(tear) — 다음 tick 에 재시도(GPU 경로와 동형).
            continue;
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
            continue;
        };
        for payload in tasty_ipc::mesh_stream::split_mesh_frame(
            sid,
            frame_id,
            frame.generation,
            frame.frame_seq,
            frame.full_textures,
            bytes,
        ) {
            let result = app.stream_hub.push(
                client_id,
                crate::ipc::stream::StreamFrame::new(
                    crate::ipc::stream::StreamTag::MeshData,
                    payload,
                ),
            );
            if matches!(
                result,
                crate::adapters::production::stream_hub::PushResult::Unknown
                    | crate::adapters::production::stream_hub::PushResult::Disconnected
            ) {
                // client 가 끊겼다 — 다음 StreamReady tick 의 disconnected 처리가
                // `remove_for_client`로 구독을 정리한다. 남은 chunk 전송은 낭비이므로
                // 이 surface 는 여기서 중단.
                break;
            }
        }
    }
}

/// `src/app/plugin_glue/lifecycle.rs::finalize_plugin_hello` 의 헤드리스 등가.
/// surface_kind registry 등록(egui-mesh 포함) + hook_event 등록만 수행하고, GUI
/// 전용 CoreEvent cascade(toast/이벤트버스 브로드캐스트)는 생략한다 — 그 브로드캐스트는
/// PluginLoaded 등을 구독하는 *다른* plugin/UI 통지용이며, hello 를 마친 plugin 자신의
/// 렌더링에는 영향이 없다(레지스트리 mutation 은 이 함수 안에서 이미 동기 반영됨).
fn finalize_plugin_hello_headless(
    app: &mut App,
    engine: &CoreState,
    hello_pairs: Vec<(String, String)>,
) {
    let core_registry = engine.surface_registry.clone();
    let hook_event_registry = engine.plugin_hook_events.clone();
    let Some(mgr) = app.plugin_manager.as_mut() else {
        return;
    };

    for (plugin_id, _) in &hello_pairs {
        if let Some(pkg) = mgr.packages.iter().find(|p| &p.manifest.id == plugin_id) {
            let keys: Vec<String> = pkg
                .manifest
                .contributes
                .hook_events
                .iter()
                .map(|h| h.key.clone())
                .collect();
            if !keys.is_empty() {
                hook_event_registry.register(plugin_id, keys);
            }
        }
    }

    let host_registry = mgr.surface_registry.is_some().then_some(core_registry);
    let Some(registry) = host_registry else {
        tracing::debug!(
            "headless plugin manager has no surface_registry; deferring registration of {} plugin(s)",
            hello_pairs.len()
        );
        for (plugin_id, _) in &hello_pairs {
            mgr.registered_plugins.insert(plugin_id.clone());
        }
        return;
    };

    for (plugin_id, _version) in &hello_pairs {
        if let Some(pkg) = mgr
            .packages
            .iter()
            .find(|p| &p.manifest.id == plugin_id)
            .cloned()
        {
            for decl in &pkg.manifest.surface_kinds {
                if let Some(default) = &decl.default_colors {
                    tasty_themes::add_plugin_surface_default(&decl.kind, default.clone());
                }
                match decl.rendering {
                    crate::plugin::manifest::SurfaceKindRendering::Remote
                    | crate::plugin::manifest::SurfaceKindRendering::Webview => {
                        // `plugin_bridge::remote_kind`/webview surface stand-in은
                        // GUI 전용(`#[cfg(feature = "gui")]`) — 실제 렌더가 창을
                        // 전제하는 surface 라 headless 에 재현할 대상이 없다. attach
                        // mesh mirror 스코프(markdown/image/mesh_demo=egui-mesh) 밖이라
                        // 등록을 skip 한다(기존 headless 동작과 동일 — 회귀 아님).
                        tracing::debug!(
                            "plugin '{}' declared non-egui-mesh surface kind '{}' \
                             (rendering={:?}); skipped in headless (gui-only registration)",
                            plugin_id,
                            decl.kind,
                            decl.rendering
                        );
                    }
                    crate::plugin::manifest::SurfaceKindRendering::EguiMesh => {
                        crate::engine::surface_registry::egui_mesh::register_egui_mesh_kind(
                            &registry,
                            plugin_id,
                            decl,
                            &pkg.manifest.api_version,
                        );
                    }
                }
            }
        }
        mgr.registered_plugins.insert(plugin_id.clone());
    }
}

/// `src/app/dispatch/plugin_ipc.rs::process_plugin_ipc_calls` 의 헤드리스 등가.
/// `host.shared_buffer.create` 는 egui-mesh 프레임 생성에 필수라 그대로 인터셉트한다.
/// popup.close/banner.open/banner.close/namespace forward 는 헤드리스에 대응하는
/// GUI 상태(popup/banner overlay, view)가 없어 이 스코프에서는 생략 — 대상 plugin
/// (markdown/image/mesh_demo)이 기동 시 이들을 호출하지 않는 한 영향 없다(생략 항목은
/// 스코프를 벗어나는 발견 시 별도 TODO로 기록).
fn dispatch_plugin_ipc_calls_headless(app: &mut App, state: &mut AppState, engine: &mut CoreState) {
    let calls = match app.plugin_manager.as_mut() {
        Some(mgr) => mgr.take_pending_plugin_calls(),
        None => return,
    };
    for call in calls {
        if call.method == tasty_plugin_protocol::METHOD_HOST_SHARED_BUFFER_CREATE {
            let size = call
                .params
                .get("size")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if let Some(mgr) = app.plugin_manager.as_mut() {
                let (result, error) =
                    match mgr.create_shared_buffer_for(&call.plugin_id, call.call_id, size) {
                        Ok(r) => (serde_json::to_value(&r).ok(), None),
                        Err(e) => (None, Some(e)),
                    };
                mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
            }
            continue;
        }
        let caller = crate::ipc::caller::CallerContext::Plugin {
            plugin_id: call.plugin_id.clone(),
            permissions: call.permissions.clone(),
        };
        let request = crate::ipc::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::from(call.call_id)),
            method: call.method.clone(),
            params: call.params.clone(),
            session_token: None,
        };
        let response = crate::ipc::handler::handle_with_caller(
            &mut app.core,
            state,
            engine,
            &request,
            &caller,
        );
        let (result, error) = match response.error {
            Some(err) => (None, Some(err.message)),
            None => (response.result, None),
        };
        if let Some(mgr) = app.plugin_manager.as_mut() {
            mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
        }
    }
}
