use super::*;
use std::fs;

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("webhooks.toml");
    (dir, path)
}
fn unlocked(path: &Path) {
    let target = file_replace::resolve_link(path).unwrap();
    let mut name = target.as_os_str().to_owned();
    name.push(".lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PathBuf::from(name))
        .unwrap();
    assert!(
        file.try_lock().is_ok(),
        "section result must release its lock before caller actions"
    );
}
fn secret_doc() -> &'static str {
    "format=2\nport=40000\n[listener]\nkeep=true\n[[webhook]]\nid='fixture'\n[webhook.auth]\ntoken='isolated-auth-payload'\n"
}

#[test]
fn actual_port_changes_preserve_host_auth_and_foreign_sections() {
    let (_dir, path) = fixture();
    fs::write(&path, secret_doc()).unwrap();
    set_port(&path, 41234).unwrap();
    unlocked(&path);
    let table = read_table(&path).unwrap();
    assert_eq!(table["port"].as_integer(), Some(41234));
    assert!(table["webhook"][0]["auth"]["token"].as_str() == Some("isolated-auth-payload"));
    assert_eq!(table["listener"]["keep"].as_bool(), Some(true));
    clear_port(&path).unwrap();
    unlocked(&path);
    let table = read_table(&path).unwrap();
    assert!(!table.contains_key("port"));
    assert!(table["webhook"][0]["auth"]["token"].as_str() == Some("isolated-auth-payload"));
}

#[test]
fn invalid_input_keeps_original_and_never_reports_its_payload() {
    for bytes in [
        b"auth='isolated-auth-payload'\nbroken=[".as_slice(),
        &[0xff, 0xfe],
    ] {
        let (_dir, path) = fixture();
        fs::write(&path, bytes).unwrap();
        for result in [
            set_port(&path, 41000),
            clear_port(&path),
            replace_webhooks(&path, vec![]),
        ] {
            let error = result.unwrap_err().to_string();
            assert!(!error.contains("isolated-auth-payload"));
            assert!(fs::read(&path).unwrap() == bytes);
            unlocked(&path);
        }
        assert_eq!(read_port(&path), None);
        assert!(fs::read(&path).unwrap() == bytes);
        unlocked(&path);
    }
}

#[test]
fn editor_before_locked_read_is_merged_and_editor_after_read_is_preserved() {
    let (_dir, path) = fixture();
    fs::write(&path, secret_doc()).unwrap();
    // Represents an owned queued section request: no full-file snapshot is retained.
    let queued_section = vec![toml::Value::Table(toml::Table::new())];
    fs::write(&path, "format=2\nport=41000\nexternal='new'\n").unwrap();
    replace_webhooks(&path, queued_section).unwrap();
    let doc = read_table(&path).unwrap();
    assert_eq!(doc["port"].as_integer(), Some(41000));
    assert!(doc["external"].as_str() == Some("new"));
    let mut file = LockedTable::open(&path).unwrap();
    file.table
        .insert("port".into(), toml::Value::Integer(42000));
    let edited = "format=2\nport=43000\nexternal='keep this edit'\n";
    fs::write(&path, edited).unwrap();
    assert!(file.commit().is_err());
    assert!(fs::read_to_string(&path).unwrap() == edited);
    unlocked(&path);
}

#[cfg(unix)]
#[test]
fn secure_replace_preserves_existing_mode_and_uses_private_mode_for_new_files() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = fixture();
    set_port(&path, 41000).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    replace_webhooks(&path, vec![toml::Value::Table(toml::Table::new())]).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    unlocked(&path);
}

#[cfg(unix)]
#[test]
fn symlink_target_is_shared_and_retargeted_or_dangling_links_are_not_replaced() {
    use std::os::unix::fs::symlink;
    let (dir, path) = fixture();
    let target = dir.path().join("target.toml");
    let other = dir.path().join("other.toml");
    fs::write(&target, secret_doc()).unwrap();
    fs::write(&other, "format=2\nport=42000\n").unwrap();
    symlink(&target, &path).unwrap();
    set_port(&path, 41000).unwrap();
    assert!(path.is_symlink());
    assert_eq!(
        read_table(&target).unwrap()["port"].as_integer(),
        Some(41000)
    );
    unlocked(&path);
    let file = LockedTable::open(&path).unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&other, &path).unwrap();
    assert!(file.commit().is_err());
    assert_eq!(
        read_table(&other).unwrap()["port"].as_integer(),
        Some(42000)
    );
    fs::remove_file(&path).unwrap();
    symlink(dir.path().join("missing"), &path).unwrap();
    assert!(set_port(&path, 45000).is_err());
    assert!(path.is_symlink());
    assert!(!dir.path().join("missing").exists());
}

#[cfg(unix)]
struct ModeRestore(PathBuf, u32);
#[cfg(unix)]
impl Drop for ModeRestore {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1)).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn read_and_write_access_errors_preserve_original_and_release_the_guard() {
    use std::os::unix::fs::PermissionsExt;
    // Real unprivileged access checks, not a mocked successful replacement.

    let (_dir, path) = fixture();
    fs::write(&path, secret_doc()).unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_ne!(
        fs::metadata(&path).unwrap().uid(),
        0,
        "permission fixture requires an unprivileged test user"
    );
    let restore = ModeRestore(path.clone(), 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    assert!(set_port(&path, 41000).is_err());
    unlocked(&path);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(replace_webhooks(&path, vec![]).is_err());
    unlocked(&path);
    assert_eq!(read_port(&path), Some(40000));
    assert!(fs::read_to_string(&path).unwrap() == secret_doc());
    drop(restore);
    clear_port(&path).unwrap();
    unlocked(&path);
}

#[cfg(unix)]
#[test]
fn temporary_creation_failure_does_not_replace_or_leave_a_temp_file() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, path) = fixture();
    fs::write(&path, secret_doc()).unwrap();
    drop(file_replace::lock(&path).unwrap()); // Existing lock can be opened in a nonwritable dir.
    let restore = ModeRestore(dir.path().into(), 0o700);
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
    assert!(set_port(&path, 41000).is_err());
    assert!(fs::read_to_string(&path).unwrap() == secret_doc());
    unlocked(&path);
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        2,
        "only original and lock sidecar remain"
    );
    drop(restore);
    set_port(&path, 41000).unwrap();
    unlocked(&path);
}

#[cfg(unix)]
#[test]
fn legacy_read_keeps_its_in_memory_port_when_migration_cannot_be_saved() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, path) = fixture();
    fs::write(&path, "port=40000\n").unwrap();
    let restore = ModeRestore(path.clone(), 0o600);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(read_port(&path), Some(40000));
    unlocked(&path);
    assert!(fs::read_to_string(&path).unwrap() == "port=40000\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&path, "port=28429\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert_eq!(read_port(&path), None);
    unlocked(&path);
    assert!(fs::read_to_string(&path).unwrap() == "port=28429\n");
    drop(restore);
}

#[test]
fn explicit_port_mutations_refresh_the_format_marker_but_host_section_does_not() {
    let (_dir, path) = fixture();
    for marker in ["3", "'custom'"] {
        fs::write(
            &path,
            format!("format={marker}\nport=40000\n[listener]\nkeep=true\n"),
        )
        .unwrap();
        replace_webhooks(&path, vec![]).unwrap();
        let table = read_table(&path).unwrap();
        assert!(
            table["format"].as_integer() == Some(3) || table["format"].as_str() == Some("custom")
        );
        set_port(&path, 41000).unwrap();
        assert_eq!(read_table(&path).unwrap()["format"].as_integer(), Some(2));
        fs::write(
            &path,
            format!("format={marker}\nport=40000\n[listener]\nkeep=true\n"),
        )
        .unwrap();
        clear_port(&path).unwrap();
        let table = read_table(&path).unwrap();
        assert_eq!(table["format"].as_integer(), Some(2));
        assert!(!table.contains_key("port"));
        assert_eq!(table["listener"]["keep"].as_bool(), Some(true));
    }
}

#[test]
fn a_legacy_file_deleted_while_migration_waits_is_not_recreated() {
    use std::time::Duration;
    let (_dir, path) = fixture();
    fs::write(&path, "port=40000\n").unwrap();
    let lock = file_replace::lock(&path).unwrap();
    let (started, start) = std::sync::mpsc::channel();
    let (finished, done) = std::sync::mpsc::channel();
    let source = path.clone();
    let worker = std::thread::spawn(move || {
        started.send(()).unwrap();
        finished.send(read_port(&source)).unwrap();
    });
    start.recv_timeout(Duration::from_secs(3)).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(
        done.try_recv().is_err(),
        "legacy migration must wait on the participating lock"
    );
    // A nonparticipating editor removes the file before the waiting reader acquires its lock.
    fs::remove_file(&path).unwrap();
    drop(lock);
    assert_eq!(done.recv_timeout(Duration::from_secs(3)).unwrap(), None);
    worker.join().unwrap();
    assert!(
        !path.exists(),
        "a read must not resurrect an externally deleted configuration"
    );
    unlocked(&path);
}
