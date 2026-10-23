//! Real files and locks exercise the production transaction.
use super::*;
use std::{sync::mpsc, time::Duration};
fn entry(id: u32, index: u32) -> super::super::ChildEntry {
    super::super::ChildEntry {
        child_surface_id: id,
        index,
        cwd: None,
        role: None,
        nickname: None,
    }
}
fn registry(path: &Path) -> RegistryData {
    let mut value = super::super::read_file(path);
    value.path = Some(path.to_owned());
    value
}
fn merged(path: &Path, id: u32) -> Command {
    let mut value = registry(path);
    value.register_child(ParentKey::External("codex:shared".into()), entry(id, id));
    Command::new(path.to_owned(), value, Operation::Merge)
}
#[test]
fn delayed_participant_preserves_two_engines_and_latest_external_edit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let first = merged(&path, 101);
    let second = merged(&path, 202);
    let lock = file_replace::lock(&path).unwrap();
    let (send, receive) = mpsc::channel();
    let worker = std::thread::spawn(move || send.send(first.execute()).unwrap());
    assert!(receive.recv_timeout(Duration::from_millis(40)).is_err());
    let mut cache = registry(&path);
    cache.register_child(88, entry(89, 0));
    let mut external = RegistryData::default();
    external.register_child(9, entry(10, 0));
    file_replace::replace(&path, &serde_json::to_vec(&external).unwrap()).unwrap();
    drop(lock);
    receive
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    second.execute().unwrap();
    let disk = read(&path).unwrap();
    assert_eq!(disk.parent_of_child(10), Some(ParentKey::Surface(9)));
    assert_eq!(
        disk.list_children(&ParentKey::External("codex:shared".into()))
            .len(),
        2
    );
    assert_eq!(cache.list_children(&ParentKey::Surface(88)).len(), 1);
}
#[test]
fn corrupt_and_unreadable_initial_loads_protect_original_even_after_repair() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    std::fs::write(&path, b"broken secret body").unwrap();
    let protected = merged(&path, 101);
    let error = protected.execute().err().unwrap();
    assert!(!error.contains("secret body"));
    assert_eq!(std::fs::read(&path).unwrap(), b"broken secret body");
    let protected = merged(&path, 101);
    let repaired = serde_json::to_vec(&RegistryData::default()).unwrap();
    std::fs::write(&path, &repaired).unwrap();
    assert!(
        read(&path).is_ok(),
        "the external repair must be a valid registry"
    );
    let error = protected.execute().err().unwrap();
    assert!(error.contains("original load failed"));
    assert_eq!(std::fs::read(&path).unwrap(), repaired);
    let reloaded = merged(&path, 303).execute().unwrap();
    assert!(reloaded.registry.parent_of_child(303).is_some());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(merged(&path, 101).execute().is_err());
    assert!(path.is_dir());
}
#[test]
fn failed_reservation_returns_no_index_and_does_not_advance_cache() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let mut cache = registry(&path);
    std::fs::write(&path, b"bad json").unwrap();
    assert!(
        cache
            .reserve_index(&ParentKey::External("codex:shared".into()))
            .is_err()
    );
    assert!(cache.external_next_index.is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), b"bad json");
}
#[test]
fn two_processes_reserve_unique_indices_and_keep_both_records() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let mut children = Vec::new();
    for id in [101, 202] {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    &format!(
                        "{}::registry_subprocess",
                        module_path!().split_once("::").unwrap().1
                    ),
                    "--nocapture",
                ])
                .env("TASTY_CHILD_REGISTRY_TEST_PATH", &path)
                .env("TASTY_CHILD_REGISTRY_TEST_ID", id.to_string())
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let disk = read(&path).unwrap();
    let mut indices: Vec<_> = disk
        .list_children(&ParentKey::External("codex:shared".into()))
        .iter()
        .map(|entry| entry.index)
        .collect();
    indices.sort();
    assert_eq!(indices, [0, 1]);
    assert_eq!(disk.external_next_index.get("codex:shared"), Some(&2));
}
#[test]
fn registry_subprocess() {
    let Some(path) = std::env::var_os("TASTY_CHILD_REGISTRY_TEST_PATH") else {
        return;
    };
    let path = PathBuf::from(path);
    let id = std::env::var("TASTY_CHILD_REGISTRY_TEST_ID")
        .unwrap()
        .parse()
        .unwrap();
    let mut cache = registry(&path);
    let parent = ParentKey::External("codex:shared".into());
    let index = cache.reserve_index(&parent).unwrap();
    cache.register_child(parent, entry(id, index));
    cache.persist(Operation::Merge).unwrap();
    assert!(read(&path).unwrap().parent_of_child(id).is_some());
}
#[cfg(unix)]
#[test]
fn atomic_replace_preserves_private_mode_and_symlink() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("actual.json");
    let path = root.path().join("children.json");
    std::fs::write(
        &target,
        serde_json::to_vec(&RegistryData::default()).unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, &path).unwrap();
    merged(&path, 101).execute().unwrap();
    assert!(std::fs::symlink_metadata(&path).unwrap().is_symlink());
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(read(&target).unwrap().parent_of_child(101).is_some());
}

#[cfg(unix)]
#[test]
fn readonly_original_is_not_replaced_and_failed_output_is_not_published() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let original = serde_json::to_vec(&RegistryData::default()).unwrap();
    std::fs::write(&path, &original).unwrap();
    let command = merged(&path, 101);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    assert!(command.execute().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[cfg(feature = "gui")]
#[test]
fn lifecycle_clear_protects_corrupt_file_and_release_survives_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let mut first = registry(&path);
    first.bind_slot(Some(1));
    first.register_child(9, entry(10, 0));
    first.persist(Operation::Merge).unwrap();
    let mut second = registry(&path);
    second.bind_slot(Some(2));
    second.register_child(19, entry(20, 0));
    second.persist(Operation::Merge).unwrap();
    first.release_owned();
    let reopened = read(&path).unwrap();
    assert!(reopened.parent_of_child(10).is_none());
    assert_eq!(reopened.parent_of_child(20), Some(ParentKey::Surface(19)));
    std::fs::write(&path, b"corrupt").unwrap();
    let value = registry(&path);
    assert!(
        Command::new(path.clone(), value, Operation::Clear)
            .execute()
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
}

#[test]
fn asynchronous_stale_snapshot_must_not_rewind_a_committed_reservation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let mut live = registry(&path);
    let parent = ParentKey::Surface(10);
    let first = live.reserve_index(&parent).unwrap();
    live.register_child(parent.clone(), entry(100, first));
    live.persist(Operation::Merge).unwrap();
    let reserve = Command::new(path.clone(), live.clone(), Operation::Reserve(parent));
    live.set_idle(100, true);
    let queued_save = Command::new(path.clone(), live, Operation::Merge);
    assert_eq!(reserve.execute().unwrap().index, Some(1));
    assert_eq!(read(&path).unwrap().next_index.get(&10), Some(&2));
    queued_save.execute().unwrap();
    assert_eq!(
        read(&path).unwrap().next_index.get(&10),
        Some(&2),
        "a queued stale owned snapshot must not rewind the committed next index"
    );
}

#[test]
fn queued_create_then_delete_retains_the_submitted_delete_intent() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let mut live = registry(&path);
    live.register_child(10, entry(100, 0));
    let create = Command::new(path.clone(), live.clone(), Operation::Merge);
    // Admission advances the submitted baseline, not the durable-success observation.
    live.checkpoint();
    assert!(live.unregister_child_by_surface(100));
    let delete = Command::new(path.clone(), live, Operation::Merge);
    create.execute().unwrap();
    assert!(read(&path).unwrap().parent_of_child(100).is_some());
    delete.execute().unwrap();
    assert!(read(&path).unwrap().parent_of_child(100).is_none());
}

#[test]
fn queued_toggle_back_is_applied_after_the_first_change() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let mut live = registry(&path);
    live.register_child(10, entry(100, 0));
    live.set_idle(100, false);
    live.persist(Operation::Merge).unwrap();
    live.set_idle(100, true);
    let first = Command::new(path.clone(), live.clone(), Operation::Merge);
    live.checkpoint();
    live.set_idle(100, false);
    let second = Command::new(path.clone(), live, Operation::Merge);
    first.execute().unwrap();
    assert_eq!(read(&path).unwrap().state_of(100), "idle");
    second.execute().unwrap();
    assert_eq!(read(&path).unwrap().state_of(100), "active");
}

#[test]
fn unrelated_owned_fields_preserve_a_latest_external_edit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let mut live = registry(&path);
    live.register_child(10, entry(100, 0));
    live.persist(Operation::Merge).unwrap();
    live.update_child(&10.into(), 0, |c| c.cwd = Some("new cwd".into()));
    let command = Command::new(path.clone(), live, Operation::Merge);
    let mut external = registry(&path);
    external.update_child(&10.into(), 0, |c| {
        c.nickname = Some("external nickname".into())
    });
    external.persist(Operation::Merge).unwrap();
    command.execute().unwrap();
    let latest = read(&path).unwrap();
    let child = latest.find_child(&10.into(), 0).unwrap();
    assert_eq!(child.cwd.as_deref(), Some("new cwd"));
    assert_eq!(child.nickname.as_deref(), Some("external nickname"));
}

#[test]
fn deletion_does_not_erase_a_pending_external_reservation_already_in_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("children.json");
    let parent = ParentKey::External("codex:shared".into());
    let mut live = registry(&path);
    let index = live.reserve_index(&parent).unwrap();
    live.register_child(parent.clone(), entry(100, index));
    live.persist(Operation::Merge).unwrap();
    let mut other = registry(&path);
    assert_eq!(other.reserve_index(&parent), Ok(1));
    // Refresh includes the other engine's next counter before removing the last child.
    live.set_idle(100, true);
    live.persist(Operation::Merge).unwrap();
    assert_eq!(live.external_next_index.get("codex:shared"), Some(&2));
    live.unregister_child_by_surface(100);
    live.persist(Operation::Merge).unwrap();
    assert_eq!(
        read(&path).unwrap().external_next_index.get("codex:shared"),
        Some(&2)
    );
    assert_eq!(live.reserve_index(&parent), Ok(2));
}
