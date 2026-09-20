//! Separate processes keep the i18n OnceLock and plugin home isolated.
use std::{path::Path, process::Command};

fn write(path: &Path, value: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, value).unwrap();
}

#[test]
fn host_and_plugin_help_use_the_selected_catalog() {
    if std::env::var("CLI_HELP_TEST_LOCALE").is_ok() {
        assert_child_side(&std::env::var("CLI_HELP_TEST_LOCALE").unwrap());
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
 {name="installed", type="string", flag="--installed", help="legacy installed", help_i18n_key="fixture.installed"},
 {name="nohelp", type="string", flag="--nohelp", default="7"},
 {name="emptyhelp", type="string", flag="--emptyhelp", help="", default="7"},
 {name="missinghelp", type="string", flag="--missinghelp", help_i18n_key="fixture.absent", default="7"}
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

fn assert_generated_help(cmd: clap::Command, args: &[&str]) {
    let error = cmd.try_get_matches_from(args).unwrap_err();
    assert_eq!(error.kind(), clap::error::ErrorKind::DisplayHelp);
    assert_eq!(error.exit_code(), 0);
    let text = error.to_string();
    for key in ["usage", "help_command"] {
        assert!(
            text.contains(tasty_i18n::t(&format!("cli.help_frame.{key}"))),
            "{args:?}: {text}"
        );
    }
    if matches!(tasty_i18n::current_language(), "ko" | "ja") {
        for english in ["Usage:", "Commands:", "Print this message"] {
            assert!(!text.contains(english), "{args:?}: {text}");
        }
    }
}

fn assert_annotation_matrix() {
    use clap::{Arg, Command};
    for help in [None, Some(""), Some("legacy missing translation")] {
        for long_help in [None, Some(""), Some("long missing translation")] {
            for default in [false, true] {
                for possible in [false, true] {
                    let mut arg = Arg::new("review_color").long("color");
                    if let Some(help) = help {
                        arg = arg.help(help);
                    }
                    if let Some(help) = long_help {
                        arg = arg.long_help(help);
                    }
                    if default {
                        arg = arg.default_value("red");
                    }
                    if possible {
                        arg = arg.value_parser(["red", "blue"]);
                    }
                    let original = Command::new("annotation-fixture").arg(arg);
                    let localized = tasty_cli::help_i18n::localize(original.clone());
                    assert_rendered_forms(&original, &localized, default, possible);
                    if default {
                        let matches = localized
                            .clone()
                            .try_get_matches_from(["annotation-fixture"])
                            .unwrap();
                        assert_eq!(matches.get_one::<String>("review_color").unwrap(), "red");
                    }
                    if possible {
                        let error = localized
                            .try_get_matches_from(["annotation-fixture", "--color", "green"])
                            .unwrap_err();
                        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
                    }
                }
            }
        }
    }
}

/// 자식 프로세스 쪽 단언 — 부모는 케이스를 돌리고, 실제 도움말 판정은 여기서 한다.
///
/// 한 함수에 두 갈래를 두면 읽는 사람이 매 줄마다 "이건 어느 쪽인가" 를 되묻게 된다.
fn assert_child_side(locale: &str) {
    let report = tasty_i18n::init(locale);
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
    for flag in ["nohelp", "emptyhelp", "missinghelp"] {
        let block = help
            .split(&format!("--{flag}"))
            .nth(1)
            .unwrap()
            .split("\n      --")
            .next()
            .unwrap();
        assert!(block.contains("7"), "{block}");
    }
    assert_generated_help(cmd.clone(), &["tasty", "help", "help"]);
    assert_generated_help(cmd.clone(), &["tasty", "fixture", "help", "help"]);
    assert_annotation_matrix();
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
}

/// 한 인자 조합에서 짧은/긴 도움말 두 형태를 모두 단언한다.
///
/// 케이스를 고르는 네 겹의 루프와 **한 케이스를 재는 일**은 서로 다른 관심사다.
fn assert_rendered_forms(
    original: &clap::Command,
    localized: &clap::Command,
    default: bool,
    possible: bool,
) {
    for long in [false, true] {
        let mut cmd = localized.clone();
        let text = if long {
            cmd.render_long_help()
        } else {
            cmd.render_help()
        }
        .to_string();
        if tasty_i18n::current_language() == "en" {
            let mut cmd = original.clone();
            let expected = if long {
                cmd.render_long_help()
            } else {
                cmd.render_help()
            }
            .to_string();
            assert_eq!(text, expected);
        }
        if default {
            assert!(text.contains("red"), "{text}");
        }
        if possible {
            assert!(text.contains("blue"), "{text}");
        }
        if tasty_i18n::current_language() != "en" {
            if default {
                assert!(
                    text.contains(&tasty_i18n::t_fmt("cli.help_frame.default", "red")),
                    "{text}"
                );
            }
            if possible {
                assert!(
                    text.contains(&tasty_i18n::t_fmt("cli.parse.valid_values", "red, blue")),
                    "{text}"
                );
            }
        }
    }
}
