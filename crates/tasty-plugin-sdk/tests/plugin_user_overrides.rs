//! Real host and SDK entry points run in isolated subprocesses, so the host's
//! OnceLock and parent-home environment cannot affect another test or instance.

use std::path::Path;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn host_and_sdk_read_the_same_user_file() {
    if let Ok(root) = std::env::var("LANGPACK_TEST_ROOT") {
        let root = Path::new(&root);
        let code = std::env::var("LANGPACK_TEST_CODE").unwrap();
        tasty_i18n::init(&code);
        let env = tasty_plugin_sdk::PluginEnv::load().unwrap();
        tasty_i18n::register_namespace(&env.plugin_id, &root.join("plugin/lang"));
        let sdk = tasty_plugin_sdk::i18n::Translator::from_plugin_env(&env);
        let expected = std::env::var("LANGPACK_TEST_EXPECTED").unwrap();
        assert_eq!(tasty_i18n::t("viewer.label"), expected);
        assert_eq!(sdk.t("viewer.label"), expected);
        assert_eq!(tasty_i18n::t("viewer.internal"), expected);
        assert_eq!(sdk.t("viewer.internal"), expected);
        let base = if code == "ko" {
            "installed ko"
        } else {
            "installed en"
        };
        assert_eq!(tasty_i18n::t("viewer.kept"), base);
        assert_eq!(sdk.t("viewer.kept"), base);
        assert_eq!(sdk.t("viewer.english_only"), "English fallback");
        assert_eq!(tasty_i18n::t("viewer.english_only"), "English fallback");
        assert_eq!(tasty_i18n::t("viewer.missing"), "viewer.missing");
        assert_eq!(sdk.t("viewer.missing"), "viewer.missing");
        tasty_i18n::unregister_namespace(&env.plugin_id);
        assert_eq!(tasty_i18n::t("viewer.label"), "viewer.label");
        return;
    }

    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("host");
    let langs = home.join("lang");
    write(
        &root.path().join("plugin/lang/en.toml"),
        "[viewer]\nlabel='installed en'\ninternal='installed en'\nkept='installed en'\nenglish_only='English fallback'\n",
    );
    write(
        &root.path().join("plugin/lang/ko.toml"),
        "[viewer]\nlabel='installed ko'\ninternal='installed ko'\nkept='installed ko'\nenglish_only='  '\n",
    );
    write(&langs.join("zz/pack.toml"), "[font]\nbuiltin=true\n");
    // A tempting sibling must never affect this plugin's catalog.
    write(
        &langs.join("plugins/com.example.other/ko.toml"),
        "[viewer]\nlabel='wrong plugin'\n",
    );

    for code in ["en", "ko", "zz"] {
        let path =
            tasty_i18n::plugin_catalog::override_path(&langs, "com.example.viewer", code).unwrap();
        for (contents, expected) in [
            (
                None,
                if code == "ko" {
                    "installed ko"
                } else {
                    "installed en"
                },
            ),
            (
                Some("[viewer]\nlabel='user choice'\ninternal='user choice'\nkept='  '\n"),
                "user choice",
            ),
            (
                Some("[viewer]\nlabel='next restart'\ninternal='next restart'\nkept=''\n"),
                "next restart",
            ),
            (
                Some("not valid TOML ["),
                if code == "ko" {
                    "installed ko"
                } else {
                    "installed en"
                },
            ),
        ] {
            if let Some(contents) = contents {
                write(&path, contents);
            }
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "host_and_sdk_read_the_same_user_file",
                    "--nocapture",
                ])
                .env("LANGPACK_TEST_ROOT", root.path())
                .env("LANGPACK_TEST_CODE", code)
                .env("LANGPACK_TEST_EXPECTED", expected)
                .env("TASTY_HOME", &home)
                .env("TASTY_PARENT_HOME", &home)
                .env("TASTY_PLUGIN_ID", "com.example.viewer")
                .env("TASTY_PLUGIN_DIR", root.path().join("plugin"))
                .env("TASTY_PLUGIN_TOKEN", "fake-test-token")
                .env("TASTY_HOST_IPC_PORT", "1")
                .env("TASTY_LOCALE", code)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "code={code}, expected={expected}\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            tracing::info!("{code}: {expected} — host and SDK agree after process restart");
        }
    }
}

#[test]
fn sdk_without_a_parent_root_does_not_read_its_own_home() {
    if let Ok(root) = std::env::var("LANGPACK_NO_PARENT_ROOT") {
        let env = tasty_plugin_sdk::PluginEnv::load().unwrap();
        let sdk = tasty_plugin_sdk::i18n::Translator::from_plugin_env(&env);
        assert_eq!(sdk.t("viewer.label"), "installed");
        assert!(Path::new(&root).exists());
        return;
    }
    let root = tempfile::tempdir().unwrap();
    write(
        &root.path().join("plugin/lang/en.toml"),
        "[viewer]\nlabel='installed'\n",
    );
    write(
        &root.path().join("lang/plugins/com.example.viewer/en.toml"),
        "[viewer]\nlabel='wrong home'\n",
    );
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "sdk_without_a_parent_root_does_not_read_its_own_home",
        ])
        .env("LANGPACK_NO_PARENT_ROOT", root.path())
        .env("TASTY_HOME", root.path())
        .env_remove("TASTY_PARENT_HOME")
        .env("TASTY_PLUGIN_ID", "com.example.viewer")
        .env("TASTY_PLUGIN_DIR", root.path().join("plugin"))
        .env("TASTY_PLUGIN_TOKEN", "fake-test-token")
        .env("TASTY_HOST_IPC_PORT", "1")
        .env("TASTY_LOCALE", "en")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}
