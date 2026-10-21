use super::*;
use crate::view::file_chooser::FileChooserOutcome;

#[test]
fn selected_plugin_folder_runs_existing_validation_and_cancel_preserves_input() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = PluginsUiState::default();
    let snapshot = PluginsSnapshot::default();
    state.add_path_input = "unchanged".into();
    apply_folder_outcome(&mut state, &snapshot, FileChooserOutcome::Cancelled);
    assert_eq!(state.add_path_input, "unchanged");
    let path = directory.path().to_string_lossy().into_owned();
    apply_folder_outcome(
        &mut state,
        &snapshot,
        FileChooserOutcome::Confirmed(vec![path.clone()]),
    );
    assert_eq!(state.add_path_input, path);
    assert!(matches!(state.add_error, Some(AddError::Read(_))));
    assert!(state.add_preview.is_none());
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/tasty-plugin-image/tasty-plugin.toml"),
        directory.path().join("tasty-plugin.toml"),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("tasty-plugin-image"),
        b"validation fixture, never executed",
    )
    .unwrap();
    std::fs::write(
        directory.path().join("tasty-plugin-image.exe"),
        b"validation fixture, never executed",
    )
    .unwrap();
    apply_folder_outcome(
        &mut state,
        &snapshot,
        FileChooserOutcome::Confirmed(vec![path]),
    );
    assert!(state.add_error.is_none(), "{:?}", state.add_error);
    assert_eq!(state.add_preview.as_ref().unwrap().id, "com.tasty.image");
}
