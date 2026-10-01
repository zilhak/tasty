//! Settings editors return values. App applies registry/file effects after the modal is removed.
use crate::file::{
    format::{DetectorDecl, DetectorId},
    handler::{HandlerId, UserHandlerUpsertDecl},
};
#[derive(Clone, Debug)]
pub(crate) enum RegistryEdit {
    Extension {
        extension: String,
        order: Vec<DetectorId>,
    },
    DetectorEnabled(DetectorId, bool),
    RemoveDetector(DetectorId),
    AddDetector(DetectorDecl),
    HandlerEnabled(HandlerId, bool),
    RemoveHandler(HandlerId),
    AddHandler(UserHandlerUpsertDecl),
    HookEnabled(crate::hook_handler::HookHandlerId, bool),
    RemoveHook(crate::hook_handler::HookHandlerId),
    UpsertHook(crate::hook_handler::registry::UserHookHandlerUpsertDecl),
}
#[derive(Default)]
pub(crate) struct SettingsEdits {
    pub(crate) registry: Vec<RegistryEdit>,
    pub(crate) clear_scrollback: bool,
    #[cfg(windows)]
    pub(crate) bashrc: Option<String>,
}

pub(crate) struct SettingsEditOwner {
    pub(crate) format: std::sync::Arc<crate::file::format::FileFormatRegistry>,
    pub(crate) handler: std::sync::Arc<crate::file::handler::FileHandlerRegistry>,
    pub(crate) path: Option<std::path::PathBuf>,
}
impl super::App {
    pub(crate) fn apply_settings_edits(&mut self, owner: SettingsEditOwner, edits: SettingsEdits) {
        if edits.clear_scrollback {
            crate::scrollback_store::clear_all();
        }
        #[cfg(windows)]
        if let Some(bashrc) = edits.bashrc
            && let Err(error) = crate::settings::general::save_user_bashrc(&bashrc)
        {
            self.surface_bashrc_save_failure(&error);
        }
        let SettingsEditOwner {
            format,
            handler,
            path,
        } = owner;
        let hook = crate::hook_handler::global();
        let mut files_changed = false;
        let mut hooks_changed = false;
        for edit in edits.registry {
            match edit {
                RegistryEdit::Extension { extension, order } => {
                    if order.is_empty() {
                        format.clear_user_extension_priority(&extension);
                    } else {
                        format.set_user_extension_priority(&extension, order);
                    }
                    files_changed = true;
                }
                RegistryEdit::DetectorEnabled(id, enabled) => {
                    format.set_user_detector_disabled(&id, !enabled);
                    files_changed = true;
                }
                RegistryEdit::RemoveDetector(id) => {
                    format.remove_user_detector(&id);
                    files_changed = true;
                }
                RegistryEdit::AddDetector(value) => {
                    if let Err(error) = format.upsert_user_detector(value) {
                        tracing::warn!(%error,"settings detector edit failed");
                    }
                    files_changed = true;
                }
                RegistryEdit::HandlerEnabled(id, enabled) => {
                    handler.set_user_handler_disabled(&id, !enabled);
                    files_changed = true;
                }
                RegistryEdit::RemoveHandler(id) => {
                    handler.remove_user_handler(&id);
                    files_changed = true;
                }
                RegistryEdit::AddHandler(value) => {
                    if let Err(error) = handler.upsert_user_handler(value) {
                        tracing::warn!(%error,"settings handler edit failed");
                    }
                    files_changed = true;
                }
                RegistryEdit::HookEnabled(id, enabled) => {
                    hook.set_user_handler_disabled(&id, !enabled);
                    hooks_changed = true;
                }
                RegistryEdit::RemoveHook(id) => {
                    hook.remove_user_handler(&id);
                    hooks_changed = true;
                }
                RegistryEdit::UpsertHook(value) => {
                    if let Err(error) = hook.upsert_user_handler(value) {
                        tracing::warn!(%error,"settings hook edit failed");
                    }
                    hooks_changed = true;
                }
            }
        }
        if files_changed
            && let Some(path) = path
            && let Err(error) =
                crate::file::handler::save::save_combined_user_config(&format, &handler, &path)
        {
            tracing::warn!(%error,"settings file handler save failed");
        }
        if hooks_changed
            && let Some(path) = crate::hook_handler::user_config_path()
            && let Err(error) = hook.save_user_config(&path)
        {
            tracing::warn!(%error,"settings hook save failed");
        }
    }
}
