use tasty_i18n::plugin_catalog::{load, override_path};

#[test]
fn paths_preserve_plugin_and_locale_boundaries() {
    let root = std::path::Path::new("language-root");
    assert_eq!(
        override_path(root, "com.example.viewer", "ko").unwrap(),
        root.join("plugins/com.example.viewer/ko.toml")
    );
    assert_eq!(
        override_path(root, "com.example.viewer", "fr-CA").unwrap(),
        root.join("fr-CA/plugins/com.example.viewer.toml")
    );
    for id in [
        "",
        ".",
        "..",
        "../com.other",
        "com/other",
        r"com\other",
        "C:com.other",
        "viewer",
    ] {
        assert!(override_path(root, id, "ko").is_none(), "{id}");
    }
    for locale in ["", "..", "../ko", r"..\ko", "/en"] {
        assert!(
            override_path(root, "com.example.viewer", locale).is_none(),
            "{locale}"
        );
    }
}

#[test]
fn oversized_overrides_keep_the_installed_translation() {
    let dir = tempfile::tempdir().unwrap();
    let installed = dir.path().join("installed");
    std::fs::create_dir(&installed).unwrap();
    std::fs::write(installed.join("en.toml"), "[viewer]\nlabel='installed'").unwrap();
    let user = dir.path().join("user");
    let override_file = override_path(&user, "com.example.viewer", "en").unwrap();
    std::fs::create_dir_all(override_file.parent().unwrap()).unwrap();
    std::fs::write(
        &override_file,
        " ".repeat(tasty_i18n::MAX_PACK_BYTES as usize + 1),
    )
    .unwrap();
    let strings = load(&installed, "en", "com.example.viewer", Some(&user));
    assert_eq!(strings["viewer.label"], "installed");
}
