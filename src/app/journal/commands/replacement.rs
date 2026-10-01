//! Fixed source/target replacement; only overwritten runtime owners enter cleanup.
use super::*;
pub(super) struct Request {
    pub engine: EngineId,
    stream: String,
    replacement: tasty_core::Replacement,
    expected: Vec<tasty_core::RetiredSurface>,
}
impl Request {
    fn resolve(request: &JsonRpcRequest, session: &EngineSession) -> Result<Self, JsonRpcResponse> {
        let input = &request.params;
        let id = serde_json::Value::Null;
        let replacement = if request.method == "intent.move-surface" {
            let source = crate::ipc::handler::params::require_u32(input, "source", &id)?;
            let target = crate::ipc::handler::params::require_u32(input, "target", &id)?;
            tasty_core::Replacement {
                source: tasty_core::EntityId {
                    kind: tasty_core::IdKind::Surface,
                    id: source,
                },
                target: tasty_core::EntityId {
                    kind: tasty_core::IdKind::Surface,
                    id: target,
                },
            }
        } else {
            serde_json::from_value(input.clone())
                .map_err(|error| JsonRpcResponse::invalid_params(id.clone(), error.to_string()))?
        };
        let core = &session.core_state;
        let surfaces = |entity: tasty_core::EntityId| -> Vec<u32> {
            match entity.kind {
                tasty_core::IdKind::Surface => core
                    .has_surface(entity.id)
                    .then_some(entity.id)
                    .into_iter()
                    .collect(),
                tasty_core::IdKind::Tab => core
                    .find_pane_for_tab(entity.id)
                    .and_then(|pane| core.find_pane_by_id(pane))
                    .and_then(|pane| pane.tabs.iter().find(|tab| tab.id == entity.id))
                    .map(|tab| tab.all_surface_ids())
                    .unwrap_or_default(),
                tasty_core::IdKind::Pane => core
                    .find_pane_by_id(entity.id)
                    .map(|pane| {
                        pane.tabs
                            .iter()
                            .flat_map(|tab| tab.all_surface_ids())
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            }
        };
        let ids: Vec<_> = surfaces(replacement.source)
            .into_iter()
            .chain(surfaces(replacement.target))
            .collect();
        if ids.iter().any(|surface| {
            core.find_workspace_index_for_surface(*surface)
                .and_then(|(index, _)| core.workspace_at(index))
                .is_some_and(|workspace| workspace.mirror)
        }) {
            return Err(JsonRpcResponse::invalid_params(
                id,
                "replacement cannot mix local and remote trees",
            ));
        }
        let expected = ids
            .into_iter()
            .filter_map(|id| {
                core.find_surface_by_id(id)
                    .map(|surface| tasty_core::RetiredSurface {
                        id,
                        kind: surface.kind.clone(),
                        activation_generation: surface.activation_generation,
                    })
            })
            .collect();
        Ok(Self {
            engine: session.id,
            stream: session
                .journal_binding
                .as_ref()
                .ok_or_else(|| JsonRpcResponse::internal_error(id, "replacement engine unbound"))?
                .stream
                .clone(),
            replacement,
            expected,
        })
    }
    pub fn stored(&self, input: tasty_core::DataRef) -> Work {
        Work::Resolve {
            changes: vec![StreamCommand {
                stream: self.stream.clone(),
                command: tasty_core::StructuralCommand::Replace {
                    operation: tasty_core::OperationId(String::new()),
                    command_id: String::new(),
                    input,
                    replacement: self.replacement,
                    expected: self.expected.clone(),
                },
            }],
            response: Some(ResponsePlan::Moved {
                success: JsonRpcResponse::success(
                    serde_json::Value::Null,
                    serde_json::json!({"moved":true}),
                ),
                not_moved: JsonRpcResponse::success(
                    serde_json::Value::Null,
                    serde_json::json!({"moved":false}),
                ),
            }),
        }
    }
    pub fn weight(&self) -> usize {
        self.stream.len()
            + serde_json::to_vec(&(&self.replacement, &self.expected))
                .map_or(usize::MAX, |bytes| bytes.len())
    }
}
impl JournalApplication {
    pub(super) fn resolve_replacement(&mut self, ticket: u64, session: &EngineSession) {
        let Some(pending) = self.commands.pending.get(&ticket) else {
            return;
        };
        match Request::resolve(&pending.request, session) {
            Ok(request) => {
                let bytes = match serde_json::to_vec(&request.replacement) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.reject_resolved_request(
                            ticket,
                            JsonRpcResponse::internal_error(
                                serde_json::Value::Null,
                                error.to_string(),
                            ),
                        );
                        return;
                    }
                };
                let pending = self
                    .commands
                    .pending
                    .get_mut(&ticket)
                    .expect("replacement admission remains owned");
                pending.request.params = serde_json::Value::Null;
                pending.queued = Some(Work::PutPayload(bytes));
                pending.replacing = Some(request);
                self.refresh_command_weight(ticket);
                (self.wake)();
            }
            Err(response) => self.reject_resolved_request(ticket, response),
        }
    }
}
