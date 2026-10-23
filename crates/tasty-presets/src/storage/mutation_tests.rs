use super::tests::ws;
use super::*;

fn seeded() -> (tempfile::TempDir, PresetStore) {
    let root = tempfile::tempdir().unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    store.save_workspace(ws("old")).unwrap();
    (root, store)
}

#[test]
fn rename_destination_failure_preserves_source_and_cache() {
    let (root, mut store) = seeded();
    let source = root.path().join("workspace/old.toml");
    let original = std::fs::read(&source).unwrap();
    std::fs::create_dir(root.path().join("workspace/new.toml.lock")).unwrap();
    let outcome = store.rename_command(PresetKind::Workspace, "old", "new");
    assert!(outcome.result.is_err());
    assert!(outcome.changes.is_empty());
    assert_eq!(store.list(PresetKind::Workspace), ["old"]);
    assert_eq!(std::fs::read(source).unwrap(), original);
    assert!(!root.path().join("workspace/new.toml").exists());
}

#[test]
fn rename_serialization_failure_does_not_remove_the_original_cached_value() {
    let (root, mut store) = seeded();
    let path = root.path().join("workspace/old.toml");
    let original = std::fs::read(&path).unwrap();
    let mut value = store.get_workspace("old").unwrap().clone();
    let crate::PresetPaneNode::Leaf { pane } = &mut value.layout else {
        panic!("leaf fixture")
    };
    let crate::PresetSurfaceLayout::Leaf { surface } = &mut pane.tabs[0].layout else {
        panic!("leaf fixture")
    };
    surface.params = serde_json::json!({"unrepresentable":u64::MAX});
    store.apply_change(PresetChange::Saved(OwnedPreset::Workspace(value.clone())));
    let outcome = store.rename_command(PresetKind::Workspace, "old", "new");
    assert!(matches!(outcome.result, Err(PresetError::Toml(_))));
    assert!(outcome.changes.is_empty());
    assert_eq!(store.get_workspace("old"), Some(&value));
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(!root.path().join("workspace/new.toml").exists());
}

#[test]
fn cleanup_failure_reports_partial_commit_and_both_caches_match_restart() {
    let (root, mut worker) = seeded();
    let mut cache = worker.clone();
    let result = worker.rename_with_remove(PresetKind::Workspace, "old", "new", |_| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "injected source cleanup failure",
        ))
    });
    assert!(matches!(result, Err(PresetError::RenamePartial { .. })));
    let outcome = worker.rename_outcome(PresetKind::Workspace, "old", "new", result);
    assert_eq!(outcome.changes.len(), 1);
    assert_eq!(cache.list(PresetKind::Workspace), ["old"]);
    for change in outcome.changes {
        cache.apply_change(change);
    }
    let reopened = PresetStore::load_from(root.path().into());
    assert_eq!(reopened.list(PresetKind::Workspace), ["new", "old"]);
    for name in ["new", "old"] {
        assert_eq!(worker.get_workspace(name), reopened.get_workspace(name));
        assert_eq!(cache.get_workspace(name), reopened.get_workspace(name));
    }
    assert!(
        outcome
            .result
            .unwrap_err()
            .to_string()
            .contains("destination saved")
    );
}

#[test]
fn external_source_edit_and_destination_creation_are_never_overwritten() {
    let (root, mut store) = seeded();
    let source = root.path().join("workspace/old.toml");
    std::fs::write(&source, "# external source\n").unwrap();
    let outcome = store.rename_command(PresetKind::Workspace, "old", "new");
    assert!(outcome.result.is_err() && outcome.changes.is_empty());
    assert!(!root.path().join("workspace/new.toml").exists());
    assert!(
        store
            .delete_command(PresetKind::Workspace, "old")
            .result
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(source).unwrap(),
        "# external source\n"
    );
    let target = root.path().join("workspace/new.toml");
    std::fs::write(&target, "# external destination\n").unwrap();
    assert!(matches!(
        store.rename(PresetKind::Workspace, "old", "new"),
        Err(PresetError::AlreadyExists(_, _))
    ));
    assert_eq!(
        std::fs::read_to_string(target).unwrap(),
        "# external destination\n"
    );
    assert_eq!(store.list(PresetKind::Workspace), ["old"]);
}

#[test]
fn delete_missing_is_achieved_but_unreadable_original_is_preserved() {
    let (root, mut store) = seeded();
    let path = root.path().join("workspace/old.toml");
    std::fs::remove_file(&path).unwrap();
    let outcome = store.delete_command(PresetKind::Workspace, "old");
    assert!(outcome.result.is_ok());
    assert!(matches!(
        outcome.changes.as_slice(),
        [PresetChange::Removed { .. }]
    ));
    assert!(store.get_workspace("old").is_none());
    store.save_workspace(ws("old")).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep"), "original").unwrap();
    let outcome = store.delete_command(PresetKind::Workspace, "old");
    assert!(outcome.result.is_err() && outcome.changes.is_empty());
    assert!(store.get_workspace("old").is_some());
    assert_eq!(
        std::fs::read_to_string(path.join("keep")).unwrap(),
        "original"
    );
}

#[test]
fn save_rename_delete_changes_follow_order_and_match_restart() {
    let (root, mut worker) = seeded();
    let mut cache = worker.clone();
    let saved = worker
        .save_command("next", None, false, OwnedPreset::Workspace(ws("ignored")))
        .unwrap()
        .unwrap();
    cache.apply_change(saved);
    let rename = worker.rename_command(PresetKind::Workspace, "next", "final");
    assert!(rename.result.is_ok());
    for change in rename.changes {
        cache.apply_change(change);
    }
    let deleted = worker.delete_command(PresetKind::Workspace, "final");
    assert!(deleted.result.is_ok());
    for change in deleted.changes {
        cache.apply_change(change);
    }
    let reopened = PresetStore::load_from(root.path().into());
    assert_eq!(cache.list(PresetKind::Workspace), ["old"]);
    assert_eq!(cache.get_workspace("old"), reopened.get_workspace("old"));
    assert_eq!(
        worker.list(PresetKind::Workspace),
        reopened.list(PresetKind::Workspace)
    );
}

#[cfg(unix)]
#[test]
fn deleting_a_preset_symlink_keeps_its_target_and_retarget_conflicts() {
    use std::os::unix::fs::symlink;
    let (root, _) = seeded();
    let dir = root.path().join("workspace");
    symlink("old.toml", dir.join("alias.toml")).unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    store.delete(PresetKind::Workspace, "alias").unwrap();
    assert!(dir.join("old.toml").exists());
    assert!(!dir.join("alias.toml").exists());
    symlink("old.toml", dir.join("alias.toml")).unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    std::fs::write(
        dir.join("other.toml"),
        std::fs::read(dir.join("old.toml")).unwrap(),
    )
    .unwrap();
    std::fs::remove_file(dir.join("alias.toml")).unwrap();
    symlink("other.toml", dir.join("alias.toml")).unwrap();
    assert!(store.delete(PresetKind::Workspace, "alias").is_err());
    assert!(
        std::fs::symlink_metadata(dir.join("alias.toml"))
            .unwrap()
            .is_symlink()
    );
    assert!(dir.join("other.toml").exists() && dir.join("old.toml").exists());
}

#[cfg(unix)]
#[test]
fn lock_namespace_alias_is_rejected_before_migration_or_replacement() {
    use std::os::unix::fs::symlink;
    let (root, _) = seeded();
    // The fixture is valid TOML with missing IDs so an unsafe load would also rewrite the lock.
    let raw = toml::to_string_pretty(&ws("alias")).unwrap();
    let lock = root.path().join(".preset-mutations.lock");
    std::fs::write(&lock, &raw).unwrap();
    symlink(
        "../.preset-mutations.lock",
        root.path().join("workspace/alias.toml"),
    )
    .unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    assert!(
        store.get_workspace("alias").is_none(),
        "reject before invoking any migration write"
    );
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), raw);
    assert!(
        store
            .save_command("", Some("alias"), true, OwnedPreset::Workspace(ws("alias")))
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), raw);
}

#[cfg(unix)]
#[test]
fn occupied_or_redirected_root_lock_rejects_without_touching_a_preset() {
    use std::os::unix::fs::symlink;
    let (root, mut store) = seeded();
    let source = root.path().join("workspace/old.toml");
    let original = std::fs::read(&source).unwrap();
    let anchor = root.path().join(".preset-mutations");
    symlink("workspace/old.toml", &anchor).unwrap();
    assert!(
        store.mutation_lock().is_err(),
        "reject before acquiring the target's file lock"
    );
    assert!(store.rename(PresetKind::Workspace, "old", "new").is_err());
    std::fs::remove_file(anchor).unwrap();
    let lock = root.path().join(".preset-mutations.lock");
    std::fs::remove_file(&lock).unwrap();
    symlink("workspace/old.toml", lock).unwrap();
    assert!(store.mutation_lock().is_err());
    assert!(store.delete(PresetKind::Workspace, "old").is_err());
    assert_eq!(std::fs::read(source).unwrap(), original);
    assert_eq!(store.list(PresetKind::Workspace), ["old"]);
}

#[cfg(target_os = "linux")]
#[test]
fn rename_preserves_source_private_mode_owner_and_extended_acl() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::process::Command;
    let (root, mut store) = seeded();
    let path = root.path().join("workspace/old.toml");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let set = Command::new("setfacl")
        .args(["-m", "u:65534:---"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(set.status.success(), "{set:?}");
    let acl = |path: &Path| {
        let output = Command::new("getfacl")
            .args(["-cp"])
            .arg(path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        output.stdout
    };
    let before = std::fs::metadata(&path).unwrap();
    let expected = acl(&path);
    assert!(
        String::from_utf8_lossy(&expected).contains("65534")
            || String::from_utf8_lossy(&expected).contains("nobody")
    );
    store.rename(PresetKind::Workspace, "old", "new").unwrap();
    let target = root.path().join("workspace/new.toml");
    let after = std::fs::metadata(&target).unwrap();
    assert_eq!(
        (after.uid(), after.gid(), after.permissions().mode() & 0o777),
        (before.uid(), before.gid(), 0o600)
    );
    assert_eq!(acl(&target), expected);
    assert!(!path.exists());
}
