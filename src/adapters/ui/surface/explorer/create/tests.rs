use super::*;

#[test]
fn names_are_checked_for_empty_invalid_characters_and_reserved_names() {
    assert_eq!(check_name("", false), Err(NameError::Empty));
    assert_eq!(
        check_name("drafts/2026", false),
        Err(NameError::InvalidChar('/'))
    );
    assert_eq!(check_name("a:b", false), Ok(()));
    assert_eq!(check_name("a:b", true), Err(NameError::InvalidChar(':')));
    assert_eq!(
        check_name("tab\there", true),
        Err(NameError::InvalidChar('\t'))
    );
    assert_eq!(check_name("..", false), Err(NameError::Reserved));
    assert_eq!(check_name("CON", true), Err(NameError::Reserved));
    assert_eq!(check_name("com3.txt", true), Err(NameError::Reserved));
    assert_eq!(check_name("CON", false), Ok(()));
    assert_eq!(check_name("COM0", true), Ok(()));
    assert_eq!(check_name("console", true), Ok(()));
    assert_eq!(check_name("New folder", true), Ok(()));
}

#[test]
fn a_taken_default_name_gets_the_next_free_number_before_the_extension() {
    let taken = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(default_name("New folder", &taken(&[])), "New folder");
    assert_eq!(
        default_name("New folder", &taken(&["New folder"])),
        "New folder 2"
    );
    assert_eq!(
        default_name("untitled.txt", &taken(&["untitled.txt", "untitled 2.txt"])),
        "untitled 3.txt"
    );
    assert_eq!(default_name(".env", &taken(&[".env"])), ".env 2");
}

#[test]
fn the_folder_default_selects_all_and_the_file_default_selects_the_stem() {
    let folder = CreateEdit::new("/w".into(), true, Vec::new());
    assert_eq!(
        folder.edit.initial_selection,
        Some(0..folder.edit.buf.chars().count())
    );
    let file = CreateEdit::new("/w".into(), false, Vec::new());
    let stem = split_ext(&file.edit.buf).0.chars().count();
    assert_eq!(file.edit.initial_selection, Some(0..stem));
    assert!(stem < file.edit.buf.chars().count());
}

#[test]
fn an_existing_name_is_refused_on_enter_and_cleared_by_typing() {
    let mut create = CreateEdit::new("/w".into(), true, vec!["Documents".into()]);
    create.edit.buf = "Documents".into();
    assert_eq!(create.error(), None);
    assert_eq!(create.confirmable(), Err(NameError::Exists));
    create.exists = Some(create.edit.buf.clone());
    assert_eq!(create.error(), Some(NameError::Exists));
    create.edit.buf = "Documents 2".into();
    assert_eq!(create.error(), None);
    assert_eq!(create.confirmable(), Ok(()));
}

#[test]
fn the_name_list_comes_from_the_shown_folder_or_the_sidebar_tree() {
    let mut view = ExplorerView::new();
    let panel = crate::model::ExplorerPanel::new(1, "/w".into());
    view.sync(&panel, None);
    view.entries = vec![placeholder_named("/w", t("explorer.new.folder_default"))];
    view.start_create("/w".into(), true);
    let create = view.create.as_ref().expect("editor opened");
    assert_eq!(
        create.edit.buf,
        format!("{} 2", t("explorer.new.folder_default"))
    );
    view.start_create("/elsewhere".into(), false);
    assert_eq!(
        view.create.as_ref().unwrap().edit.buf,
        t("explorer.new.file_default")
    );
}

fn placeholder_named(dir: &str, name: &str) -> super::super::DirEntryInfo {
    let mut row = placeholder_row(Path::new(dir));
    row.path = Path::new(dir).join(name);
    row.name = name.to_string();
    row
}

#[test]
fn blank_names_are_empty_edge_spaces_are_refused_and_windows_refuses_a_trailing_dot() {
    assert_eq!(check_name("   ", false), Err(NameError::Empty));
    assert_eq!(check_name(" \t", true), Err(NameError::Empty));
    // 앞뒤 공백은 OS 와 관계없이 떼지 않고 거절한다.
    for windows in [false, true] {
        assert_eq!(check_name("report ", windows), Err(NameError::EdgeSpace));
        assert_eq!(check_name(" report", windows), Err(NameError::EdgeSpace));
        assert_eq!(check_name("my report", windows), Ok(()));
    }
    // Windows 는 끝의 점을 떼고 만든다. Unix 는 그 이름 그대로 만든다.
    assert_eq!(check_name("a.", true), Err(NameError::Reserved));
    assert_eq!(check_name("a.", false), Ok(()));
    crate::i18n::init("en");
    assert_eq!(
        NameError::EdgeSpace.message("report "),
        "Names can't start or end with a space."
    );
    assert_eq!(check_name(".hidden", true), Ok(()));
}
