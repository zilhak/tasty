//! Separate processes keep the i18n OnceLock and plugin home isolated.
use std::{path::Path, process::Command};

fn write(path: &Path, value: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, value).unwrap();
}

#[test]
fn host_and_plugin_help_use_the_selected_catalog() {
    if let Ok(locale) = std::env::var("CLI_HELP_TEST_LOCALE") {
        let report = tasty_i18n::init(&locale);
        let locale = report.effective;
        if locale == "en" {
            use clap::CommandFactory;
            assert_eq!(
                tasty_cli::localized_command()
                    .render_long_help()
                    .to_string(),
                tasty_cli::Cli::command().render_long_help().to_string()
            );
        }
        let home = tasty_utils::path::tasty_home().unwrap();
        let entries = tasty_cli::dynamic::discover_plugin_clis(&home.join("plugins"));
        assert_eq!(entries.len(), 1);
        let mut cmd = tasty_cli::dynamic::build_augmented_cli(&entries);
        let root = cmd.render_long_help().to_string();
        let label = tasty_i18n::t("cli.help_frame.commands");
        assert!(root.contains(label), "{root}");
        let sub = cmd.find_subcommand_mut("fixture").unwrap();
        let top = sub.render_long_help().to_string();
        assert!(top.contains("USER TOP"), "{top}");
        let leaf = sub.find_subcommand_mut("show").unwrap();
        let help = leaf.render_long_help().to_string();
        assert!(help.contains("USER SUB"), "{help}");
        assert!(help.contains("USER ARG"), "{help}");
        assert!(help.contains("legacy fallback"), "{help}");
        assert!(help.contains(&format!("installed {locale}")), "{help}");
        let args = ["tasty", "fixture", "show", "--translated", "value"];
        let matches = cmd.try_get_matches_from(args).unwrap();
        assert_eq!(matches.subcommand().unwrap().0, "fixture");
        let mut host = tasty_cli::localized_command();
        let completion = host
            .find_subcommand_mut("surface")
            .unwrap()
            .find_subcommand_mut("completion")
            .unwrap();
        assert_eq!(
            completion.get_long_about().unwrap().to_string(),
            tasty_i18n::t("cli.help._root.surface.completion.long")
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let plugin = home.join("plugins/com.example.fixture");
    write(
        &plugin.join("tasty-plugin.toml"),
        r#"
manifest_version = 1
id = "com.example.fixture"
name = "Fixture"
version = "1.0.0"
api_version = "1"
lang_dir = "translations"
[entry]
type = "process"
command = "never-spawned"
[[contributes.ipc_namespace]]
prefix = "fixture"
[[contributes.cli]]
name = "fixture"
description = "legacy top"
description_i18n_key = "fixture.top"
subcommands = [{name="show", ipc_method="fixture.show", args="show", description="legacy sub", description_i18n_key="fixture.sub"}]
[contributes.cli.arg_groups.show]
flags = [
 {name="translated", type="string", flag="--translated", help="legacy arg", help_i18n_key="fixture.arg"},
 {name="fallback", type="string", flag="--fallback", help="legacy fallback", help_i18n_key="fixture.absent"},
 {name="installed", type="string", flag="--installed", help="legacy installed", help_i18n_key="fixture.installed"}
]
"#,
    );
    write(&home.join("lang/zz/pack.toml"), "[font]\nbuiltin=true\n");
    for requested in ["en", "ko", "ja", "zz", "missing"] {
        let locale = if requested == "missing" {
            "en"
        } else {
            requested
        };
        write(
            &plugin.join(format!("translations/{locale}.toml")),
            &format!(
                "[fixture]\ntop='installed top'\nsub='installed sub'\narg='installed arg'\ninstalled='installed {locale}'\n"
            ),
        );
        write(
            &home.join(if locale == "zz" {
                "lang/zz/plugins/com.example.fixture.toml".to_owned()
            } else {
                format!("lang/plugins/com.example.fixture/{locale}.toml")
            }),
            "[fixture]\ntop='USER TOP'\nsub='USER SUB'\narg='USER ARG'\ninstalled='  '\n",
        );
        let mut child = Command::new(std::env::current_exe().unwrap());
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("TASTY_") {
                child.env_remove(key);
            }
        }
        let output = child
            .args([
                "--exact",
                "host_and_plugin_help_use_the_selected_catalog",
                "--nocapture",
            ])
            .env("CLI_HELP_TEST_LOCALE", requested)
            .env("TASTY_HOME", home)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{locale}\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
