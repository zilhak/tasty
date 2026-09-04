//! 헤드리스 전용 PluginManager 부트스트랩 + pump (attach mesh mirror 선행조건 —
//! `docs/dev-guide/attach-behavior.md` "mesh mirror 채널").
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
//!   깨운다 — 별도 wake 채널이 필요 없다("PaintFrame 도착 시 즉시 wake" 요구도 이
//!   경로가 충족한다).
//! - plugin 자체 주기 작업(ping/healthcheck/RSS/auto-reload)은 `PluginManager` 가
//!   소유한 타이머 허브가 스케줄한다 — headless 메인 루프가 그 데드라인을 자기
//!   대기 계산에 합성하므로(`docs/dev-guide/timer-hub.md`) plugin 소켓이 조용해도
//!   제때 깨어난다. 1Hz `Tick::Busy` 에 편승하던 안전망은 그래서 더는 필요 없다.

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
    mgr.set_completion_strategy_registry(std::sync::Arc::new(
        crate::completion_strategy::HostCompletionStrategyPort,
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
        mgr.pump(std::time::Instant::now())
    };
    if !hello_pairs.is_empty() {
        finalize_plugin_hello_headless(app, engine, hello_pairs);
    }
    dispatch_plugin_ipc_calls_headless(app, state, engine);
    forward_mesh_frames(app, engine);
}

/// `CoreState::mesh_mirror`(구독 상태)를 읽어 plugin 을 구동하고, 새 frame 을 attach
/// client 에 chunk forward 한다(상세
/// `docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-소비-경로`).
/// `pump_plugins` 호출 tick 마다 실행돼
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
    crate::plugin_bridge::mesh_forward::forward_mesh_frames_for_engine(
        engine,
        mgr,
        &app.stream_hub,
    );
}

/// `src/app/plugin_glue/lifecycle.rs::finalize_plugin_hello` 의 헤드리스 등가.
/// surface_kind registry 등록(egui-mesh 포함) + hook_event 등록만 수행하고, GUI
/// 전용 CoreEvent cascade(toast/이벤트버스 브로드캐스트)는 생략한다 — 그 브로드캐스트는
/// PluginLoaded 등을 구독하는 *다른* plugin/UI 통지용이며, hello 를 마친 plugin 자신의
/// 렌더링에는 영향이 없다(레지스트리 mutation 은 이 함수 안에서 이미 동기 반영됨).
#[allow(clippy::cognitive_complexity)] // complexity-exempt: hello 쌍마다 surface_kind 등록 · egui-mesh 분기 · hook_event 등록을 순차로 도는 평평한 루프라 중첩이 얕고, 쪼개면 세 함수가 같은 레지스트리 핸들 두 개를 계속 주고받게 된다. gui 등가(`plugin_glue/lifecycle.rs::finalize_plugin_hello`)와 나란히 읽히는 편이 유지에 유리하다.
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
                        crate::core::surface_registry::egui_mesh::register_egui_mesh_kind(
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
        // plugin 호출도 같은 IPC 핸들러를 타므로(예: Claude 플러그인 훅의
        // `surface.completion`) 결과 회신 전에 Intent 큐를 적용한다 —
        // `docs/adr/0111-headless-drains-the-intent-queue.md`.
        crate::intent::headless::drain_pending_intents(&mut app.core, state, engine);
        let (result, error) = match response.error {
            Some(err) => (None, Some(err.message)),
            None => (response.result, None),
        };
        if let Some(mgr) = app.plugin_manager.as_mut() {
            mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error);
        }
    }
}
