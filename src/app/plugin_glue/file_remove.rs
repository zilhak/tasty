use crate::{
    app::{
        App,
        command::{CoreEvent, PluginRegistryChange},
        plugin_file_lifecycle::Failure,
    },
    plugin,
};
use serde_json::{Value, json};
use tasty_host_plugin::{
    PluginsConfig, file_persistence::ConfigUpdate, known_plugins::KnownPlugins,
    manager::file_config::ConfigChange,
};

type Finish = Box<dyn FnOnce(&mut App, Result<Value, Failure>)>;
impl App {
    pub(crate) fn enqueue_plugin_remove(
        &mut self,
        id: String,
        finish: impl FnOnce(&mut App, Result<Value, Failure>) + 'static,
    ) -> Result<(), Failure> {
        let root =
            plugin::plugin_root().ok_or_else(|| Failure::before("plugin directory unavailable"))?;
        let known = KnownPlugins::path()
            .ok_or_else(|| Failure::before("known-plugins.toml path unavailable"))?;
        self.plugin_files
            .enqueue(move |app| app.start_plugin_remove(id, root, known, Box::new(finish)))?;
        (self.services.reply_wake())();
        Ok(())
    }

    fn start_plugin_remove(
        &mut self,
        id: String,
        root: std::path::PathBuf,
        known: std::path::PathBuf,
        finish: Finish,
    ) {
        self.start_plugin_config(
            ConfigChange::Enabled {
                id: id.clone(),
                enabled: false,
            },
            move |app, result| match result {
                Err(error) => finish(app, Err(error)),
                Ok(_) => app.after_plugin_retired(id.clone(), move |app| {
                    app.remove_plugin_files(id, root, known, finish)
                }),
            },
        );
    }

    fn remove_plugin_files(
        &mut self,
        id: String,
        root: std::path::PathBuf,
        known: std::path::PathBuf,
        finish: Finish,
    ) {
        let Some(before) = self.plugin_manager.as_ref().map(|mgr| mgr.config.clone()) else {
            finish(
                self,
                Err(after_disable(Failure::before("plugin manager unavailable"))),
            );
            return;
        };
        let bytes = crate::app::file_writer::command::value_bytes(&before) + id.len();
        let remove_id = id.clone();
        self.run_plugin_file(bytes,move ||remove_files(&root.join(&remove_id),&known,&remove_id,before),
            |result|match result {Ok(config)=>crate::app::file_writer::command::value_bytes(config),Err(e)=>e.message.len()},
            crate::app::plugin_file_lifecycle::classify_failure,
            move |app,result| {
                let result=match result {
                    Err(error)=>Err(after_disable(error)),
                    Ok(Err(error))=>{
                        if error.applied && let Err(cleanup)=app.apply_removed(id,None) {tracing::warn!(error=%cleanup.message,"partial removal cache refresh failed");}
                        Err(after_disable(error))
                    }
                    Ok(Ok(config))=>app.apply_removed(id,Some(config)),
                };
                finish(app,result);
            },
        );
    }

    fn apply_removed(
        &mut self,
        id: String,
        config: Option<PluginsConfig>,
    ) -> Result<Value, Failure> {
        let mgr = self
            .plugin_manager
            .as_mut()
            .ok_or_else(|| Failure::before("plugin manager unavailable after remove"))?;
        if let Some(config) = config {
            mgr.config = config;
        }
        mgr.replace_committed_package(&id, None);
        mgr.command_registry.unregister_plugin(&id);
        crate::i18n::unregister_namespace(&id);
        self.services.registries.plugin_hook_events.unregister(&id);
        self.cascade_plugin_events(vec![CoreEvent::PluginRegistryChanged {
            plugin_id: id.clone(),
            change: PluginRegistryChange::Removed,
        }]);
        self.refresh_tool_registry();
        self.refresh_palette_plugin_commands();
        self.refresh_plugin_window_after_file();
        Ok(json!({"removed":id}))
    }
}

fn remove_files(
    dir: &std::path::Path,
    known: &std::path::Path,
    id: &str,
    before: PluginsConfig,
) -> Result<PluginsConfig, Failure> {
    let mut removed = false;
    let result = (|| {
        // A changed path kind must not redirect recursive deletion through another user's link.
        let metadata = std::fs::symlink_metadata(dir)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::other(
                "plugin path is not an owned directory",
            ));
        }
        std::fs::remove_dir_all(dir)?;
        removed = true;
        tasty_host_plugin::trust_verdict::forget_unverified_choice_in(known, id)?;
        let mut after = before.clone();
        after.enable(id);
        if tasty_host_plugin::builtin::is_builtin_plugin(id) {
            after.mark_builtin_removed(id);
        }
        let update = ConfigUpdate::between(&before, &after)?.with_enabled_intent(id, true);
        let update = if tasty_host_plugin::builtin::is_builtin_plugin(id) {
            update.with_builtin_removed_intent(id, true)
        } else {
            update
        };
        update.execute()
    })();
    result.map_err(|error| Failure {
        message: error.to_string(),
        saved: Some(false),
        applied: removed,
        details: json!({"removed":removed}),
    })
}

fn after_disable(mut error: Failure) -> Failure {
    error.applied = true;
    if !error.details.is_object() {
        error.details = json!({});
    }
    error.details["disabled"] = json!(true);
    error.details["partial"] = json!(true);
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_final_config_reports_known_partial_removal_and_protects_original() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("plugin");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("owned"), "fixture").unwrap();
        let path = home.path().join("plugins.toml");
        let before = PluginsConfig::load_from(path.clone());
        std::fs::write(&path, "PRIVATE = [[[\n").unwrap();
        let error = remove_files(&dir, &home.path().join("known.toml"), "p", before).unwrap_err();
        assert!(error.applied);
        assert_eq!(error.saved, Some(false));
        assert_eq!(error.details["removed"], true);
        assert!(!error.message.contains("PRIVATE"));
        assert!(!dir.exists());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "PRIVATE = [[[\n");
    }
    #[cfg(unix)]
    #[test]
    fn changed_remove_path_kind_preserves_link_and_foreign_directory() {
        let home = tempfile::tempdir().unwrap();
        let foreign = home.path().join("foreign");
        std::fs::create_dir(&foreign).unwrap();
        std::fs::write(foreign.join("keep"), "private").unwrap();
        let dir = home.path().join("plugin");
        std::os::unix::fs::symlink(&foreign, &dir).unwrap();
        let error = remove_files(
            &dir,
            &home.path().join("known.toml"),
            "p",
            PluginsConfig::load_from(home.path().join("plugins.toml")),
        )
        .unwrap_err();
        assert!(!error.applied);
        assert!(
            std::fs::symlink_metadata(&dir)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(foreign.join("keep")).unwrap(),
            "private"
        );
    }
    #[test]
    fn unknown_after_disable_retains_known_effects_and_unknown_file_outcome() {
        let error = after_disable(Failure {
            message: "unknown".into(),
            saved: None,
            applied: false,
            details: Value::Null,
        });
        assert!(error.applied);
        assert_eq!(error.saved, None);
        assert_eq!(error.details["partial"], true);
        assert_eq!(error.response_data()["outcome"], "unknown");
    }
}
