use super::tests::ws;
use super::*;

fn value(name: &str, subtitle: &str) -> OwnedPreset {
    let mut preset = ws(name);
    preset.subtitle = subtitle.into();
    OwnedPreset::Workspace(preset)
}

#[test]
fn independent_stores_choose_names_inside_the_participant_boundary() {
    let root = tempfile::tempdir().unwrap();
    let mut first = PresetStore::load_from(root.path().into());
    let mut second = first.clone();
    let first = first
        .save_command("dev", None, false, value("ignored", "one"))
        .unwrap();
    let second = second
        .save_command("dev", None, false, value("ignored", "two"))
        .unwrap();
    assert!(
        matches!(first, Some(PresetChange::Saved(OwnedPreset::Workspace(p))) if p.name == "dev")
    );
    assert!(
        matches!(second, Some(PresetChange::Saved(OwnedPreset::Workspace(p))) if p.name == "dev-2")
    );
    let reopened = PresetStore::load_from(root.path().into());
    assert_eq!(reopened.list(PresetKind::Workspace), ["dev", "dev-2"]);
    assert_eq!(reopened.get_workspace("dev").unwrap().subtitle, "one");
    assert_eq!(reopened.get_workspace("dev-2").unwrap().subtitle, "two");
}

#[test]
fn compatibility_rename_delete_and_save_keep_the_observation_current() {
    let root = tempfile::tempdir().unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    store.save_workspace(ws("dev")).unwrap();
    store
        .rename(PresetKind::Workspace, "dev", "renamed")
        .unwrap();
    store
        .save_command("", Some("renamed"), true, value("renamed", "updated"))
        .unwrap();
    store.save_workspace(ws("dev")).unwrap();
    store.delete(PresetKind::Workspace, "dev").unwrap();
    store.save_workspace(ws("dev")).unwrap();
    let reopened = PresetStore::load_from(root.path().into());
    assert_eq!(reopened.list(PresetKind::Workspace), ["dev", "renamed"]);
    assert_eq!(
        reopened.get_workspace("renamed").unwrap().subtitle,
        "updated"
    );
}

#[test]
fn delayed_worker_keeps_cache_reads_available_and_publishes_only_a_committed_delta() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = PresetStore::load_from(root.path().into());
    cache.save_workspace(ws("dev")).unwrap();
    let mut worker = cache.clone();
    let path = root.path().join("workspace/dev.toml");
    let before = std::fs::read(&path).unwrap();
    let held = tasty_utils::file_replace::lock(&path).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        tx.send(worker.save_command("", Some("dev"), true, value("dev", "committed")))
            .unwrap();
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(30))
            .is_err()
    );
    assert_eq!(cache.get_workspace("dev").unwrap().subtitle, "");
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(held);
    let change = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap()
        .unwrap()
        .unwrap();
    thread.join().unwrap();
    assert_eq!(cache.get_workspace("dev").unwrap().subtitle, "");
    let disk = PresetStore::load_from(root.path().into());
    assert_eq!(disk.get_workspace("dev").unwrap().subtitle, "committed");
    cache.apply_change(change).unwrap();
    assert_eq!(cache.get_workspace("dev"), disk.get_workspace("dev"));
}

#[test]
fn external_edit_and_failed_replacement_never_publish_new_values() {
    let root = tempfile::tempdir().unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    store.save_workspace(ws("dev")).unwrap();
    let path = root.path().join("workspace/dev.toml");
    std::fs::write(&path, "# external edit\n").unwrap();
    let error = store
        .save_command("", Some("dev"), true, value("dev", "discard"))
        .unwrap_err();
    assert!(error.to_string().contains("changed outside"));
    assert_eq!(store.get_workspace("dev").unwrap().subtitle, "");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# external edit\n");

    let blocked = root.path().join("workspace/new.toml.lock");
    std::fs::create_dir(blocked).unwrap();
    assert!(
        store
            .save_command("", Some("new"), false, value("new", "discard"))
            .is_err()
    );
    assert!(store.get_workspace("new").is_none());
    assert!(!root.path().join("workspace/new.toml").exists());
}

#[test]
fn unreadable_and_damaged_originals_are_protected_and_reserve_their_names() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("workspace");
    std::fs::create_dir_all(dir.join("unreadable.toml")).unwrap();
    std::fs::write(dir.join("unreadable.toml/keep"), "original").unwrap();
    let damaged = "private='PRESET_PRIVATE_SENTINEL'\n[[broken\n";
    std::fs::write(dir.join("dev.toml"), damaged).unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    let error = store
        .save_command("", Some("dev"), true, value("dev", "discard"))
        .unwrap_err();
    assert!(!error.to_string().contains("PRESET_PRIVATE_SENTINEL"));
    assert!(
        store
            .save_command("", Some("unreadable"), true, value("unreadable", "discard"))
            .is_err()
    );
    assert!(
        store
            .save_command("", Some("dev"), false, value("dev", "discard"))
            .unwrap()
            .is_none()
    );
    let change = store
        .save_command("dev", None, false, value("ignored", "safe"))
        .unwrap();
    assert!(
        matches!(change, Some(PresetChange::Saved(OwnedPreset::Workspace(p))) if p.name == "dev-2")
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("dev.toml")).unwrap(),
        damaged
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("unreadable.toml/keep")).unwrap(),
        "original"
    );
}

#[cfg(unix)]
#[test]
fn save_preserves_link_and_permissions_and_rejects_a_retargeted_link() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir().unwrap();
    let mut initial = PresetStore::load_from(root.path().into());
    initial.save_workspace(ws("target")).unwrap();
    let dir = root.path().join("workspace");
    let target = dir.join("target.toml");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    symlink("target.toml", dir.join("alias.toml")).unwrap();
    let mut store = PresetStore::load_from(root.path().into());
    store
        .save_command("", Some("alias"), true, value("alias", "saved"))
        .unwrap();
    assert!(
        std::fs::symlink_metadata(dir.join("alias.toml"))
            .unwrap()
            .is_symlink()
    );
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let saved = std::fs::read(&target).unwrap();
    std::fs::write(dir.join("other.toml"), &saved).unwrap();
    std::fs::remove_file(dir.join("alias.toml")).unwrap();
    symlink("other.toml", dir.join("alias.toml")).unwrap();
    assert!(
        store
            .save_command("", Some("alias"), true, value("alias", "discard"))
            .is_err()
    );
    assert_eq!(std::fs::read(&target).unwrap(), saved);
    assert_eq!(std::fs::read(dir.join("other.toml")).unwrap(), saved);
}
