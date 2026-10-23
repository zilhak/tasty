use super::*;
use tempfile::TempDir;

#[test]
fn real_store_seed_sync_and_deleted_latte_preserve_custom_themes() {
    let d = TempDir::new().unwrap();
    let store = ThemeFiles::new(d.path().into());
    fs::write(d.path().join("README.md"), b"keep").unwrap();
    store.first_run_init().unwrap();
    assert_eq!(fs::read(d.path().join("README.md")).unwrap(), b"keep");
    assert_eq!(
        fs::read_to_string(store.path(BUILTIN_MOCHA_ID)).unwrap(),
        crate::MOCHA_TOML_TEXT
    );
    assert_eq!(
        fs::read_to_string(store.path(BUILTIN_LATTE_ID)).unwrap(),
        crate::LATTE_TOML_TEXT
    );
    fs::write(store.path(BUILTIN_LATTE_ID), "label = 'old latte'\n").unwrap();
    store.sync_builtin_themes().unwrap();
    assert_eq!(
        fs::read_to_string(store.path(BUILTIN_LATTE_ID)).unwrap(),
        crate::LATTE_TOML_TEXT
    );
    fs::remove_file(store.path(BUILTIN_LATTE_ID)).unwrap();
    fs::write(d.path().join("custom.toml"), "label = 'custom'\n").unwrap();
    fs::write(store.path(BUILTIN_MOCHA_ID), "label = 'old'\n").unwrap();
    store.first_run_init().unwrap();
    store.sync_builtin_themes().unwrap();
    assert!(!store.path(BUILTIN_LATTE_ID).exists());
    assert_eq!(
        fs::read_to_string(d.path().join("custom.toml")).unwrap(),
        "label = 'custom'\n"
    );
    assert_eq!(
        fs::read_to_string(store.path(BUILTIN_MOCHA_ID)).unwrap(),
        crate::MOCHA_TOML_TEXT
    );
}

#[test]
fn readable_corruption_repairs_but_invalid_utf8_and_nonregular_originals_survive() {
    let d = TempDir::new().unwrap();
    let store = ThemeFiles::new(d.path().into());
    let path = store.path(BUILTIN_MOCHA_ID);
    fs::write(&path, "bad = [").unwrap();
    store.ensure_mocha_exists().unwrap();
    assert!(crate::file::ThemeFile::parse(&fs::read_to_string(&path).unwrap()).is_ok());
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(store.sync_builtin_themes().is_err());
    assert!(store.ensure_mocha_exists().is_err());
    assert!(store.rewrite_mocha_fallback().is_err());
    assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(store.sync_builtin_themes().is_err());
    assert!(path.is_dir());
}

#[test]
fn external_edit_during_participant_wait_is_preserved() {
    let d = TempDir::new().unwrap();
    let path = d.path().join("mocha.toml");
    fs::write(&path, "label = 'old'\n").unwrap();
    let (observed, _) = Observed::read(path.clone()).unwrap();
    let held = file_replace::lock(&path).unwrap();
    let (start, started) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        start.send(()).unwrap();
        observed.replace(crate::MOCHA_TOML_TEXT)
    });
    started.recv().unwrap();
    fs::write(&path, "label = 'external'\n").unwrap();
    drop(held);
    assert!(worker.join().unwrap().is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "label = 'external'\n");
    assert!(
        fs::read_dir(d.path()).unwrap().all(|x| !x
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}

#[cfg(unix)]
#[test]
fn live_links_keep_the_link_and_changed_or_broken_links_are_not_repaired() {
    use std::os::unix::fs::symlink;
    let d = TempDir::new().unwrap();
    let store = ThemeFiles::new(d.path().into());
    let path = store.path(BUILTIN_MOCHA_ID);
    let target = d.path().join("data");
    fs::write(&target, "label = 'old'\n").unwrap();
    symlink("data", &path).unwrap();
    store.sync_builtin_themes().unwrap();
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), crate::MOCHA_TOML_TEXT);
    let (observed, _) = Observed::read(path.clone()).unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(d.path().join("other"), b"foreign").unwrap();
    symlink("other", &path).unwrap();
    assert!(observed.replace(crate::MOCHA_TOML_TEXT).is_err());
    assert_eq!(fs::read(d.path().join("other")).unwrap(), b"foreign");
    fs::remove_file(&path).unwrap();
    symlink("absent", &path).unwrap();
    assert!(store.ensure_mocha_exists().is_err());
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn modes_read_failures_and_replacement_failures_protect_originals() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let d = TempDir::new().unwrap();
    let store = ThemeFiles::new(d.path().into());
    let path = store.path(BUILTIN_MOCHA_ID);
    fs::write(&path, "label = 'old'\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let owner = fs::metadata(&path).unwrap().uid();
    store.sync_builtin_themes().unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(fs::metadata(&path).unwrap().uid(), owner);
    fs::write(&path, b"original").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    assert!(store.ensure_mocha_exists().is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(store.sync_builtin_themes().is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let failed = store.sync_builtin_themes();
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(failed.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert!(
        fs::read_dir(d.path()).unwrap().all(|x| !x
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}
