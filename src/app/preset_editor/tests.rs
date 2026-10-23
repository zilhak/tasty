use super::*;
use crate::app::preset_commands::tests::value;
fn workspace(subtitle: &str) -> tasty_presets::WorkspacePreset {
    let tasty_presets::OwnedPreset::Workspace(value) = value(subtitle) else {
        unreachable!()
    };
    value
}
#[test]
fn queued_field_merge_preserves_independent_worker_change_and_rejects_same_field_change() {
    let root = tempfile::tempdir().unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    let mut before = workspace("before");
    before.name = "dev".into();
    store.save_workspace(before.clone()).unwrap();
    let mut live = before.clone();
    live.description = "independent description".into();
    store.save_workspace_overwrite(live).unwrap();
    let mut after = before.clone();
    after.subtitle = "changed subtitle".into();
    let result = apply(
        &mut store,
        PresetEdit::Save {
            before: Some(PresetValue::Workspace(before.clone())),
            after: PresetValue::Workspace(after.clone()),
        },
    );
    result.result.unwrap();
    let saved = store.get_workspace("dev").unwrap();
    assert_eq!(saved.subtitle, "changed subtitle");
    assert_eq!(saved.description, "independent description");
    after.subtitle = "stale overwrite".into();
    let result = apply(
        &mut store,
        PresetEdit::Save {
            before: Some(PresetValue::Workspace(before)),
            after: PresetValue::Workspace(after),
        },
    );
    assert!(matches!(
        result.result,
        Err(PresetMutationError::Conflict(_))
    ));
    assert!(result.changes.is_empty());
    assert_eq!(
        store.get_workspace("dev").unwrap().subtitle,
        "changed subtitle"
    );
}
#[test]
fn batch_failure_keeps_committed_prefix_and_does_not_execute_later_edits() {
    let root = tempfile::tempdir().unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    let mut first = workspace("one");
    first.name = "first".into();
    let mut last = workspace("three");
    last.name = "last".into();
    let output = run_edits(
        &mut store,
        vec![
            PresetEdit::Save {
                before: None,
                after: PresetValue::Workspace(first.clone()),
            },
            PresetEdit::Save {
                before: None,
                after: PresetValue::Workspace(first),
            },
            PresetEdit::Save {
                before: None,
                after: PresetValue::Workspace(last),
            },
        ],
    );
    let report = output.result.unwrap();
    assert_eq!(report.applied, 1);
    assert!(report.error.is_some());
    assert!(!output.saved);
    assert!(output.partial);
    assert!(report.partial);
    assert_eq!(output.completed_edits, 1);
    assert_eq!(output.changes.len(), 1);
    assert_eq!(store.list(PresetKind::Workspace), ["first"]);
    assert!(root.path().join("workspace/first.toml").is_file());
    assert!(!root.path().join("workspace/last.toml").exists());
}
