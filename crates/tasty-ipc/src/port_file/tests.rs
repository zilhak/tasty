use super::*;
use std::fs;

#[test]
fn publication_is_complete_before_return_and_cleanup_preserves_a_foreign_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasty.port");
    let publication = publish_port_file_to(4242, Some(&path)).unwrap();
    assert_eq!(
        read_port_file_from(Some(path.to_str().unwrap())).unwrap(),
        4242
    );
    fs::write(&path, b"4243").unwrap();
    assert!(!publication.cleanup_if_owned().unwrap());
    assert_eq!(fs::read(&path).unwrap(), b"4243");
    let publication = publish_port_file_to(4244, Some(&path)).unwrap();
    assert!(publication.cleanup_if_owned().unwrap());
    assert!(!publication.cleanup_if_owned().unwrap());
    assert!(!path.exists());
}

#[test]
fn nonregular_target_and_failed_parent_creation_never_publish() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("directory.port");
    fs::create_dir(&path).unwrap();
    assert!(publish_port_file_to(4242, Some(&path)).is_err());
    let parent = dir.path().join("file");
    fs::write(&parent, b"original").unwrap();
    assert!(write_port_file_to(4242, Some(&parent.join("tasty.port"))).is_err());
    assert_eq!(fs::read(parent).unwrap(), b"original");
    assert!(
        fs::read_dir(dir.path()).unwrap().all(|x| !x
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}

#[cfg(unix)]
#[test]
fn owner_permissions_readonly_unreadable_and_live_symlink_are_preserved() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("actual");
    write_port_file_to(4242, Some(&target)).unwrap();
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let link = dir.path().join("tasty.port");
    symlink("actual", &link).unwrap();
    write_port_file_to(4243, Some(&link)).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).unwrap(), b"4243");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
    fs::set_permissions(&target, fs::Permissions::from_mode(0o000)).unwrap();
    assert!(write_port_file_to(4244, Some(&link)).is_err());
    fs::set_permissions(&target, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(write_port_file_to(4244, Some(&link)).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"4243");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&target).unwrap();
    assert!(write_port_file_to(4244, Some(&link)).is_err());
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn edit_after_observation_does_not_get_overwritten() {
    use tasty_utils::file_replace::{self, Fingerprint};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasty.port");
    fs::write(&path, b"4242").unwrap();
    let target = file_replace::resolve_link(&path).unwrap();
    let observed = Fingerprint::read(&target).unwrap();
    fs::write(&path, b"4243").unwrap();
    assert!(publish_observed(4244, path.clone(), target, observed).is_err());
    assert_eq!(fs::read(path).unwrap(), b"4243");
}
