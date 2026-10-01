//! View supplies an explicit destination; the worker owns undo selection and immutable payload reads.
use super::*;
impl JournalApplication {
    pub(super) fn resolve_undo(&mut self, ticket: u64, session: &EngineSession) {
        let Some(pending) = self.commands.pending.get(&ticket) else {
            return;
        };
        let target_pane = pending
            .request
            .params
            .get("pane")
            .and_then(|value| value.as_u64())
            .and_then(|id| u32::try_from(id).ok());
        let scope = pending
            .request
            .params
            .get("scope")
            .and_then(|value| value.as_u64())
            .and_then(|id| u32::try_from(id).ok());
        let Some(binding) = session.journal_binding.clone() else {
            self.reject_resolved_request(
                ticket,
                JsonRpcResponse::internal_error(
                    serde_json::Value::Null,
                    "undo engine binding missing",
                ),
            );
            return;
        };
        let settings = &session.runtime.settings;
        let shell = crate::core::state::ShellConfig::from_settings(settings);
        let work = Work::PrepareUndo {
            binding,
            target_pane,
            scope,
            shell: crate::runtime::journal_product::ShellRecipe {
                executable: shell.shell,
                arguments: shell.args,
                environment: shell.envs,
                cols: session.runtime.default_cols,
                rows: session.runtime.default_rows,
                scrollback_lines: settings.general.scrollback_lines,
                disk_scrollback: settings.performance.scrollback_disk_swap,
                startup_command: settings.general.startup_command.clone(),
                restore_command: None,
            },
        };
        let pending = self
            .commands
            .pending
            .get_mut(&ticket)
            .expect("undo admission remains owned");
        pending.request.params = serde_json::Value::Null;
        pending.queued = Some(work);
        self.refresh_command_weight(ticket);
        (self.wake)();
    }
}

impl JournalApplication {
    pub(crate) fn resolve_preset(
        &mut self,
        ticket: u64,
        session: &EngineSession,
        services: &crate::app::services::AppServices,
        default_pane: Option<u32>,
    ) {
        let Some(pending) = self.commands.pending.get(&ticket) else {
            return;
        };
        let params = &pending.request.params;
        let result = (|| -> Result<Work, String> {
            let kind = match params.get("kind").and_then(|value| value.as_str()) {
                Some("workspace") => tasty_presets::PresetKind::Workspace,
                Some("tab") => tasty_presets::PresetKind::Tab,
                Some("pane") => tasty_presets::PresetKind::Pane,
                _ => return Err("Missing or invalid 'kind' parameter".into()),
            };
            let name = params
                .get("name")
                .and_then(|value| value.as_str())
                .ok_or("Missing 'name' parameter")?;
            let preset = crate::intent::preset::clone_preset_from_store(services, kind, name)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("preset not found: {}/{name}", kind.as_str()))?;
            let parse = |key| {
                crate::ipc::handler::params::optional_u32(params, key, &serde_json::Value::Null)
                    .map_err(|error| {
                        error
                            .error
                            .map_or_else(|| "invalid preset target".into(), |error| error.message)
                    })
            };
            let mut target = parse("target_pane_id")?.or(default_pane);
            if kind == tasty_presets::PresetKind::Pane
                && let Some(workspace) = parse("target_workspace_id")?
            {
                let workspace = session
                    .core_state
                    .find_workspace_index_for_id(workspace)
                    .and_then(|index| session.core_state.workspace_at(index))
                    .ok_or("preset target workspace missing")?;
                target = self
                    .completion_views
                    .get(&session.id)
                    .and_then(|view| view.focused_panes.get(&workspace.id).copied())
                    .filter(|pane| workspace.pane_layout().find_pane(*pane).is_some())
                    .or_else(|| workspace.pane_layout().first_pane().map(|pane| pane.id));
            }
            if kind != tasty_presets::PresetKind::Workspace
                && target
                    .and_then(|pane| session.core_state.find_workspace_index_for_pane(pane))
                    .and_then(|index| session.core_state.workspace_at(index))
                    .is_some_and(|workspace| workspace.mirror)
            {
                return Err("preset cannot be applied to a remote mirror".into());
            }
            if pending.request.method == "preset.apply"
                && kind != tasty_presets::PresetKind::Workspace
                && let Some(workspace) = target
                    .and_then(|pane| session.core_state.find_workspace_index_for_pane(pane))
                    .and_then(|index| session.core_state.workspace_at(index))
                && session
                    .live
                    .occupancy
                    .workspace_holder(workspace.id)
                    .is_some()
            {
                return Err(crate::ipc::handler::hard_occupied_denial(
                    workspace.id,
                    &serde_json::Value::Null,
                )
                .error
                .map_or_else(|| "workspace is occupied".into(), |error| error.message));
            }
            let category = if pending.request.method == "intent.preset-apply" {
                parse("category")?
            } else {
                None
            };
            let draft =
                crate::runtime::preset_plan::draft(&session.as_ref(), &preset, target, category)?;
            let binding = session
                .journal_binding
                .clone()
                .ok_or("preset engine has no journal binding")?;
            Ok(Work::PrepareSubtree { binding, draft })
        })();
        match result {
            Ok(work) => {
                let pending = self
                    .commands
                    .pending
                    .get_mut(&ticket)
                    .expect("preset admission remains owned");
                pending.request.params = serde_json::Value::Null;
                pending.queued = Some(work);
                self.refresh_command_weight(ticket);
                (self.wake)();
            }
            Err(error) => self.reject_resolved_request(
                ticket,
                JsonRpcResponse::invalid_params(serde_json::Value::Null, error),
            ),
        }
    }
}
