mod cached;

use super::*;

fn assert_same(actual: &SelectedPaths, expected: &HashSet<PathBuf>) {
    assert_eq!(actual, expected);
    assert_eq!(actual.iter().count(), expected.len());
    assert_eq!(actual.is_empty(), expected.is_empty());
    for path in expected {
        assert!(actual.contains(path));
    }
}

#[test]
fn normalized_paths_and_duplicate_names_keep_standard_path_equality() {
    let paths = [
        "/w/a", "/w//a", "/w/./a", "/w/a/", "/other/a", "/w/a/..", "/w/a/../", "/", "//", ".",
        "./", "", "a", "./a", "a/.", "a//", "../a", ".././a",
    ]
    .map(PathBuf::from);
    let mut actual = SelectedPaths::default();
    let mut expected = HashSet::new();
    for path in paths {
        assert_eq!(actual.insert(path.clone()), expected.insert(path));
        assert_same(&actual, &expected);
    }
    for spelling in ["/w/./a/", "./a", "/w/a/..", "//", "missing"] {
        let path = Path::new(spelling);
        assert_eq!(actual.remove(path), expected.remove(path));
        assert_same(&actual, &expected);
    }
}

#[test]
fn recursive_common_names_use_a_full_path_set_and_survive_clone_retain_clear() {
    let paths: Vec<_> = (0..5_000)
        .map(|i| PathBuf::from(format!("/root/{i}/common.txt")))
        .collect();
    let mut actual: SelectedPaths = paths.iter().cloned().collect();
    let mut expected: HashSet<_> = paths.iter().cloned().collect();
    assert_same(&actual, &expected);
    assert!(matches!(
        actual.buckets.values().next(),
        Some(Bucket::Many(_))
    ));
    let cloned = actual.clone();
    for path in paths.iter().step_by(3) {
        assert_eq!(actual.remove(path), expected.remove(path));
    }
    actual.retain(|p| p.parent().unwrap().ends_with("17"));
    expected.retain(|p| p.parent().unwrap().ends_with("17"));
    assert_same(&actual, &expected);
    assert_eq!(cloned.len(), 5_000);
    assert!(cloned.contains(&paths[0]));
    actual.clear();
    assert_same(&actual, &HashSet::new());
    actual.extend(paths.iter().cloned());
    assert_eq!(actual, cloned);
}

#[test]
fn a_hundred_thousand_long_paths_match_the_standard_set() {
    let prefix = "/long/parent/component/repeated/in/every/selected/entry/without/copying/the/name";
    let mut actual = SelectedPaths::with_capacity(100_000);
    let mut expected = HashSet::with_capacity(100_000);
    for i in 0..100_000 {
        let path = PathBuf::from(format!("{prefix}/item{i:06}.txt"));
        assert_eq!(actual.insert(path.clone()), expected.insert(path));
    }
    assert_same(&actual, &expected);
    assert!(!actual.contains(Path::new("/different/parent/item099999.txt")));
    assert!(!actual.remove(Path::new("/different/parent/item099999.txt")));
    actual.retain(|p| {
        p.file_name()
            .unwrap()
            .as_encoded_bytes()
            .ends_with(b"7.txt")
    });
    expected.retain(|p| {
        p.file_name()
            .unwrap()
            .as_encoded_bytes()
            .ends_with(b"7.txt")
    });
    assert_same(&actual, &expected);
}

#[cfg(unix)]
#[test]
fn non_utf8_names_and_aliases_keep_os_path_equality() {
    use std::os::unix::ffi::OsStringExt;
    let name = std::ffi::OsString::from_vec(vec![b'n', 0xff]);
    let path = Path::new("/w").join(&name);
    let alias = Path::new("/w/./").join(&name);
    let other = Path::new("/other").join(name);
    let actual: SelectedPaths = [path.clone(), alias.clone(), other.clone()].into();
    let expected = HashSet::from([path, alias.clone(), other]);
    assert_same(&actual, &expected);
    assert!(actual.contains(&alias));
}

#[test]
fn bulk_paths_share_storage_reuse_capacity_and_keep_owned_payloads() {
    let paths: Vec<_> = (0..100_000)
        .map(|i| PathBuf::from(format!("/long/parent/with/unicode/한글/item{i:06}.txt")))
        .collect();
    let expected: HashSet<_> = paths.iter().cloned().collect();
    let mut actual = SelectedPaths::default();
    actual.refill(paths.len(), paths.iter().map(PathBuf::as_path));
    assert_same(&actual, &expected);
    assert!(!actual.storage.packed.is_empty());
    assert!(
        actual
            .buckets
            .values()
            .all(|bucket| !matches!(bucket, Bucket::One(StoredPath::Owned(_))))
    );
    assert_eq!(actual.owned().collect::<HashSet<_>>(), expected);
    let mut iter = actual.iter();
    assert_eq!(iter.len(), 100_000);
    assert!(iter.next().is_some());
    assert_eq!(iter.len(), 99_999);
    drop(iter);
    assert!(actual.staged.is_empty());
    let scratch_pointer = actual.staged.as_ptr();
    let scratch_capacity = actual.staged.capacity();
    let pointer = actual.storage.packed.as_ptr();
    let capacity = actual.storage.packed.capacity();
    let original = actual.clone();
    actual.refill(10, paths[..10].iter().map(PathBuf::as_path));
    assert_eq!(actual.storage.packed.as_ptr(), pointer);
    assert!(actual.staged.is_empty());
    assert_eq!(actual.staged.as_ptr(), scratch_pointer);
    assert_eq!(actual.staged.capacity(), scratch_capacity);
    assert_eq!(actual.storage.packed.capacity(), capacity);
    assert_eq!(actual.len(), 10);
    assert_eq!(original.len(), 100_000);
    assert!(original.contains(&paths[99_999]));
    assert!(!actual.contains(&paths[99_999]));
}

#[test]
fn packed_paths_promote_collisions_and_preserve_the_first_original_spelling() {
    let paths = ["/w/./a", "/w//a", "/other/a", "/w/b", "/"].map(PathBuf::from);
    let mut actual = SelectedPaths::default();
    actual.refill(paths.len(), paths.iter().map(PathBuf::as_path));
    let mut expected: HashSet<_> = paths.into_iter().collect();
    assert_same(&actual, &expected);
    let originals = |paths: Vec<PathBuf>| {
        let mut paths: Vec<_> = paths.into_iter().map(PathBuf::into_os_string).collect();
        paths.sort();
        paths
    };
    assert_eq!(
        originals(actual.owned().collect()),
        originals(expected.iter().cloned().collect())
    );
    let snapshot = actual.clone();
    assert_eq!(
        actual.remove(Path::new("/w/a")),
        expected.remove(Path::new("/w/a"))
    );
    actual.insert(PathBuf::from("/third/a"));
    expected.insert(PathBuf::from("/third/a"));
    actual.retain(|path| !path.starts_with("/other"));
    expected.retain(|path| !path.starts_with("/other"));
    assert_same(&actual, &expected);
    assert!(snapshot.contains(Path::new("/w/a")));
    assert!(snapshot.contains(Path::new("/other/a")));
}

#[cfg(unix)]
#[test]
fn bulk_selection_keeps_non_utf8_paths_without_lossy_conversion() {
    use std::os::unix::ffi::OsStringExt;
    let name = std::ffi::OsString::from_vec(vec![b'n', 0xff]);
    let paths = [
        Path::new("/w").join(&name),
        Path::new("/other").join(name),
        PathBuf::from("/w/한글"),
    ];
    let mut actual = SelectedPaths::default();
    actual.refill(paths.len(), paths.iter().map(PathBuf::as_path));
    let expected: HashSet<_> = paths.into_iter().collect();
    assert_same(&actual, &expected);
    assert_eq!(actual.owned().collect::<HashSet<_>>(), expected);
    let entries = std::sync::Arc::new(
        expected
            .iter()
            .map(|path| crate::core::fs_list::DirEntryInfo {
                path: path.clone(),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    );
    actual.refill_entries(entries);
    assert_same(&actual, &expected);
    assert_eq!(actual.owned().collect::<HashSet<_>>(), expected);
}

#[test]
fn listing_snapshot_selection_keeps_paths_after_replacement_without_copying_bytes() {
    let entries = std::sync::Arc::new(
        (0..100_000)
            .map(|i| crate::core::fs_list::DirEntryInfo {
                path: PathBuf::from(format!("/long/parent/한글/item{i:06}.txt")),
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    );
    let expected: HashSet<_> = entries.iter().map(|e| e.path.clone()).collect();
    let mut actual = SelectedPaths::default();
    actual.refill_entries(entries.clone());
    assert!(std::sync::Arc::ptr_eq(
        actual.storage.entries.as_ref().unwrap(),
        &entries
    ));
    assert!(actual.storage.packed.is_empty());
    assert_same(&actual, &expected);
    let mut cloned = actual.clone();
    assert!(actual.remove(&entries[17].path));
    assert!(cloned.contains(&entries[17].path));
    assert!(actual.insert(PathBuf::from("/new/added.txt")));
    drop(entries);
    cloned.retain(|p| p.file_name().unwrap() == "item000017.txt");
    assert_eq!(
        cloned.owned().collect::<Vec<_>>(),
        vec![PathBuf::from("/long/parent/한글/item000017.txt")]
    );
    actual.clear();
    assert!(actual.storage.entries.is_none());
    assert!(actual.is_empty());
    assert_eq!(cloned.len(), 1);
}

#[test]
fn shared_listing_selection_keeps_alias_equality_and_collision_promotion() {
    let entries = std::sync::Arc::new(
        ["/w/./a", "/w//a", "/other/a", "/"]
            .map(|p| crate::core::fs_list::DirEntryInfo {
                path: p.into(),
                ..Default::default()
            })
            .to_vec(),
    );
    let expected = entries.iter().map(|e| e.path.clone()).collect();
    let mut actual = SelectedPaths::default();
    actual.refill_entries(entries);
    assert_same(&actual, &expected);
    actual.retain(|p| p != Path::new("/other/a"));
    assert!(actual.contains(Path::new("/w/a")));
    assert!(!actual.contains(Path::new("/other/a")));
}
