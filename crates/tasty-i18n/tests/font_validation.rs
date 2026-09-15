#[test]
fn font_bytes_are_validated_before_they_leave_the_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("font.ttf");
    assert!(matches!(
        tasty_i18n::font::read_validated(&path),
        Err(tasty_i18n::font::LocaleFontError::Read(_))
    ));
    std::fs::write(&path, b"broken font").unwrap();
    assert!(matches!(
        tasty_i18n::font::read_validated(&path),
        Err(tasty_i18n::font::LocaleFontError::Parse)
    ));
    let valid = include_bytes!("../../tasty-font/assets/D2Coding-ligature-Regular.ttf");
    std::fs::write(&path, valid).unwrap();
    assert_eq!(tasty_i18n::font::read_validated(&path).unwrap(), valid);
}
