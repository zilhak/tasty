//! Child terminal creation fixes its relation before entering the ordinary resource lifecycle.
use super::*;

pub(super) fn resolve_spawn(
    request: &JsonRpcRequest,
    session: &mut EngineSession,
    view: &crate::runtime::journal_product::CompletionView,
) -> Result<create::Request, JsonRpcResponse> {
    use crate::ipc::handler::params;
    let id = serde_json::Value::Null;
    let bad = |reason: String| JsonRpcResponse::invalid_params(id.clone(), reason);
    let input = &request.params;
    let parent = params::require_u32(input, "parent", &id)
        .or_else(|_| params::require_u32(input, "surface", &id))?;
    let workspace = input
        .get("workspace")
        .and_then(|value| value.as_str())
        .ok_or_else(|| bad("missing workspace".into()))?;
    let core = &session.core_state;
    let workspace = workspace
        .parse::<u32>()
        .ok()
        .and_then(|id| {
            core.workspaces()
                .into_iter()
                .find(|workspace| workspace.id == id)
        })
        .or_else(|| {
            core.workspaces()
                .into_iter()
                .find(|candidate| candidate.name == workspace)
        })
        .ok_or_else(|| {
            bad(format!(
                "workspace '{workspace}' not found — pass its numeric workspace id"
            ))
        })?;
    let pane = params::optional_u32(input, "pane", &id)?
        .or_else(|| workspace.pane_layout().all_pane_ids().into_iter().next())
        .ok_or_else(|| bad("workspace has no panes".into()))?;
    let pane_value = core
        .find_pane_by_id(pane)
        .ok_or_else(|| bad(format!("Pane {pane} not found")))?;
    let target = core
        .find_workspace_index_for_pane(pane)
        .and_then(|index| core.workspace_at(index))
        .ok_or_else(|| bad("pane workspace disappeared".into()))?;
    if let Some(response) = crate::ipc::handler::spawn_target_guard(&session.as_ref(), pane, &id) {
        return Err(response);
    }
    let workspace = target.id;
    let tab_index = pane_value.tabs.len();
    let cwd = optional_str(input, "cwd");
    if let Some(path) = &cwd
        && !std::path::Path::new(path).is_dir()
    {
        return Err(bad(format!("cwd does not exist: {path}")));
    }
    let launch_cwd = cwd.as_ref().map(std::path::PathBuf::from).or_else(|| {
        if !session.runtime.settings.general.inherit_cwd {
            return None;
        }
        let tab = view
            .selected_tabs
            .get(&pane)
            .and_then(|id| pane_value.tabs.iter().find(|tab| tab.id == *id))
            .or_else(|| pane_value.tabs.first())?;
        let surface = view
            .selected_surfaces
            .get(&tab.id)
            .copied()
            .filter(|id| tab.contains_surface(*id))
            .or_else(|| tab.first_surface_id())?;
        session.as_ref().local_surface_cwd(surface)
    });
    let mut result =
        create::Request::base(session, "terminal", &serde_json::json!({}), launch_cwd)?;
    let index = session
        .runtime
        .child_terminals
        .reserve_index(parent)
        .map_err(|reason| JsonRpcResponse::internal_error(id.clone(), reason))?;
    let command = optional_str(input, "command");
    let recipe = crate::runtime::journal_product::ChildRecipe {
        parent,
        index,
        workspace,
        runtime_epoch: result.binding.runtime_epoch,
        cwd,
        role: optional_str(input, "role"),
        nickname: optional_str(input, "nickname"),
        has_command: command.is_some(),
        replacing: false,
    };
    result
        .input
        .as_mut()
        .ok_or_else(|| bad("child preparation input missing".into()))?
        .child = Some(recipe);
    result.plan.destination = tasty_core::CreationDestination::Tab {
        pane,
        tab: 0,
        index: tab_index,
    };
    result.plan.tab_name = format!("child{index}");
    result.plan.explicit_name = Some(result.plan.tab_name.clone());
    result.shape = create::Shape::Child { index, workspace };
    result.activate = false;
    result.one_shot_input = command.map(|body| {
        if body.contains('\n') {
            format!("\x1b[200~{body}\x1b[201~")
        } else {
            body
        }
    });
    Ok(result)
}

fn optional_str(input: &serde_json::Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

pub(super) fn resolve_respawn(
    request: &JsonRpcRequest,
    session: &EngineSession,
) -> Result<create::Request, JsonRpcResponse> {
    let id = serde_json::Value::Null;
    let input = &request.params;
    let parent = crate::ipc::handler::params::optional_u32(input, "surface", &id)?
        .or_else(|| {
            session
                .runtime
                .child_terminals
                .single_parent(&session.core_state.live_surface_ids())
        })
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(
                id.clone(),
                "missing 'surface' parameter (0 or >1 parents — specify --surface)",
            )
        })?;
    let index = crate::ipc::handler::params::require_u32(input, "child", &id)?;
    let entry = session
        .runtime
        .child_terminals
        .find_child(parent, index)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(
                id.clone(),
                format!("child {index} not found under surface {parent}"),
            )
        })?;
    let cwd = optional_str(input, "cwd").ok_or_else(|| {
        JsonRpcResponse::invalid_params(id.clone(), "structural child respawn requires cwd")
    })?;
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("cwd does not exist: {cwd}"),
        ));
    }
    let spec = super::create_spec::Spec {
        destination: super::create_spec::Destination::Convert {
            surface: entry.child_surface_id,
            respawn: true,
        },
        kind: "terminal".into(),
        cwd: Some(cwd.clone().into()),
        params: serde_json::json!({}),
    };
    let mut result = create::Request::from_spec(spec, session)?;
    let workspace = session
        .core_state
        .find_workspace_index_for_surface(entry.child_surface_id)
        .and_then(|(index, _)| session.core_state.workspace_at(index))
        .ok_or_else(|| JsonRpcResponse::invalid_params(id, "child surface disappeared"))?
        .id;
    let command = optional_str(input, "command");
    result.input.as_mut().expect("fresh preparation").child =
        Some(crate::runtime::journal_product::ChildRecipe {
            parent,
            index,
            workspace,
            runtime_epoch: result.binding.runtime_epoch,
            cwd: Some(cwd),
            role: optional_str(input, "role").or_else(|| entry.role.clone()),
            nickname: optional_str(input, "nickname").or_else(|| entry.nickname.clone()),
            has_command: command.is_some(),
            replacing: true,
        });
    result.shape = create::Shape::ChildRespawn { index };
    result.one_shot_input = command;
    Ok(result)
}
