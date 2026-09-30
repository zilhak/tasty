//! Ratio facts use the projection revision and explicit layout target captured at drag start.
use super::*;
use crate::state::layout_preview::{DividerCommit, LayoutTarget};

impl JournalApplication {
    pub(crate) fn admit_divider(
        &mut self,
        session: &EngineSession,
        commit: DividerCommit,
        origin: &crate::intent::IntentOrigin,
    ) -> Result<(), String> {
        let binding = session
            .journal_binding
            .as_ref()
            .ok_or("divider engine has no journal binding")?;
        let command = match commit.target {
            LayoutTarget::Workspace(workspace_id) => {
                tasty_domain::StructuralCommand::SetPaneRatio {
                    workspace_id,
                    path: commit.path,
                    expected_leaves: commit.leaves,
                    expected_revision: commit.revision,
                    ratio: tasty_domain::Ratio::from_f32(commit.ratio),
                }
            }
            LayoutTarget::Tab(tab_id) => tasty_domain::StructuralCommand::SetSurfaceRatio {
                tab_id,
                path: commit.path,
                expected_leaves: commit.leaves,
                expected_revision: commit.revision,
                ratio: tasty_domain::Ratio::from_f32(commit.ratio),
            },
        };
        let changes = vec![StreamCommand {
            stream: binding.stream.clone(),
            command,
        }];
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "intent.divider".into(),
            params: serde_json::to_value(&changes).map_err(|error| error.to_string())?,
            id: None,
            session_token: None,
            response_timeout_ms: None,
            idempotency_key: None,
        };
        let ticket = self.next_ticket;
        self.admit_request(
            &request,
            Reply::Divider {
                engine: session.id,
                sequence: commit.sequence,
                origin: origin.clone(),
            },
            super::intents::intent_actor(origin).into(),
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
