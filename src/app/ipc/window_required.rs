//! step 4: focused window 가 필요한 메서드 (GPU/IME/debug 도구).
//!
//! - `surface.ime_*` (debug only — 창 IME 조합 상태를 강제로 세팅하는 사용자
//!   입력 재현이고, 대상을 ID 로 받지 못한 채 포커스된 창에 작용한다)
//! - `debug.info` (debug only)
//!
//! 이 step 은 통째로 debug 표면이다 — release 빌드에서는 어떤 메서드도 여기
//! 걸리지 않는다.
//!
//! `ui.screenshot` was promoted to a release, focus-independent method — it now
//! lives in the `app_methods` step (targets window/surface by ID, not focus).

#[cfg(debug_assertions)]
use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::app::ipc::IpcStep;
use crate::ipc::server::IpcCommand;
// 응답을 실제로 만들어 보내는 것은 debug 경로뿐이다 — release stub 은 곧바로
// `NotHandled` 만 돌려준다.
#[cfg(debug_assertions)]
use crate::ipc as host_ipc;
#[cfg(debug_assertions)]
use crate::ipc::server::send_response;

/// 모르는 `unit` 값은 기본값으로 삼키지 않고 거절한다 — 오타를 point 로 대신 재면
/// 테스트가 의도한 것과 다른 환산 경로를 재고도 통과한다.
#[cfg(debug_assertions)]
fn reject_unknown_scroll_unit(cmd: &IpcCommand) -> IpcStep {
    let response = host_ipc::protocol::JsonRpcResponse::error(
        cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
        -32602,
        "unknown scroll unit: expected \"line\", \"point\" or \"page\"",
    );
    send_response(&cmd.response_tx, response);
    IpcStep::Handled
}

/// 잘못 온 params 는 기본값으로 삼키지 않고 거절한다 — 삼키면 주입이 의도한 것과
/// 다른 좌표·버튼을 재고도 "injected: true" 로 답한다.
#[cfg(debug_assertions)]
fn reject_bad_params(cmd: &IpcCommand, msg: &str) -> IpcStep {
    let response = host_ipc::protocol::JsonRpcResponse::error(
        cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
        -32602,
        msg,
    );
    send_response(&cmd.response_tx, response);
    IpcStep::Handled
}

/// 두 포인터 주입 경로(mesh · egui)가 **같은 키를 같은 방식으로** 읽게 한다.
///
/// 종전에는 두 블록이 각자 `p.get("fx").and_then(|v| v.as_f64()).unwrap_or(0.5)` 를
/// 적고 있었다 — 한쪽만 고치면 다른 쪽은 안 고쳐지고, 그 갈림은 아무 데서도 안 터진다.
/// 스칼라는 관문(`handler::params`)을 지난다: 잘못 온 값은 기본값이 되지 않는다.
#[cfg(debug_assertions)]
fn read_pointer_params(
    p: &serde_json::Value,
    unit: crate::view::main::debug_input::ScrollUnit,
) -> Result<(f32, f32, crate::view::main::debug_input::InjectPointer), String> {
    use crate::view::main::debug_input::InjectPointer;

    // fx, fy ∈ [0,1] surface-local 정규화 좌표 (기본 중앙).
    let fx = params::read_f64(p, "fx")?.unwrap_or(0.5) as f32;
    let fy = params::read_f64(p, "fy")?.unwrap_or(0.5) as f32;
    let button = match params::read_int::<u64>(p, "button")?.unwrap_or(0) {
        1 => winit::event::MouseButton::Middle,
        2 => winit::event::MouseButton::Right,
        _ => winit::event::MouseButton::Left,
    };
    let event_type = p
        .get("event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("move");
    let action = match event_type {
        "press" => InjectPointer::Button {
            button,
            pressed: true,
        },
        "release" => InjectPointer::Button {
            button,
            pressed: false,
        },
        "scroll" => InjectPointer::Scroll {
            dx: params::read_f64(p, "scroll_dx")?.unwrap_or(0.0) as f32,
            dy: params::read_f64(p, "scroll_dy")?.unwrap_or(0.0) as f32,
            unit,
        },
        _ => InjectPointer::Move,
    };
    Ok((fx, fy, action))
}

impl App {
    /// release 빌드에는 window-required 메서드가 하나도 없다 — 이 step 전체가
    /// debug 표면이라 통째로 사라진다. cfg 가드는 이 stub 한 쌍뿐이다.
    #[cfg(not(debug_assertions))]
    pub(crate) fn ipc_step_window_required(&mut self, _cmd: &IpcCommand) -> IpcStep {
        IpcStep::NotHandled
    }

    #[cfg(debug_assertions)]
    pub(crate) fn ipc_step_window_required(&mut self, cmd: &IpcCommand) -> IpcStep {
        // `surface.ime_` 접두는 **이름 판정이지만 이름이 곧 성질이다** — 이 접두를 가진
        // 메서드 집합과 `handle_ime_method` 가 실제로 푸는 arm 집합이 지금 정확히 같다
        // (다섯). 그래서 성질로 다시 써도 집합이 안 달라진다.
        //
        // 다만 그 일치는 저절로 유지되지 않는다. IME 메서드를 **다른 이름으로** 더하면
        // 이 관문이 안 걸어 창 없이 통과하고, 그 실패는 조용하다. 새 IME 메서드는 이
        // 접두를 쓰거나, 안 쓸 거면 아래 `==` 나열에 함께 적어라.
        let is_window_required = cmd.request.method.starts_with("surface.ime_")
            || cmd.request.method == "debug.info"
            || cmd.request.method == "debug.inject_window_mouse"
            || cmd.request.method == "debug.inject_egui_mouse"
            || cmd.request.method == "debug.inject_egui_key"
            || cmd.request.method == "debug.selection"
            || cmd.request.method == "debug.pending_menu"
            || cmd.request.method == "debug.focused_surface";
        if !is_window_required {
            return IpcStep::NotHandled;
        }
        let focused_id = match self.view.focused_view_id {
            Some(id) => id,
            None => {
                let response = host_ipc::protocol::JsonRpcResponse::error(
                    cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                    -32000,
                    "No window available for this command",
                );
                send_response(&cmd.response_tx, response);
                return IpcStep::Handled;
            }
        };
        let w = match self
            .view
            .views
            .get_mut(&focused_id)
            .and_then(|w| w.as_main_mut())
        {
            Some(w) => w,
            // focused id 가 있는데 MainView 가 아니면 (모달 등) 본 step 으로 처리 불가 —
            // 이 케이스를 옛 코드는 `continue` (드롭) 으로 처리했다. 동일 의미를
            // Handled 로 표현 (응답 전송 없음 → client 가 timeout).
            None => return IpcStep::Handled,
        };

        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.info" {
            let debug_data = crate::debug_info::collect(
                &w.state,
                &w.core_state,
                Some(&w.base.gpu),
                w.ime_active,
            );
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                debug_data,
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_window_mouse" {
            use crate::view::main::debug_input::ScrollUnit;
            let params = &cmd.request.params;
            // 이 경로는 종전까지 항상 winit `LineDelta` 를 합성했으므로 기본이 line 이다.
            let Some(unit) = ScrollUnit::from_name(
                params
                    .get("unit")
                    .and_then(|v| v.as_str())
                    .unwrap_or("line"),
            ) else {
                return reject_unknown_scroll_unit(cmd);
            };
            let surface_id = match params::read_u32(params, "surface_id") {
                Ok(v) => v.unwrap_or(0),
                Err(msg) => return reject_bad_params(cmd, &msg),
            };
            let (fx, fy, action) = match read_pointer_params(params, unit) {
                Ok(v) => v,
                Err(msg) => return reject_bad_params(cmd, &msg),
            };
            let ok = w.debug_inject_mesh_pointer(surface_id, fx, fy, action);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "injected": ok }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // egui-mesh popup(A2) 입력 forward 검증용 — winit 핸들러가 아니라 egui 입력 큐에
        // 직접 주입한다(popup 은 egui input 을 통해 plugin 으로 forward 되기 때문). 좌표는
        // window 정규화 (fx,fy ∈ [0,1] 논리). release 미노출, debug 격리(원칙 1·3).
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_egui_mouse" {
            use crate::view::main::debug_input::ScrollUnit;
            let params = &cmd.request.params;
            // 이 경로는 종전까지 항상 `MouseWheelUnit::Point` 를 합성했으므로 기본이
            // point 다 — 기존 호출자가 단위를 넘기지 않아도 같은 것을 재현한다.
            let Some(unit) = ScrollUnit::from_name(
                params
                    .get("unit")
                    .and_then(|v| v.as_str())
                    .unwrap_or("point"),
            ) else {
                return reject_unknown_scroll_unit(cmd);
            };
            let (fx, fy, action) = match read_pointer_params(params, unit) {
                Ok(v) => v,
                Err(msg) => return reject_bad_params(cmd, &msg),
            };
            let surface_id = match params::read_u32(params, "surface_id") {
                Ok(v) => v,
                Err(msg) => return reject_bad_params(cmd, &msg),
            };
            let ok = w.debug_inject_egui_pointer(fx, fy, surface_id, action);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "injected": ok }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_egui_key" {
            let params = &cmd.request.params;
            let key = params
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("Escape");
            let pressed = params
                .get("pressed")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let ok = w.debug_inject_egui_key(key, pressed);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "injected": ok }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // read-only debug dump: 마우스 라우팅이 만든 로컬 텍스트 선택 상태를 그대로 노출한다
        // (input 안전망 — press→move→release 주입 후 selection 회귀를 단언). 부수효과 0,
        // 사용자 상태를 변경하지 않으므로 관찰 전용. debug 격리(원칙 1·3), release 미노출.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.selection" {
            let sel = w.text_selection.as_ref();
            let body = match sel {
                Some(s) => {
                    let n = s.normalized();
                    serde_json::json!({
                        "present": true,
                        "surface_id": s.surface_id,
                        "mode": format!("{:?}", s.mode),
                        "dragging": s.dragging,
                        "empty": s.is_empty(),
                        "anchor": { "col": s.anchor.col, "row": s.anchor.absolute_row },
                        "cursor": { "col": s.cursor.col, "row": s.cursor.absolute_row },
                        "start": { "col": n.start.col, "row": n.start.absolute_row },
                        "end": { "col": n.end.col, "row": n.end.absolute_row },
                    })
                }
                None => serde_json::json!({ "present": false }),
            };
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                body,
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // read-only debug dump: 우클릭 라우팅이 세운 대기 중 컨텍스트 메뉴(종류/대상 surface).
        // 우클릭 주입 후 메뉴 라우팅 회귀를 단언한다. 관찰 전용, debug 격리, release 미노출.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.pending_menu" {
            // 주입 경로가 세운 메뉴는 `debug_captured_menu` 로 가로채져 있다(팝업 회피).
            // live pending 이 있으면(비-주입 경로) 그걸, 아니면 포획본을 관찰한다.
            let menu = w
                .state
                .dialogs
                .pending_native_menu
                .as_ref()
                .or(w.debug_captured_menu.as_ref());
            let body = match menu {
                Some(menu) => {
                    let (kind, surface_id) = pending_menu_kind(menu);
                    let mut obj = serde_json::json!({ "present": true, "kind": kind });
                    if let Some(sid) = surface_id {
                        obj["surface_id"] = serde_json::json!(sid);
                    }
                    obj
                }
                None => serde_json::json!({ "present": false }),
            };
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                body,
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        // read-only debug dump: 현재 포커스된 surface id (없으면 null). click-to-activate
        // 라우팅(비활성 surface 좌클릭 → 포커스 전환) 회귀를 단언한다. `surface.list` 는
        // engine 단위라 view-layer 포커스를 노출하지 않으므로 별도 관찰 IPC 가 필요하다.
        // 관찰 전용, debug 격리, release 미노출.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.focused_surface" {
            let focused = w.state.focused_surface_id(&w.core_state);
            let response = host_ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                serde_json::json!({ "surface_id": focused }),
            );
            send_response(&cmd.response_tx, response);
            return IpcStep::Handled;
        }
        if cmd.request.method.starts_with("surface.ime_") {
            let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            let response = host_ipc::handler::ime::handle_ime_method(
                w,
                &cmd.request.method,
                &cmd.request.params,
                id,
            );
            send_response(&cmd.response_tx, response);
            w.base.dirty = true;
        }
        IpcStep::Handled
    }
}

/// `PendingNativeMenu` variant → (kind 문자열, 대상 surface_id). 관찰용 debug dump 전용.
#[cfg(debug_assertions)]
fn pending_menu_kind(menu: &crate::state::PendingNativeMenu) -> (&'static str, Option<u32>) {
    use crate::state::PendingNativeMenu as M;
    match menu {
        M::Tab { .. } => ("Tab", None),
        M::Pane { .. } => ("Pane", None),
        M::Workspace { .. } => ("Workspace", None),
        M::TerminalSurface { surface_id, .. } => ("TerminalSurface", Some(*surface_id)),
        M::Surface { surface_id, .. } => ("Surface", Some(*surface_id)),
        M::Explorer { surface_id, .. } => ("Explorer", Some(*surface_id)),
        M::ExplorerFavorite { surface_id, .. } => ("ExplorerFavorite", Some(*surface_id)),
        M::NewWorkspaceButton { .. } => ("NewWorkspaceButton", None),
        M::WorkspaceCategoryHeader { .. } => ("WorkspaceCategoryHeader", None),
        M::SidebarBackground { .. } => ("SidebarBackground", None),
        M::NewTabButton { .. } => ("NewTabButton", None),
    }
}
