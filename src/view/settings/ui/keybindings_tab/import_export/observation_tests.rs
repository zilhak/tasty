use super::*;
use crate::app::settings_files::{SettingsFileRequest, SettingsFileResult, export::ExportError};
fn start(state: &mut ImportExportState) -> std::sync::Weak<()> {
    state.export_to(
        Path::new("owned.toml"),
        &KeybindingSettings::default(),
        &PluginBundleContext::default(),
        &PluginShortcutDraft::new(),
    );
    let SettingsFileRequest::Export { observer, .. } = state.take_file_requests().pop().unwrap()
    else {
        panic!("export")
    };
    observer
}
#[test]
fn unknown_completion_is_distinct_and_never_queues_a_replay() {
    let mut state = ImportExportState::default();
    let observer = start(&mut state);
    state.accept_file_result(SettingsFileResult::ExportTransport {
        observer,
        error: ExportError::Unknown,
    });
    assert!(matches!(
        state.export_failure.as_ref().unwrap().reason,
        ExportFailReason::OutcomeUnknown
    ));
    assert!(state.toast.is_none());
    assert!(state.take_file_requests().is_empty());
}
#[test]
fn old_completion_cannot_finish_new_request_or_reopened_form() {
    let mut state = ImportExportState::default();
    let old = start(&mut state);
    state.export_observation.observe("a different draft");
    let new = start(&mut state);
    state.accept_file_result(SettingsFileResult::Export {
        path: "old.toml".into(),
        observer: old.clone(),
        result: Ok(()),
    });
    assert!(state.toast.is_none());
    assert!(state.export_observation.pending());
    state.accept_file_result(SettingsFileResult::Export {
        path: "new.toml".into(),
        observer: new,
        result: Ok(()),
    });
    assert!(state.take_toast().unwrap().contains("owned.toml"));
    drop(state);
    let mut reopened = ImportExportState::default();
    reopened.accept_file_result(SettingsFileResult::Export {
        path: "old.toml".into(),
        observer: old,
        result: Err(ExportError::Io(std::io::ErrorKind::PermissionDenied.into())),
    });
    assert!(reopened.export_failure.is_none());
}
#[test]
fn pending_click_is_not_duplicated_and_explicit_retry_encodes_the_latest_draft() {
    let mut state = ImportExportState::default();
    let observer = start(&mut state);
    state.export_to(
        Path::new("owned.toml"),
        &KeybindingSettings::default(),
        &PluginBundleContext::default(),
        &PluginShortcutDraft::new(),
    );
    assert!(state.take_file_requests().is_empty());
    state.accept_file_result(SettingsFileResult::Export {
        path: "owned.toml".into(),
        observer,
        result: Err(ExportError::Io(std::io::ErrorKind::PermissionDenied.into())),
    });
    let mut keys = KeybindingSettings::default();
    keys.toggle_settings.clear();
    state.export_to(
        Path::new("owned.toml"),
        &keys,
        &PluginBundleContext::default(),
        &PluginShortcutDraft::new(),
    );
    let requests = state.take_file_requests();
    assert_eq!(requests.len(), 1);
    let SettingsFileRequest::Export { text, .. } = &requests[0] else {
        panic!("retry")
    };
    assert_eq!(
        *text,
        encode(
            &keys,
            &merged_overrides(&Default::default(), &PluginShortcutDraft::new())
        )
        .unwrap()
    );
    assert!(state.toast.is_none());
}
