use crate::app::{
    App,
    command::{CoreEvent, PluginRegistryChange},
    plugin_file_lifecycle::Failure,
};
use serde_json::Value;
use std::collections::VecDeque;
use tasty_host_plugin::{
    BuiltinUpgradeAction, BuiltinUpgradeItem, BuiltinUpgradeReport,
    builtin::file_upgrade::{self, Entry, Written},
};

type Finish = Box<dyn FnOnce(&mut App, Result<Value, Failure>)>;
struct Upgrade {
    entries: VecDeque<Entry>,
    items: Vec<BuiltinUpgradeItem>,
    restart: bool,
    applied: bool,
    finish: Finish,
}

impl App {
    pub(crate) fn enqueue_builtin_upgrade(
        &mut self,
        force: bool,
        restore: Vec<String>,
        all: bool,
        restart: bool,
        finish: impl FnOnce(&mut App, Result<Value, Failure>) + 'static,
    ) -> Result<(), Failure> {
        let root = crate::plugin::plugin_root()
            .ok_or_else(|| Failure::before("plugin directory unavailable"))?;
        let bundle = tasty_host_plugin::builtin::bundle_root();
        let known = tasty_host_plugin::known_plugins::KnownPlugins::path()
            .ok_or_else(|| Failure::before("known-plugins.toml path unavailable"))?;
        self.plugin_files.enqueue(move |app| {
            let Some(config) = app.plugin_manager.as_ref().map(|mgr| mgr.config.clone()) else {
                finish(app, Err(Failure::before("plugin manager unavailable")));
                return;
            };
            let bytes = crate::app::file_writer::command::value_bytes(&config)
                + restore.iter().map(String::len).sum::<usize>();
            app.run_plugin_file(
                bytes,
                move || {
                    file_upgrade::prepare(root, bundle, known, config, force, &restore, all)
                        .map_err(|e| e.to_string())
                },
                |result| match result {
                    Ok(plan) => {
                        crate::app::file_writer::command::value_bytes(&plan.config)
                            + plan
                                .entries
                                .iter()
                                .map(Entry::retained_bytes)
                                .fold(0usize, usize::saturating_add)
                    }
                    Err(error) => error.len(),
                },
                |result| {
                    let mut disposition = crate::app::plugin_file_lifecycle::classify_result(result);
                    if let Ok(plan) = result {
                        let failed = plan.entries.iter().any(|entry| matches!(entry, Entry::Done(item) if matches!(item.action, BuiltinUpgradeAction::Failed { .. })));
                        if failed {
                            disposition.outcome = crate::app::file_writer::command::CommandOutcome::KnownFailure;
                            disposition.partial = plan.restored;
                        }
                    }
                    disposition
                },
                move |app, result| match result.and_then(|result| result.map_err(Failure::before)) {
                    Err(error) => finish(app, Err(error)),
                    Ok(plan) => {
                        let applied = plan.restored;
                        if let Some(mgr) = app.plugin_manager.as_mut() {
                            mgr.config = plan.config;
                        }
                        app.next_builtin_upgrade(Upgrade {
                            entries: plan.entries.into(),
                            items: Vec::new(),
                            restart,
                            applied,
                            finish: Box::new(finish),
                        });
                    }
                },
            );
        })?;
        (self.services.reply_wake())();
        Ok(())
    }

    fn next_builtin_upgrade(&mut self, mut upgrade: Upgrade) {
        let write = loop {
            match upgrade.entries.pop_front() {
                Some(Entry::Done(item)) => upgrade.items.push(item),
                Some(Entry::Write(write)) => break write,
                None => {
                    let report = BuiltinUpgradeReport {
                        items: upgrade.items,
                    };
                    let result = serde_json::to_value(report).map_err(Failure::before);
                    self.refresh_tool_registry();
                    self.refresh_palette_plugin_commands();
                    self.refresh_plugin_window_after_file();
                    (upgrade.finish)(self, result);
                    return;
                }
            }
        };
        let Some(mgr) = self.plugin_manager.as_mut() else {
            (upgrade.finish)(self, Err(Failure::before("plugin manager unavailable")));
            return;
        };
        let (swap, pending_restart) =
            mgr.begin_file_swap(&write.id, upgrade.restart && write.stops_running());
        upgrade.applied |= swap;
        let id = write.id.clone();
        self.after_plugin_retired(id, move |app| {
            let Some(config) = app.plugin_manager.as_ref().map(|mgr| mgr.config.clone()) else {
                (upgrade.finish)(app, Err(Failure::before("plugin manager unavailable")));
                return;
            };
            app.run_plugin_file(
                write.retained_bytes() + crate::app::file_writer::command::value_bytes(&config),
                move || file_upgrade::execute(write, config),
                |written| {
                    crate::app::file_writer::command::value_bytes(&written.config)
                        + crate::app::file_writer::command::value_bytes(&written.item)
                },
                |written| crate::app::file_writer::command::CommandDisposition {
                    outcome: if written.uncertain {
                        crate::app::file_writer::command::CommandOutcome::Unknown
                    } else if matches!(written.item.action, BuiltinUpgradeAction::Failed { .. }) {
                        crate::app::file_writer::command::CommandOutcome::KnownFailure
                    } else {
                        crate::app::file_writer::command::CommandOutcome::KnownSuccess
                    },
                    partial: written.changed
                        && matches!(written.item.action, BuiltinUpgradeAction::Failed { .. }),
                },
                move |app, result| match result {
                    Err(mut error) => {
                        error.applied |= upgrade.applied;
                        if !error.details.is_object() {
                            error.details = serde_json::json!({});
                        }
                        error.details["partial"] = serde_json::json!(upgrade.applied);
                        error.details["completed_items"] = serde_json::json!(upgrade.items);
                        (upgrade.finish)(app, Err(error));
                    }
                    Ok(written) => {
                        upgrade.applied |= written.changed;
                        upgrade.items.push(app.apply_builtin_written(
                            written,
                            swap,
                            pending_restart,
                        ));
                        app.next_builtin_upgrade(upgrade);
                    }
                },
            );
        });
    }

    fn apply_builtin_written(
        &mut self,
        mut written: Written,
        swap: bool,
        pending_restart: bool,
    ) -> BuiltinUpgradeItem {
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return written.item;
        };
        mgr.config = written.config;
        let validated = written.package.is_some();
        if let Some(package) = written.package {
            mgr.replace_committed_package(&written.item.id, Some(package));
        }
        let (restarted, restart_error) = finish_builtin_restart(
            self.plugin_files.stopping(),
            written.uncertain || !validated,
            swap,
            pending_restart,
            || mgr.finish_file_swap(&written.item.id),
        );
        match &mut written.item.action {
            BuiltinUpgradeAction::Upgraded {
                was_restarted,
                restart_error: error,
                ..
            }
            | BuiltinUpgradeAction::Reinstalled {
                was_restarted,
                restart_error: error,
                ..
            } => {
                *was_restarted = restarted;
                *error = restart_error;
            }
            _ => {
                if let Some(error) = restart_error {
                    tracing::warn!(plugin=%written.item.id,error,"plugin restart after file operation failed");
                }
            }
        }
        let version = match &written.item.action {
            BuiltinUpgradeAction::Upgraded { to, .. } => Some(to.clone()),
            BuiltinUpgradeAction::Reinstalled { version, .. } => Some(version.clone()),
            _ => None,
        };
        if written.changed
            && let Some(version) = version
        {
            self.cascade_plugin_events(vec![CoreEvent::PluginRegistryChanged {
                plugin_id: written.item.id.clone(),
                change: PluginRegistryChange::Installed { version },
            }]);
        }
        written.item
    }
}

fn finish_builtin_restart(
    stopping: bool,
    uncertain: bool,
    swap: bool,
    pending_restart: bool,
    restart: impl FnOnce() -> anyhow::Result<bool>,
) -> (bool, Option<String>) {
    if uncertain || stopping || !(swap || pending_restart) {
        return (false, None);
    }
    match restart() {
        Ok(restarted) => (restarted, None),
        Err(error) => (false, Some(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restart_report_tracks_actual_success_for_closing_swap_and_pending_combinations() {
        // stopping, uncertain, swap, pending restart, actual successful restart expected
        let cases = [
            (false, false, false, false, false),
            (false, false, false, true, true),
            (false, false, true, false, true),
            (false, false, true, true, true),
            (true, false, false, false, false),
            (true, false, false, true, false),
            (true, false, true, false, false),
            (true, false, true, true, false),
            (false, true, false, false, false),
            (false, true, false, true, false),
            (false, true, true, false, false),
            (false, true, true, true, false),
            (true, true, false, false, false),
            (true, true, false, true, false),
            (true, true, true, false, false),
            (true, true, true, true, false),
        ];
        for (stopping, uncertain, swap, pending, expected) in cases {
            let mut calls = 0;
            let (restarted, error) =
                finish_builtin_restart(stopping, uncertain, swap, pending, || {
                    calls += 1;
                    Ok(true)
                });
            assert_eq!(
                calls,
                usize::from(expected),
                "actual call: {stopping}/{uncertain}/{swap}/{pending}"
            );
            assert_eq!(
                restarted, expected,
                "reported restart: {stopping}/{uncertain}/{swap}/{pending}"
            );
            assert!(error.is_none());
        }
    }
    #[test]
    fn a_closing_host_never_claims_a_skipped_swap_restart() {
        let (restarted, error) = finish_builtin_restart(true, false, true, false, || {
            panic!("restart during shutdown")
        });
        assert!(!restarted);
        assert!(error.is_none());
    }
    #[test]
    fn a_successful_noop_is_not_reported_as_a_restart() {
        assert_eq!(
            finish_builtin_restart(false, false, true, false, || Ok(false)),
            (false, None)
        );
    }
    #[test]
    fn a_failed_actual_restart_is_not_reported_successful() {
        for (swap, pending) in [(true, false), (false, true), (true, true)] {
            let mut calls = 0;
            let (restarted, error) = finish_builtin_restart(false, false, swap, pending, || {
                calls += 1;
                anyhow::bail!("owned restart failure")
            });
            assert_eq!(calls, 1);
            assert!(!restarted);
            assert_eq!(error.as_deref(), Some("owned restart failure"));
        }
    }
}
