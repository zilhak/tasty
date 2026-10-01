//! View supplies an explicit destination; the worker owns undo selection and immutable payload reads.
use super::*;
impl JournalApplication {
    pub(super) fn resolve_undo(&mut self,ticket:u64,session:&EngineSession) {
        let Some(pending)=self.commands.pending.get(&ticket) else{return;};
        let target_pane=pending.request.params.get("pane").and_then(|value|value.as_u64()).and_then(|id|u32::try_from(id).ok());
        let scope=pending.request.params.get("scope").and_then(|value|value.as_u64()).and_then(|id|u32::try_from(id).ok());
        let Some(binding)=session.journal_binding.clone() else {self.reject_resolved_request(ticket,JsonRpcResponse::internal_error(serde_json::Value::Null,"undo engine binding missing"));return;};
        let settings=&session.core_state.settings;
        let shell=crate::core::state::ShellConfig::from_settings(settings);
        let work=Work::PrepareUndo {binding,target_pane,scope,shell:crate::runtime::journal_product::ShellRecipe {
            executable:shell.shell,arguments:shell.args,environment:shell.envs,cols:session.core_state.default_cols,rows:session.core_state.default_rows,
            scrollback_lines:settings.general.scrollback_lines,disk_scrollback:settings.performance.scrollback_disk_swap,startup_command:settings.general.startup_command.clone(),restore_command:None,
        }};
        let pending=self.commands.pending.get_mut(&ticket).expect("undo admission remains owned");
        pending.request.params=serde_json::Value::Null;pending.queued=Some(work);
        self.refresh_command_weight(ticket);(self.wake)();
    }
}
