//! Debug 빌드 전용 IPC 핸들러 — cell_info / screen_attrs / feed_bytes /
//! glyph_color / inject_mouse / inject_key.
//!
//! 디버그 빌드에서만 라우터에 등록되며, 사용자의 키/마우스 입력을 재현하는
//! 저수준 IPC 표면을 제공한다. release 빌드에는 본 모듈이 컴파일되지 않는다.

#![cfg(debug_assertions)]

use super::params::{self, p_try};
use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

pub(super) fn require_input_simulation(
    _state: &AppState,
    engine: &crate::core::CoreState,
    id: &serde_json::Value,
) -> Result<(), JsonRpcResponse> {
    if !engine.input_simulation_enabled {
        Err(JsonRpcResponse::error(
            id.clone(),
            -32001,
            "input simulation not enabled. Launch tasty with --enable-input-simulation",
        ))
    } else {
        Ok(())
    }
}

/// Inject a mouse event into a surface's PTY as if the terminal received it.
/// Encodes as SGR mouse (mode 1006) bytes: ESC [ < Cb ; Cx ; Cy M/m
#[cfg(debug_assertions)]
pub(super) fn handle_debug_inject_mouse(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, engine, &id) {
        return e;
    }
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    // col, row: 1-indexed cell coordinates
    let col = match p_try!(params::opt_int::<u64>(params, "col", &id)) {
        Some(c) => c,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'col' parameter"),
    };
    let row = match p_try!(params::opt_int::<u64>(params, "row", &id)) {
        Some(r) => r,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'row' parameter"),
    };
    // button: 0=left, 1=middle, 2=right. Default: 0
    let button = p_try!(params::opt_int::<u64>(params, "button", &id)).unwrap_or(0);
    // event_type: "press", "release", "move". Default: "press"
    let event_type = params
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("press");

    let cb = match event_type {
        "press" | "release" => button as u8,
        "move" => 32 + button as u8,
        _ => return JsonRpcResponse::invalid_params(id, "event_type must be press/release/move"),
    };
    // SGR mouse(1006). col/row 입력은 0-indexed → 1-based 로. 공용 인코더 사용.
    let bytes = tasty_terminal::encode_mouse_report(
        true,
        cb,
        (col + 1) as usize,
        (row + 1) as usize,
        event_type == "release",
    );

    if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
        terminal.send_bytes(&bytes);
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// `debug.host_popup.list` — 호스트 빌트인 popup(`PopupDef`) 전체 목록을 반환.
///
/// plugin 이 contribute 한 popup 은 `debug.popup.list` 가 담당한다. 이쪽은 tasty
/// 본체가 `popup::defs::all_defs()` 로 정의한 빌트인 popup (tools_menu / port_scanner
/// / command_palette / remote_tool 등) 전용이다. 사용자 클릭 경로 없이 popup 을
/// 직접 띄워 시각 검증하기 위한 debug 격리 표면.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_host_popup_list(
    state: &AppState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let items: Vec<_> = crate::adapters::ui::popup::defs::all_defs()
        .iter()
        .map(|def| {
            // 열려 있는 popup 은 현재 rect 와 z_seq 를 함께 노출한다 — 겹친 popup 의
            // 마우스 소유권 판정(`popup/occlusion.rs`)을 debug 로 검증할 때 좌표를
            // 실측 없이 조준하기 위한 관찰면. z_seq 는 plugin popup 과 공유하는 전역
            // 시퀀스라 `debug.popup.list` 값과 직접 비교된다.
            let geom = state.popups.open_geometry(def.id);
            json!({
                "id": def.id,
                "title_key": def.title_key,
                "headless": def.headless,
                "close_on_outside_click": def.close_on_outside_click,
                "open": geom.is_some(),
                "z_seq": geom.map(|(z, _)| z),
                "rect": geom.map(|(_, r)| {
                    json!({ "x": r.min.x, "y": r.min.y, "w": r.width(), "h": r.height() })
                }),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "popups": items }))
}

/// `debug.host_popup.open` — `{ popup_id }` 로 호스트 빌트인 popup 을 화면 중앙에
/// 강제로 띄운다. 사용자 클릭(사이드바 도구 버튼 → 메뉴 → 항목) 을 재현하는
/// 디버그 동작이므로 release 에 노출되지 않는다.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_host_popup_open(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(popup_id) = params.get("popup_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'popup_id' parameter");
    };
    // 런타임 문자열 → 정적 def id (`PopupId` 는 &'static str). 정의에 없는 id 는 거부.
    let Some(def) = crate::adapters::ui::popup::defs::find(popup_id) else {
        return JsonRpcResponse::error(id, -32602, format!("host popup '{popup_id}' not found"));
    };
    // `workspace_scope` 는 런타임 스코프 주입(`OpenPopupMode::WithScope`)을 쓰는
    // popup 을 위한 것이다 — 그런 popup 은 `CenteredFocused` 로 열면 스코프가
    // 기본값(`Window`)에 머물러 워크스페이스 가시성 게이트가 아예 발동하지 않는다.
    let workspace_scope = params
        .get("workspace_scope")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mode = if workspace_scope {
        crate::intent::OpenPopupMode::WithScope(crate::adapters::ui::popup::PopupScope::Workspace(
            state.active_workspace,
        ))
    } else {
        crate::intent::OpenPopupMode::CenteredFocused
    };
    state.dispatch_intent(crate::intent::UiIntent::OpenPopup { id: def.id, mode }.from_agent_ipc());
    JsonRpcResponse::success(
        id,
        json!({ "opened": def.id, "workspace_scope": workspace_scope }),
    )
}

/// `debug.host_popup.close` — `{ popup_id }` 로 호스트 빌트인 popup 을 닫는다.
/// 여러 popup 을 차례로 스크린샷할 때 직전 popup 을 정리하는 용도.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_host_popup_close(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(popup_id) = params.get("popup_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'popup_id' parameter");
    };
    let Some(def) = crate::adapters::ui::popup::defs::find(popup_id) else {
        return JsonRpcResponse::error(id, -32602, format!("host popup '{popup_id}' not found"));
    };
    state.dispatch_intent(crate::intent::UiIntent::ClosePopup { id: def.id }.from_agent_ipc());
    JsonRpcResponse::success(id, json!({ "closed": def.id }))
}

/// `debug.modifier_hint.hold` — `{ ctrl, alt, option, shift, elapsed_ms? }` 로 오버레이의
/// 홀드 조합을 직접 세팅한다(생략 축 = false, 모두 false 면 홀드 해제). `elapsed_ms` 가
/// 있으면 타이머를 그만큼 과거로 백데이트해 표시 지연 게이트를 즉시 통과시킨다.
///
/// 원칙1상 오버레이는 실 modifier 홀드로만 뜨지만, 이는 PTY raw 주입이 아니라 오버레이
/// 내부 상태만 세팅하는 force-state 라 `host_popup.open` 과 동일하게 debug 격리로 충분하다.
/// 응답은 `state` 와 동일한 렌더 상태 덤프. release 미노출.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_modhint_hold(
    state: &mut AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let axis = |k: &str| params.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    state
        .modifier_hint
        .update_hold(axis("ctrl"), axis("alt"), axis("option"), axis("shift"));
    if let Some(ms) = p_try!(params::opt_int::<u64>(params, "elapsed_ms", &id)) {
        state
            .modifier_hint
            .debug_backdate(std::time::Duration::from_millis(ms));
    }
    let theme = crate::theme::theme();
    let reduced_motion = engine.settings.accessibility.reduced_motion;
    let dump = crate::adapters::ui::modifier_hint_overlay::debug_state_json(
        &state.modifier_hint,
        &engine.settings,
        &theme,
        reduced_motion,
    );
    JsonRpcResponse::success(id, dump)
}

/// `debug.modifier_hint.state` — 오버레이의 현재 렌더 상태를 draw 경로와 동일 로직으로
/// 재평가해 덤프한다(held / 지연 / alpha / visible / header_combo / 좁혀진 sections). 스크린샷
/// 없이 좁힘·즉시갱신·지연을 자동 단정하기 위한 debug 격리 표면. release 미노출.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_modhint_state(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let theme = crate::theme::theme();
    let reduced_motion = engine.settings.accessibility.reduced_motion;
    let dump = crate::adapters::ui::modifier_hint_overlay::debug_state_json(
        &state.modifier_hint,
        &engine.settings,
        &theme,
        reduced_motion,
    );
    JsonRpcResponse::success(id, dump)
}

/// `debug.banner.list` — 빌트인 배너 정의 목록 + 현재 표시/대기 상태 + **기하**를 반환.
///
/// 배너는 사용자 행동에서만 발사되므로(발화 정책 §불가침) 이 표면은 release 에
/// 노출되지 않는다. plugin 기여 배너는 (도입 시) 별도 표면이 담당한다.
///
/// # 좌표계 — 두 rect 가 서로 다르다
///
/// 응답 최상위의 `coords` 가 이것을 그대로 싣는다. 이 레포에서 좌표계를 안 적으면
/// 반드시 틀린다(`docs/concepts/typed-length.md`).
///
/// - `shown[].rect` — 셸(카드) 영역. **egui 논리 좌표.** `debug.host_popup.list` 의
///   `rect` 와 같은 좌표계·같은 키 모양(`x`/`y`/`w`/`h`)이라 두 응답을 직접 비교할 수
///   있다. 배율이 바뀌어도 값이 같아야 한다 — 달라지면 그 자체가 DPI 결함 신호다.
/// - `shown[].content_rect` — plugin egui-mesh 배너의 콘텐츠 합성 영역. **물리 픽셀.**
///   셸은 host egui 가 논리로 그리고 콘텐츠는 plugin 이 ppp 로 재렌더해 GPU 가 합성하는
///   별개 경로라, 논리로 접어 내리면 그 두 경로를 가르는 정보가 사라진다. host 배너는
///   `null`.
///
/// `rect` 는 popup 과 달리 **한 프레임 늦고**, 배너가 뜬 직후 첫 프레임에는 `null` 이다 —
/// 배너는 자기 좌표를 모델에 들고 있지 않고 컨테이너가 매 프레임 배치하므로 좌표가
/// 그린 뒤에야 확정된다(`BannerManager::card_rect`).
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_list(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let defs: Vec<_> = crate::adapters::ui::banner::defs::all_defs()
        .iter()
        .map(|def| {
            json!({
                "id": def.id,
                "ttl_seconds": def.ttl_seconds,
            })
        })
        .collect();
    let shown: Vec<_> = state
        .banners
        .shown_banners()
        .map(|b| {
            let queued: Vec<&str> = state
                .banners
                .queued_banners(&b.scope)
                .map(|q| q.id)
                .collect();
            // 셸 rect: 직전 프레임 실측(논리). popup 과 같은 키 모양으로 낸다.
            let rect = state
                .banners
                .card_rect(&b.scope, &b.key())
                .map(|r| json!({ "x": r.min.x, "y": r.min.y, "w": r.width(), "h": r.height() }));
            // 콘텐츠 rect: plugin egui-mesh 배너만. GPU 합성 영역이라 물리 픽셀이다.
            let content_rect = match &b.content {
                crate::adapters::ui::banner::BannerContentSource::PluginMesh {
                    instance_id,
                    ..
                } => state
                    .plugin_mesh_banner_regions
                    .iter()
                    .find(|(inst, _)| inst == instance_id)
                    .map(|(_, r)| {
                        json!({
                            "x": r.x.value(),
                            "y": r.y.value(),
                            "w": r.width.value(),
                            "h": r.height.value(),
                        })
                    }),
                crate::adapters::ui::banner::BannerContentSource::Host => None,
            };
            json!({
                "id": b.id,
                "scope": b.scope.to_token(),
                "remaining_seconds": b.remaining_seconds(),
                "queued": queued,
                "rect": rect,
                "content_rect": content_rect,
            })
        })
        .collect();
    JsonRpcResponse::success(
        id,
        json!({
            "defs": defs,
            "shown": shown,
            "total_queued": state.banners.total_queued(),
            // 좌표계를 응답이 스스로 말한다 — 문서만 아는 사실이면 호출부가 물리로 읽는다.
            "coords": { "rect": "logical", "content_rect": "physical" },
        }),
    )
}

/// `debug.banner.show` — `{ banner_id, scope }` 로 배너를 직접 발화한다.
///
/// 사용자 조작(마우스 캡쳐 surface 에서 드래그 등) 을 재현하는 디버그 동작이라
/// release 에 없다. `scope` 는 `view` / `workspace:<i>` / `pane:<id>` /
/// `tab:<pane>:<i>` / `surface:<id>` 토큰. def 의 ttl 을 그대로 적용한다.
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_show(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(banner_id) = params.get("banner_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'banner_id' parameter");
    };
    let Some(scope_token) = params.get("scope").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'scope' parameter");
    };
    let Some(def) = crate::adapters::ui::banner::defs::find(banner_id) else {
        return JsonRpcResponse::error(id, -32602, format!("banner '{banner_id}' not found"));
    };
    let Some(scope) = crate::adapters::ui::BannerScope::from_token(scope_token) else {
        return JsonRpcResponse::error(id, -32602, format!("invalid scope '{scope_token}'"));
    };
    let banner = match def.ttl_seconds {
        Some(secs) => crate::adapters::ui::BannerState::with_ttl(def.id, scope, secs),
        None => crate::adapters::ui::BannerState::persistent(def.id, scope),
    };
    let outcome = state.banners.push(banner);
    JsonRpcResponse::success(id, json!({ "outcome": format!("{outcome:?}") }))
}

/// `debug.banner.close` — `{ banner_id }` 로 배너를 닫는다 (표시 중이면 큐 head 승격).
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_close(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(banner_id) = params.get("banner_id").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'banner_id' parameter");
    };
    let Some(def) = crate::adapters::ui::banner::defs::find(banner_id) else {
        return JsonRpcResponse::error(id, -32602, format!("banner '{banner_id}' not found"));
    };
    let closed = state.banners.close_by_id(def.id);
    JsonRpcResponse::success(id, json!({ "closed": closed }))
}

/// `debug.banner.set_countdown` — `{ scope, seconds }` 로 표시 중 TTL 배너의
/// 남은 시간을 강제 설정한다 (만료 직전 상태 등 시각 검증용).
#[cfg(all(debug_assertions, feature = "gui"))]
pub(super) fn handle_debug_banner_set_countdown(
    state: &mut AppState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(scope_token) = params.get("scope").and_then(|v| v.as_str()) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'scope' parameter");
    };
    let Some(seconds) = p_try!(params::opt_int::<u64>(params, "seconds", &id)) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'seconds' parameter");
    };
    let Some(scope) = crate::adapters::ui::BannerScope::from_token(scope_token) else {
        return JsonRpcResponse::error(id, -32602, format!("invalid scope '{scope_token}'"));
    };
    let applied = state.banners.set_countdown(&scope, seconds as u32);
    JsonRpcResponse::success(id, json!({ "applied": applied }))
}

/// Inject a key event into a surface's PTY.
#[cfg(debug_assertions)]
pub(super) fn handle_debug_inject_key(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    if let Err(e) = require_input_simulation(state, engine, &id) {
        return e;
    }
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let bytes = match params.get("bytes").and_then(|v| v.as_str()) {
        Some(hex) => {
            let hex = hex.trim();
            let mut result = Vec::new();
            for i in (0..hex.len()).step_by(2) {
                match u8::from_str_radix(&hex[i..i.min(hex.len()).max(i + 2)], 16) {
                    Ok(b) => result.push(b),
                    Err(_) => return JsonRpcResponse::invalid_params(id, "Invalid hex in 'bytes'"),
                }
            }
            result
        }
        None => match params.get("text").and_then(|v| v.as_str()) {
            Some(t) => t.as_bytes().to_vec(),
            None => {
                return JsonRpcResponse::invalid_params(id, "Missing 'bytes' or 'text' parameter");
            }
        },
    };

    if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&String::from_utf8_lossy(&bytes));
        JsonRpcResponse::success(id, json!({"sent": true}))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

/// `debug.gpu.stall` — 다음 프레임의 `present` 직전을 `ms` 밀리초 블로킹하도록 예약한다.
///
/// 실제 GPU 드라이버 행을 결정적으로 재현할 수 없으므로, 같은 구조(이벤트 루프 스레드
/// 안에서 반환하지 않는 GPU 호출)를 인위적으로 만들어 stall 워치독을 검증한다.
///
/// `debug_assertions` 가 cfg 에 반드시 들어간다 — 호출 대상인 `arm_debug_stall` 이 debug
/// 전용이라, 이 함수만 gui 로 남으면 호출자가 없어도 release 에서 타입체크에 걸려 빌드가
/// 깨진다(`route_debug_handler` 는 debug 전용이라 dead code 경고도 뜨지 않는다).
pub(super) fn handle_debug_gpu_stall(
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(ms) = params.get("ms").and_then(serde_json::Value::as_u64) else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'ms' parameter (u64)");
    };
    crate::stall_watchdog::arm_debug_stall(ms);
    JsonRpcResponse::success(id, serde_json::json!({ "armed_ms": ms }))
}
