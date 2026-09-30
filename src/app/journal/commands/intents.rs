//! Fixed engine intents and settings continuations share ordinary journal admission.
use super::*;

impl JournalApplication {
    pub(crate) fn admit_metadata_intent(
        &mut self,
        engine_id: EngineId,
        core: &crate::core::CoreState,
        intent: &crate::core::intent::DomainIntent,
        origin: &crate::intent::IntentOrigin,
    ) -> bool {
        use crate::core::intent::DomainIntent as I;
        let (method, params) = match intent {
            I::CreateCategory { name } => (
                "workspace_category.create",
                serde_json::json!({"name":name}),
            ),
            I::RenameCategory { id, name } => (
                "workspace_category.rename",
                serde_json::json!({"id":id,"name":name}),
            ),
            I::DeleteCategory { id } => ("workspace_category.delete", serde_json::json!({"id":id})),
            I::ReorderCategory {
                from_index,
                to_index,
            } => {
                let Some(category) = core.categories.get(*from_index) else {
                    self.commands.deliver(
                        Reply::Intent {
                            engine: engine_id,
                            origin: origin.clone(),
                        },
                        JsonRpcResponse::invalid_params(
                            serde_json::Value::Null,
                            "category index out of range",
                        ),
                    );
                    (self.wake)();
                    return true;
                };
                (
                    "workspace_category.move",
                    serde_json::json!({"id":category.id,"to_index":to_index}),
                )
            }
            I::UpdateWorkspaceMeta {
                workspace_id,
                name,
                subtitle,
                description,
            } => (
                "workspace.update",
                serde_json::json!({"id":workspace_id,"name":name,"subtitle":subtitle,"description":description}),
            ),
            I::MoveWorkspace {
                workspace_id,
                to_index,
            } => (
                "workspace.move",
                serde_json::json!({"id":workspace_id,"to_index":to_index}),
            ),
            I::SetWorkspaceCategory {
                workspace_id,
                category,
            } => (
                "workspace.update",
                serde_json::json!({"id":workspace_id,"category":category}),
            ),
            I::SetWorkspaceAttachMapping {
                workspace_id,
                mapping,
            } => (
                "intent.workspace-mapping",
                serde_json::json!({"id":workspace_id,"mapping":mapping}),
            ),
            _ => return false,
        };
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id: None,
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        self.admit_request(
            &request,
            Reply::Intent {
                engine: engine_id,
                origin: origin.clone(),
            },
            match origin {
                crate::intent::IntentOrigin::User { .. } => "user",
                crate::intent::IntentOrigin::Agent { .. } => "agent",
                crate::intent::IntentOrigin::System => "system",
            }
            .into(),
            &format!("intent:{origin:?}"),
        );
        true
    }
}

#[cfg(feature = "gui")]
impl JournalApplication {
    pub(crate) fn admit_fixed_intent(
        &mut self,
        session: &EngineSession,
        commands: Vec<tasty_domain::StructuralCommand>,
        origin: &crate::intent::IntentOrigin,
    ) -> Result<(), String> {
        let binding = session
            .journal_binding
            .as_ref()
            .ok_or("structural intent engine has no journal binding")?;
        if commands
            .iter()
            .any(|command| !command.reserved_ids().is_empty())
        {
            return Err("fixed intent requires admission-time ID reservation".into());
        }
        let changes: Vec<_> = commands
            .into_iter()
            .map(|command| StreamCommand {
                stream: binding.stream.clone(),
                command,
            })
            .collect();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "intent.structural".into(),
            params: serde_json::to_value(&changes).map_err(|error| error.to_string())?,
            id: None,
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let ticket = self.next_ticket;
        self.admit_request(
            &request,
            Reply::Intent {
                engine: session.id,
                origin: origin.clone(),
            },
            intent_actor(origin).into(),
            &format!("intent:{origin:?}"),
        );
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            pending.fixed = Some(changes);
            pending.request.params = serde_json::Value::Null;
        }
        self.refresh_command_weight(ticket);
        Ok(())
    }
}

#[cfg(feature = "gui")]
impl JournalApplication {
    pub(crate) fn admit_settings_reset(
        &mut self,
        generation: u64,
        changes: Vec<StreamCommand>,
        settings: crate::settings::Settings,
        origin: &crate::intent::IntentOrigin,
    ) {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "intent.category-reset".into(),
            params: serde_json::to_value(&changes).expect("fixed commands serialize"),
            id: None,
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let ticket = self.next_ticket;
        self.admit_request(
            &request,
            Reply::Settings {
                settings: Some(Box::new(settings)),
                generation,
                origin: origin.clone(),
            },
            intent_actor(origin).into(),
            &format!("intent:{origin:?}"),
        );
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            pending.fixed = Some(changes);
            pending.request.params = serde_json::Value::Null;
        }
        self.refresh_command_weight(ticket);
    }
}

#[cfg(feature = "gui")]
impl JournalApplication {
    pub(crate) fn note_settings_intent(&mut self) -> Result<u64, String> {
        let generation = self
            .commands
            .settings_generation
            .checked_add(1)
            .ok_or("settings continuation generation exhausted")?;
        self.commands.settings_generation = generation;
        Ok(generation)
    }

    pub(super) fn take_settings_results(
        &mut self,
    ) -> Vec<(
        Option<Box<crate::settings::Settings>>,
        crate::intent::IntentOrigin,
        JsonRpcResponse,
    )> {
        std::mem::take(&mut self.commands.completed_settings)
            .into_iter()
            .filter_map(|(generation, settings, origin, response)| {
                (generation == self.commands.settings_generation)
                    .then_some((settings, origin, response))
            })
            .collect()
    }
}

#[cfg(feature = "gui")]
fn intent_actor(origin: &crate::intent::IntentOrigin) -> &'static str {
    match origin {
        crate::intent::IntentOrigin::User { .. } => "user",
        crate::intent::IntentOrigin::Agent { .. } => "agent",
        crate::intent::IntentOrigin::System => "system",
    }
}
