//! A listing prepares one index; independent selections detach only on mutation.
use super::*;
use crate::adapters::ui::surface::explorer::view::EntrySnapshot;

#[test]
fn full_selection_reuses_the_prepared_index_and_detaches_on_toggle() {
    let snapshot: EntrySnapshot = (0..100_000)
        .map(|i| DirEntryInfo {
            path: PathBuf::from(format!("/long/parent/item{i:06}.txt")),
            ..Default::default()
        })
        .collect();
    let mut chosen = snapshot.selected_all();
    let cached = snapshot.selected_all();
    assert!(Arc::ptr_eq(&chosen.buckets, &cached.buckets));
    assert!(Arc::ptr_eq(
        chosen.storage.entries.as_ref().unwrap(),
        cached.storage.entries.as_ref().unwrap()
    ));
    assert_eq!(chosen.len(), 100_000);
    let removed = Path::new("/long/parent/item000017.txt");
    assert!(chosen.remove(removed));
    assert!(!Arc::ptr_eq(&chosen.buckets, &cached.buckets));
    assert!(!chosen.contains(removed));
    assert!(cached.contains(removed));
    let again = snapshot.selected_all();
    assert!(Arc::ptr_eq(&again.buckets, &cached.buckets));
    assert_eq!(again.len(), 100_000);
    drop(snapshot);
    chosen.clear();
    assert_eq!(cached.owned().count(), 100_000);
    assert!(cached.contains(removed));
}

#[test]
fn mutating_and_clearing_an_alias_selection_cannot_change_the_listing_cache() {
    let paths = ["/w/./a", "/w//a", "/other/a", "/w/b", "/"];
    let snapshot: EntrySnapshot = paths
        .map(|path| DirEntryInfo {
            path: path.into(),
            ..Default::default()
        })
        .into_iter()
        .collect();
    let expected: HashSet<_> = paths.into_iter().map(PathBuf::from).collect();
    let cached = snapshot.selected_all();
    let mut chosen = snapshot.selected_all();
    assert!(chosen.insert("/third/a".into()));
    chosen.retain(|path| !path.starts_with("/other"));
    assert!(!chosen.contains(Path::new("/other/a")));
    assert!(cached.contains(Path::new("/other/a")));
    chosen.clear();
    assert!(chosen.is_empty());
    let again = snapshot.selected_all();
    assert!(Arc::ptr_eq(&again.buckets, &cached.buckets));
    assert_same(&again, &expected);
}
