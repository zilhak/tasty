use super::super::Entry;
use super::*;
use std::{io::Read, path::PathBuf, process::Command};

fn junction(link: &Path, target: &Path) {
    let output = Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .unwrap();
    assert!(output.status.success(), "junction: {:?}", output);
}
fn fixture(ancestor: bool) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let approved = dir.path().join("approved");
    let outside = dir.path().join("outside");
    let suffix = if ancestor { "sub" } else { "" };
    std::fs::create_dir_all(approved.join(suffix)).unwrap();
    std::fs::create_dir_all(outside.join(suffix)).unwrap();
    let source = approved.join(suffix).join("entry.html");
    std::fs::write(&source, b"approved entry").unwrap();
    std::fs::write(outside.join(suffix).join("entry.html"), vec![b'X'; 49152]).unwrap();
    (dir, approved, outside, source)
}
#[test]
fn root_and_ancestor_redirect_before_pin_are_rejected() {
    for ancestor in [false, true] {
        let (dir, approved, outside, source) = fixture(ancestor);
        let canonical = source.parent().unwrap().canonicalize().unwrap();
        let held = dir.path().join("held");
        for _ in 0..3 {
            std::fs::rename(&approved, &held).unwrap();
            junction(&approved, &outside);
            assert!(Root::open(&canonical).is_err());
            std::fs::remove_dir(&approved).unwrap();
            std::fs::rename(&held, &approved).unwrap();
        }
        dir.close().unwrap();
    }
}
#[test]
fn original_root_survives_repeated_root_and_ancestor_redirects() {
    for ancestor in [false, true] {
        let (dir, approved, outside, source) = fixture(ancestor);
        let canonical = source.parent().unwrap().canonicalize().unwrap();
        let root = Root::open(&canonical).unwrap();
        let held = dir.path().join("held");
        for _ in 0..3 {
            let swapped = match std::fs::rename(&approved, &held) {
                Ok(()) => true,
                // Some Windows filesystems forbid renaming an ancestor of an open
                // directory. This is an observed additional boundary, not a successful swap.
                Err(e) if ancestor && matches!(e.raw_os_error(), Some(5 | 32)) => false,
                Err(e) => panic!("rename ancestor={ancestor}: {e}"),
            };
            if swapped {
                junction(&approved, &outside);
            }
            let mut file = root.open_file(Path::new("entry.html")).unwrap();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"approved entry");
            assert_eq!(root.is_current(&canonical).unwrap_or(false), !swapped);
            drop(file);
            if swapped {
                std::fs::remove_dir(&approved).unwrap();
                std::fs::rename(&held, &approved).unwrap();
            }
            assert!(root.is_current(&canonical).unwrap());
        }
        drop(root);
        dir.close().unwrap();
    }
}
#[test]
fn relative_intermediate_junction_and_traversal_are_rejected() {
    let (dir, approved, outside, source) = fixture(false);
    let root = Root::open(&source.parent().unwrap().canonicalize().unwrap()).unwrap();
    junction(&approved.join("sub"), &outside);
    assert!(root.open_file(Path::new("sub/entry.html")).is_err());
    for path in [
        "",
        "../outside/entry.html",
        "C:\\entry.html",
        "\\entry.html",
    ] {
        assert!(root.open_file(Path::new(path)).is_err());
    }
    std::fs::remove_dir(approved.join("sub")).unwrap();
    drop(root);
    dir.close().unwrap();
}

#[test]
fn ordinary_root_replacement_does_not_reauthorize_a_new_identity() {
    let (dir, approved, _outside, source) = fixture(false);
    let canonical = source.parent().unwrap().canonicalize().unwrap();
    let root = Root::open(&canonical).unwrap();
    let held = dir.path().join("held");
    // Swap in the pin-to-file-open gap. Windows may refuse ancestor renames once a
    // descendant file is open, so do not rely on that additional OS restriction.
    std::fs::rename(&approved, &held).unwrap();
    std::fs::create_dir(&approved).unwrap();
    std::fs::write(&source, b"approved entry").unwrap();
    let file = root.open_file(Path::new("entry.html")).unwrap();
    let stamp = Stamp::of(&file).unwrap();
    let entry = Entry {
        file,
        stamp,
        root: canonical,
        source: source.clone(),
        authorized_root: root,
    };
    assert!(!entry.authorized_root.is_current(&entry.root).unwrap());
    assert!(!entry.unchanged());
    let mut original = entry
        .authorized_root
        .open_file(Path::new("entry.html"))
        .unwrap();
    let mut bytes = Vec::new();
    original.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"approved entry");
    drop(original);
    std::fs::remove_file(&source).unwrap();
    std::fs::remove_dir(&approved).unwrap();
    drop(entry);
    std::fs::rename(&held, &approved).unwrap();
    assert!(Entry::open(&source).unwrap().unchanged());
    dir.close().unwrap();
}
