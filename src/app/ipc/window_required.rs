//! 포커스 창을 사용하는 debug 입력·조회 메서드. release에서는 처리하지 않는다.
//! ID로 대상을 지정하는 ui.screenshot은 app_methods에 있다.

#[cfg(debug_assertions)]
use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::app::ipc::IpcStep;
#[cfg(debug_assertions)]
use crate::ipc as host_ipc;
use crate::ipc::server::IpcCommand;
#[cfg(debug_assertions)]
use crate::ipc::server::send_response;

/// 잘못된 단위를 기본값으로 바꾸면 다른 주입 경로를 시험하게 되므로 거절한다.
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

/// 두 포인터 주입 경로에서 숫자 인자를 같은 방식으로 읽는다.
/// 알 수 없는 버튼 번호는 Left, event_type은 Move가 된다.
#[cfg(debug_assertions)]
fn read_pointer_params(
    p: &serde_json::Value,
    unit: crate::view::main::debug_input::ScrollUnit,
) -> Result<(f32, f32, crate::view::main::debug_input::InjectPointer), String> {
    use crate::view::main::debug_input::InjectPointer;

    // 좌표는 비율로 받으며 여기서 0..1 범위로 제한하지는 않는다.
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
    #[cfg(not(debug_assertions))]
    pub(crate) fn ipc_step_window_required(&mut self, _cmd: &IpcCommand) -> IpcStep {
        IpcStep::NotHandled
    }

    #[cfg(debug_assertions)]
    pub(crate) fn ipc_step_window_required(&mut self, cmd: &IpcCommand) -> IpcStep {
        // 새 창 의존 메서드를 추가하면 이 분류도 갱신해야 한다.
        let is_window_required = cmd.request.method.starts_with("surface.ime_")
            || cmd.request.method == "debug.info"
            || cmd.request.method == "debug.inject_window_mouse"
            || cmd.request.method == "debug.inject_egui_mouse"
            || cmd.request.method == "debug.inject_egui_key"
            || cmd.request.method == "debug.inject_egui_text"
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
            // 현재 ID가 MainView를 가리키지 않으면 별도 응답 없이 처리됨으로 반환한다.
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
        // winit 이벤트 대신 egui 입력 큐에 넣어 popup 입력 전달을 시험한다.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_egui_mouse" {
            use crate::view::main::debug_input::ScrollUnit;
            let params = &cmd.request.params;
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
        // TextEdit의 문자 입력은 키 이벤트와 별도의 Text 이벤트다.
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.inject_egui_text" {
            let params = &cmd.request.params;
            let Some(text) = params.get("text").and_then(|v| v.as_str()) else {
                return reject_bad_params(cmd, "missing or non-string 'text'");
            };
            let ok = w.debug_inject_egui_text(text);
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
        #[cfg(debug_assertions)]
        if cmd.request.method == "debug.pending_menu" {
            // 실제 열린 메뉴가 없으면 주입 시 캡처한 메뉴 정보를 조회한다.
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
        // engine 목록과 별개인 view의 포커스 surface를 조회한다.
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

#[cfg(debug_assertions)]
fn pending_menu_kind(menu: &crate::state::PendingNativeMenu) -> (&'static str, Option<u32>) {
    use crate::state::PendingNativeMenu as M;
    match menu {
        M::Tab { .. } => ("Tab", None),
        M::Pane { .. } => ("Pane", None),
        M::Workspace { .. } => ("Workspace", None),
        M::TerminalSurface { surface_id, .. } => ("TerminalSurface", Some(*surface_id)),
        M::TerminalLink { link, .. } => ("TerminalLink", Some(link.surface_id)),
        M::Surface { surface_id, .. } => ("Surface", Some(*surface_id)),
        M::Explorer { surface_id, .. } => ("Explorer", Some(*surface_id)),
        M::ExplorerFavorite { surface_id, .. } => ("ExplorerFavorite", Some(*surface_id)),
        M::NewWorkspaceButton { .. } => ("NewWorkspaceButton", None),
        M::WorkspaceCategoryHeader { .. } => ("WorkspaceCategoryHeader", None),
        M::SidebarBackground { .. } => ("SidebarBackground", None),
        M::NewTabButton { .. } => ("NewTabButton", None),
    }
}
