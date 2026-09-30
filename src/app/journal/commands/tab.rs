use super::display::{DisplayAction, DisplayContinuation};
use super::*;

pub(super) fn rename(
    request: &JsonRpcRequest,
    session: &EngineSession,
) -> Result<workspace::Resolved, JsonRpcResponse> {
    let input = &request.params;
    let id = serde_json::Value::Null;
    let tab_id = crate::ipc::handler::params::require_u32(input, "tab_id", &id)?;
    let core = &session.core_state;
    let pane_id = core.find_pane_for_tab(tab_id).ok_or_else(|| {
        JsonRpcResponse::invalid_params(id.clone(), format!("Tab id {tab_id} not found"))
    })?;
    let workspace = core
        .find_workspace_index_for_pane(pane_id)
        .and_then(|index| core.workspace_at(index))
        .ok_or_else(|| JsonRpcResponse::internal_error(id.clone(), "tab workspace missing"))?;
    let name: Option<String> = serde_json::from_value(input["name"].clone())
        .map_err(|error| JsonRpcResponse::invalid_params(id.clone(), error.to_string()))?;
    let notify = input.get("user_direct").and_then(|value| value.as_bool());
    let mirrors = if workspace.mirror {
        vec![(
            workspace.id,
            core.mirror_projection_token(workspace.id)
                .expect("mirror projection lifetime"),
        )]
    } else {
        Vec::new()
    };
    let changes = if workspace.mirror {
        Vec::new()
    } else {
        vec![tasty_domain::StructuralCommand::RenameTab {
            tab_id,
            name: name.clone(),
        }]
    };
    Ok(workspace::Resolved {
        changes,
        response: ResponsePlan::Fixed(JsonRpcResponse::success(
            id,
            serde_json::json!({"updated":true}),
        )),
        display: Some(DisplayContinuation {
            engine: session.id,
            mirrors,
            action: DisplayAction::TabName {
                tab_id,
                name,
                notify,
            },
        }),
    })
}

pub(super) fn move_tab(request: &JsonRpcRequest) -> Result<workspace::Resolved, JsonRpcResponse> {
    let id = serde_json::Value::Null;
    let pane_id = crate::ipc::handler::params::require_u32(&request.params, "pane_id", &id)?;
    let tab_id = crate::ipc::handler::params::require_u32(&request.params, "tab_id", &id)?;
    let to_index = crate::ipc::handler::params::opt_int::<usize>(&request.params, "to_index", &id)?
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "missing fixed destination index")
        })?;
    Ok(workspace::Resolved {
        changes: vec![tasty_domain::StructuralCommand::MoveTab {
            pane_id,
            tab_id,
            to_index,
        }],
        response: ResponsePlan::Fixed(JsonRpcResponse::success(
            id,
            serde_json::json!({"updated":true}),
        )),
        display: None,
    })
}
