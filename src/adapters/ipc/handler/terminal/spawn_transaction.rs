//! Commit the newly created child's relationship before any command reaches its PTY.
use super::*;

/// `child` must be the fresh surface returned by this spawn's tab.create.
pub(super) fn finish(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    id: &Value,
    parent: u32,
    child: ChildEntry,
    command: Option<&str>,
) -> Result<(), JsonRpcResponse> {
    let target = child.child_surface_id;
    if let Err(error) = engine.completion.begin_relation(parent, target) {
        let error =
            JsonRpcResponse::error(id.clone(), -32000, format!("completion relation: {error}"));
        return Err(rollback(core, state, engine, parent, &child, error));
    }
    engine.child_terminals.register_child(parent, child.clone());
    engine.child_terminals.save();
    let label = child.nickname.clone().or_else(|| child.role.clone());
    if let Err(error) = engine.occupy_soft(target, parent, label) {
        let error =
            JsonRpcResponse::error(id.clone(), -32020, format!("occupy_soft failed: {error:?}"));
        return Err(rollback(core, state, engine, parent, &child, error));
    }
    if let Some(command) = command {
        if let Err(error) =
            send_body_then_submit(engine, core, id, target, build_tell_payload(command))
        {
            return Err(rollback(core, state, engine, parent, &child, error));
        }
    }
    Ok(())
}

fn rollback(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    parent: u32,
    child: &ChildEntry,
    mut original: JsonRpcResponse,
) -> JsonRpcResponse {
    if engine
        .child_terminals
        .list_children(parent)
        .iter()
        .any(|entry| entry.index == child.index && entry.child_surface_id == child.child_surface_id)
    {
        engine.child_terminals.remove_child(parent, child.index);
        engine.child_terminals.save();
    }
    // Standard agent close runs PTY/occupancy/lifecycle cleanup without adding
    // user close history. The only close target is this invocation's fresh surface.
    let closed = surface::handle_surface_close(
        core,
        state,
        engine,
        original.id.clone(),
        &json!({"surface_id":child.child_surface_id}),
    );
    if let Some(error) = closed.error {
        tracing::warn!("spawn rollback close failed: {}", error.message);
        if let Some(primary) = original.error.as_mut() {
            primary.message.push_str(&format!(
                "; owned surface cleanup failed: {}",
                error.message
            ));
        }
    }
    original
}

#[cfg(test)]
mod tests;
