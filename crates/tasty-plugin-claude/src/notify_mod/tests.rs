use super::*;

#[test]
fn merges_idempotently_and_uninstalls_only_own_path_on_both_platforms() {
    for (sep, own, other) in [
        (':', "/home/u/config/tasty-managed/notify-mod", "/other"),
        (
            ';',
            r"C:\Users\u\config\tasty-managed\notify-mod",
            r"D:\other",
        ),
    ] {
        let mut root = json!({"env":{"CLAUDE_CODE_PLUGIN_DIRS":other,"KEEP":"value"},"hooks":{"Stop":[{"command":"user"}]}});
        assert!(configure(&mut root, Path::new(own), sep, true).unwrap());
        let installed = root.clone();
        assert!(!configure(&mut root, Path::new(own), sep, true).unwrap());
        assert_eq!(root, installed);
        assert_eq!(root["env"][ENV], format!("{other}{sep}{own}"));
        assert!(configure(&mut root, Path::new(own), sep, false).unwrap());
        assert_eq!(root["env"][ENV], other);
        assert_eq!(root["env"]["KEEP"], "value");
        assert_eq!(root["hooks"]["Stop"][0]["command"], "user");
    }
}

#[test]
fn duplicate_own_entries_are_normalized_and_empty_key_is_removed() {
    let mut root = json!({"env":{ENV:"/own:/other:/own"}});
    configure(&mut root, Path::new("/own"), ':', true).unwrap();
    assert_eq!(root["env"][ENV], "/other:/own");
    configure(&mut root, Path::new("/own"), ':', false).unwrap();
    assert_eq!(root["env"][ENV], "/other");
    let mut root = json!({});
    configure(&mut root, Path::new("/own"), ':', true).unwrap();
    configure(&mut root, Path::new("/own"), ':', false).unwrap();
    assert!(root.get("env").is_none());
}

#[test]
fn invalid_user_settings_are_not_coerced_or_overwritten() {
    for mut root in [json!({"env":false}), json!({"env":{ENV:["user"]}})] {
        let original = root.clone();
        assert!(configure(&mut root, Path::new("/own"), ':', true).is_err());
        assert_eq!(root, original);
    }
    assert!(configure(&mut json!({}), Path::new("/has:separator"), ':', true).is_err());
}

#[test]
fn installed_assets_are_embedded_and_second_install_does_not_write() {
    let temp = tempfile::tempdir().unwrap();
    let settings = temp.path().join("settings.json");
    let mut root = json!({});
    assert!(install(&mut root, &settings).unwrap());
    let dir = directory(&settings).unwrap();
    let times: Vec<_> = ASSETS
        .iter()
        .map(|(name, content)| {
            let path = dir.join(name);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), *content);
            std::fs::metadata(path).unwrap().modified().unwrap()
        })
        .collect();
    assert!(!install(&mut root, &settings).unwrap());
    for ((name, _), time) in ASSETS.iter().zip(times) {
        assert_eq!(
            std::fs::metadata(dir.join(name))
                .unwrap()
                .modified()
                .unwrap(),
            time
        );
    }
    std::fs::write(dir.join("keep.txt"), "user").unwrap();
    assert!(uninstall(&mut root, &settings).unwrap());
    assert_eq!(
        std::fs::read_to_string(dir.join("keep.txt")).unwrap(),
        "user"
    );
    assert!(!uninstall(&mut root, &settings).unwrap());
    for (name, _) in ASSETS {
        assert!(!dir.join(name).exists());
    }
}

#[cfg(unix)]
#[test]
fn install_and_uninstall_refuse_linked_assets_without_touching_target() {
    let temp = tempfile::tempdir().unwrap();
    let settings = temp.path().join("settings.json");
    let mut root = json!({});
    install(&mut root, &settings).unwrap();
    let target = temp.path().join("user-file");
    std::fs::write(&target, "preserve").unwrap();
    let asset = directory(&settings).unwrap().join("hooks/register.ts");
    std::fs::remove_file(&asset).unwrap();
    std::os::unix::fs::symlink(&target, asset).unwrap();
    assert!(install(&mut root, &settings).is_err());
    assert!(uninstall(&mut root, &settings).is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "preserve");
}

#[test]
fn unowned_existing_directory_is_never_overwritten_or_removed() {
    let temp = tempfile::tempdir().unwrap();
    let settings = temp.path().join("settings.json");
    let dir = directory(&settings).unwrap();
    std::fs::create_dir_all(dir.join("hooks")).unwrap();
    std::fs::write(dir.join("hooks/register.ts"), "user").unwrap();
    let mut root = json!({});
    assert!(install(&mut root, &settings).is_err());
    assert!(uninstall(&mut root, &settings).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("hooks/register.ts")).unwrap(),
        "user"
    );
    assert_eq!(root, json!({}));
}
