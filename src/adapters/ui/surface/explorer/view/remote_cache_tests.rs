//! Fresh address-probe responses replace an already displayed same-root listing once.
use super::*;

fn entry(name: &str) -> DirEntryInfo {
    DirEntryInfo {
        path: Path::new("/srv").join(name),
        name: name.into(),
        ..Default::default()
    }
}

#[test]
fn successful_same_root_file_probe_applies_new_listing_and_reveals_file_once() {
    let ctx = egui::Context::default();
    let mut panel = ExplorerPanel::new(1, "/srv".into());
    let mut view = ExplorerView::default();
    view.sync(&panel, Some(7));
    let initial = view.drain_outbox().pop().unwrap();
    assert!(view.apply_remote_list_dir_result(
        initial.request_id,
        &panel,
        Ok(vec![entry("old.txt")])
    ));
    view.sync(&panel, Some(7));
    let previous_generation = view.entries_gen;
    assert!(
        view.address_action(&ctx, Some("/srv/notes.md"), Path::new("/srv"), true)
            .is_none()
    );
    let probe = view.drain_outbox().pop().unwrap();
    assert!(view.apply_remote_list_dir_result(
        probe.request_id,
        &panel,
        Ok(vec![entry("old.txt"), entry("notes.md")]),
    ));
    let Some(super::super::ExplorerAction::Navigate(dir)) =
        view.address_action(&ctx, None, Path::new("/srv"), true)
    else {
        panic!("successful file probe must navigate to its parent");
    };
    panel.active_tab_mut().navigate_to(dir);
    assert_eq!(panel.current_root(), Path::new("/srv"));
    assert_eq!(view.entries_gen, previous_generation);
    view.sync(&panel, Some(7));
    assert_eq!(view.entries_gen, previous_generation + 1);
    assert_eq!(view.take_reveal(), Some(PathBuf::from("/srv/notes.md")));
    assert!(view.selected.contains(Path::new("/srv/notes.md")));
    assert_eq!(view.selected.len(), 1);
    let entries_ptr = view.entries.as_ptr();
    for _ in 0..90 {
        view.sync(&panel, Some(7));
        assert_eq!(view.entries_gen, previous_generation + 1);
        assert_eq!(view.entries.as_ptr(), entries_ptr);
    }
    assert!(view.drain_outbox().is_empty());
    assert!(view.take_reveal().is_none());
}

#[test]
fn caching_another_folder_does_not_rebuild_the_current_listing() {
    let panel = ExplorerPanel::new(1, "/srv".into());
    let mut view = ExplorerView::default();
    view.sync(&panel, Some(7));
    let initial = view.drain_outbox().pop().unwrap();
    assert!(view.apply_remote_list_dir_result(
        initial.request_id,
        &panel,
        Ok(vec![entry("old.txt")])
    ));
    let generation = view.entries_gen;
    let entries_ptr = view.entries.as_ptr();
    view.cache_remote_listing("/elsewhere".into(), Vec::new());
    view.sync(&panel, Some(7));
    assert_eq!(view.entries_gen, generation);
    assert_eq!(view.entries.as_ptr(), entries_ptr);
}
