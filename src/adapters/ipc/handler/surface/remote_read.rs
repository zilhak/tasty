//! mirror 터미널의 `screen_text`·`read_since_mark`·`set_mark` 를 서버 원본으로 보낸다.
//! mirror 가 아니면 기존 로컬 핸들러가 답한다. 전송과 대기는 [`crate::remote::terminal_read`] 가 맡는다.
//!
//! mirror 의 출력 보관 버퍼는 attach snapshot 으로 다시 만든 사본이라 attach 이전 원문이 없고 유실이
//! `skipped` 에 남지 않는다. scrollback 도 attach 이전 행이 없다. 그래서 서버가 지원하지 않거나 연결이
//! 없으면 로컬 사본으로 답하지 않고 `remote_read_unsupported`·`remote_disconnected` 오류로 답한다.

use serde_json::Value;
use tasty_ipc::protocol::JsonRpcResponse;

use crate::runtime::engine_access::EngineMut;

pub(crate) fn screen_text(
    #[cfg_attr(
        not(feature = "gui"),
        expect(unused_variables, reason = "a headless instance has no mirrors")
    )]
    core: &crate::app::services::AppServices,
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    #[cfg(feature = "gui")]
    if let Some(workspace) = mirror_terminal(engine, params) {
        use crate::remote::terminal_read::client::{self as reads, Request};
        let lines = match crate::ipc::handler::params::opt_int::<usize>(params, "lines", &id) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let show_dim = params
            .get("show_dim")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(e) = reads::unavailable(workspace) {
            return JsonRpcResponse::error_with_data(id, e.code, e.message, e.data);
        }
        return defer(
            core,
            engine,
            id,
            workspace,
            params,
            Request::Screen { lines, show_dim },
        );
    }
    super::handle_screen_text(&engine.as_ref(), id, params)
}

pub(crate) fn read_since_mark(
    #[cfg_attr(
        not(feature = "gui"),
        expect(unused_variables, reason = "a headless instance has no mirrors")
    )]
    core: &crate::app::services::AppServices,
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    #[cfg(feature = "gui")]
    if let Some(workspace) = mirror_terminal(engine, params) {
        use crate::remote::terminal_read::client::{self as reads, Request};
        let parsed = match super::mark::OutputReadParams::parse(params, &id) {
            Ok(v) => v.req,
            Err(e) => return *e,
        };
        if let Some(e) = reads::unavailable(workspace) {
            return JsonRpcResponse::error_with_data(id, e.code, e.message, e.data);
        }
        let cursor = match parsed.from {
            tasty_terminal::OutputCursor::At(at) => {
                Some((at, parsed.expect_stream.unwrap_or_default()))
            }
            tasty_terminal::OutputCursor::Mark => None,
        };
        let request = Request::Output {
            cursor,
            max_bytes: Some(parsed.max_bytes),
            strip_ansi: parsed.strip_ansi,
        };
        return defer(core, engine, id, workspace, params, request);
    }
    super::handle_read_since_mark(engine, id, params)
}

/// mirror 에서는 서버 위치를 받은 뒤 로컬 마크와 mirror 마크를 함께 바꾼다. 실패하면 둘 다 그대로다.
pub(crate) fn set_mark(
    #[cfg_attr(
        not(feature = "gui"),
        expect(unused_variables, reason = "a headless instance has no mirrors")
    )]
    core: &crate::app::services::AppServices,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    #[cfg(feature = "gui")]
    if let Some(workspace) = mirror_terminal(engine, params) {
        use crate::remote::terminal_read::client::{self as reads, Request};
        if let Some(e) = reads::unavailable(workspace) {
            return JsonRpcResponse::error_with_data(id, e.code, e.message, e.data);
        }
        return defer(core, engine, id, workspace, params, Request::Mark);
    }
    super::handle_set_mark(out, engine, id, params)
}

/// 대상이 mirror workspace 의 터미널이면 그 workspace id.
#[cfg(feature = "gui")]
fn mirror_terminal(engine: &EngineMut<'_>, params: &Value) -> Option<u32> {
    let surface = params
        .get("surface_id")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())?;
    engine.runtime.terminals.get(surface)?;
    let core = &engine.core;
    let pane = core.find_pane_for_surface(surface)?;
    let workspace = core
        .find_workspace_index_for_pane(pane)
        .and_then(|i| core.workspace_at(i))?;
    workspace.mirror.then_some(workspace.id)
}

#[cfg(feature = "gui")]
fn defer(
    #[cfg_attr(
        not(feature = "gui"),
        expect(unused_variables, reason = "a headless instance has no mirrors")
    )]
    core: &crate::app::services::AppServices,
    engine: &mut EngineMut<'_>,
    id: Value,
    workspace: u32,
    params: &Value,
    request: crate::remote::terminal_read::client::Request,
) -> JsonRpcResponse {
    use crate::remote::terminal_read::client as reads;
    let Some(surface) = params
        .get("surface_id")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
    else {
        return JsonRpcResponse::invalid_params(id, "missing 'surface_id'");
    };
    let request_id = reads::start(workspace, surface, request, std::time::Instant::now());
    (engine.runtime.waker)();
    let reply_id = id.clone();
    core.reply_with_file(
        id,
        move || reads::reply(request_id, reply_id.clone(), std::time::Instant::now()),
        |_, response| response,
    )
}
