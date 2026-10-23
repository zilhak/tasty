//! Retain the completed file stage when live manager application cannot proceed.
use super::Failure;
use crate::plugin::PluginManager;
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Registration,
    Removed { config_saved: bool },
}
pub(crate) fn manager_after_files(
    manager: &mut Option<PluginManager>,
    stage: Stage,
) -> Result<&mut PluginManager, Failure> {
    manager.as_mut().ok_or_else(|| {
        let (message, saved, applied, details) = match stage {
            Stage::Registration => (
                "plugin manager unavailable after registration", true, false,
                serde_json::json!({"files_committed":true,"registered":false,"partial":true}),
            ),
            Stage::Removed { config_saved } => (
                "plugin manager unavailable after remove", config_saved, true,
                serde_json::json!({"files_committed":config_saved,"disabled":true,"removed":true,"cache_updated":false,"partial":true}),
            ),
        };
        Failure { message:message.into(), saved:Some(saved), applied, details }
    })
}
