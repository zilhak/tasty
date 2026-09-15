//! Separate processes keep host CWD and SDK environment changes out of the test runner.
use std::{ffi::OsStr, path::Path, process::Command};

use super::command;
use tasty_plugin_manifest::PluginPackage;

const TEST: &str = "process::launch::tests::relative_install_matrix";
const ID: &str = "com.example.launch";

fn package(dir: &Path, entry: &str) -> PluginPackage {
    PluginPackage {
        dir: dir.to_owned(),
        manifest: toml::from_str(&format!(
            "manifest_version=1\nid='{ID}'\nname='Launch'\nversion='1.0.0'\napi_version='1'\n[entry]\ntype='process'\ncommand='{entry}'\n"
        )).unwrap(),
    }
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn isolate(cmd: &mut Command) {
    let explicit: std::collections::HashSet<_> =
        cmd.get_envs().map(|(key, _)| key.to_owned()).collect();
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("TASTY_") && !explicit.contains(&key) {
            cmd.env_remove(key);
        }
    }
    cmd.args(["--exact", TEST, "--nocapture"]);
}

#[test]
fn relative_install_matrix() {
    match std::env::var("INSTALL_TEST_MODE").as_deref() {
        Ok("sdk") => return sdk(),
        Ok("host") => return host(),
        _ => {}
    }
    let root = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    isolate(&mut cmd);
    let output = cmd
        .current_dir(root.path())
        .env("INSTALL_TEST_MODE", "host")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn sdk() {
    let env = tasty_plugin_sdk::PluginEnv::load().unwrap();
    assert!(env.plugin_dir.as_ref().unwrap().is_absolute());
    let catalog = tasty_plugin_sdk::i18n::Translator::from_plugin_env(&env);
    assert_eq!(
        catalog.t("launch.label"),
        std::env::var("INSTALL_EXPECTED").unwrap()
    );
    assert_eq!(catalog.t("launch.fallback"), "English fallback");
    assert_eq!(catalog.t("launch.empty"), "Installed empty fallback");
    assert_eq!(
        std::env::current_dir().unwrap(),
        std::fs::canonicalize(std::env::var_os("INSTALL_CWD").unwrap()).unwrap()
    );
}

fn host() {
    let root = std::env::current_dir().unwrap();
    let install = root.join("home/plugins/launch");
    let other = root.join("other");
    std::fs::create_dir_all(install.join("bin")).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let binary = install.join(format!("probe{}", std::env::consts::EXE_SUFFIX));
    let relative_binary = install.join("bin").join(binary.file_name().unwrap());
    for dest in [&binary, &relative_binary] {
        #[cfg(unix)]
        std::os::unix::fs::symlink(std::env::current_exe().unwrap(), dest).unwrap();
        #[cfg(not(unix))]
        std::fs::copy(std::env::current_exe().unwrap(), dest).unwrap();
    }
    for locale in ["en", "ko", "ja", "zz"] {
        write(
            &install.join(format!("lang/{locale}.toml")),
            &format!(
                "[launch]\nlabel='installed {locale}'\nempty='Installed empty fallback'\n{}",
                if locale == "en" {
                    "fallback='English fallback'\n"
                } else {
                    ""
                }
            ),
        );
    }
    let mut matrix = Vec::new();
    for absolute in [false, true] {
        let dir = if absolute {
            install.clone()
        } else {
            Path::new("home/plugins/launch").to_owned()
        };
        for entry in [
            binary.file_name().unwrap().to_str().unwrap().to_owned(),
            format!("bin/{}", binary.file_name().unwrap().to_str().unwrap()),
            binary.to_string_lossy().into_owned(),
        ] {
            let pkg = package(&dir, &entry);
            for cwd in [&install, &other] {
                for shadow in [false, true] {
                    for locale in ["en", "ko", "ja", "zz", "missing"] {
                        let effective = if locale == "missing" { "en" } else { locale };
                        for user in ["missing", "empty", "override"] {
                            let user_path = tasty_i18n::plugin_catalog::override_path(
                                &root.join("home/lang"),
                                ID,
                                effective,
                            )
                            .unwrap();
                            if user == "missing" {
                                if user_path.exists() {
                                    std::fs::remove_file(&user_path).unwrap();
                                }
                            } else {
                                let value = if user == "empty" {
                                    "  "
                                } else {
                                    "User override"
                                };
                                write(
                                    &user_path,
                                    &format!("[launch]\nlabel='{value}'\nempty=''\n"),
                                );
                            }
                            let wrong_install = cwd.join("home/plugins/launch/lang");
                            let wrong_user = tasty_i18n::plugin_catalog::override_path(
                                &cwd.join("home/lang"),
                                ID,
                                effective,
                            )
                            .unwrap();
                            if shadow {
                                write(
                                    &wrong_install.join(format!("{effective}.toml")),
                                    "[launch]\nlabel='WRONG INSTALL'\nfallback='WRONG FALLBACK'\n",
                                );
                                write(&wrong_user, "[launch]\nlabel='WRONG USER'\n");
                            } else {
                                if wrong_install.exists() {
                                    std::fs::remove_dir_all(&wrong_install).unwrap();
                                }
                                if wrong_user.exists() {
                                    std::fs::remove_file(&wrong_user).unwrap();
                                }
                            }
                            let expected = if user == "override" {
                                "User override".to_owned()
                            } else {
                                format!("installed {effective}")
                            };
                            let host_catalog = tasty_i18n::plugin_catalog::load(
                                &dir.join("lang"),
                                effective,
                                ID,
                                Some(&root.join("home/lang")),
                            );
                            assert_eq!(host_catalog.get("launch.label"), Some(&expected));
                            let mut cmd = command(&pkg).unwrap();
                            assert!(Path::new(cmd.get_program()).is_absolute());
                            assert_eq!(cmd.get_current_dir(), Some(install.as_path()));
                            isolate(&mut cmd);
                            cmd.current_dir(cwd)
                                .env("INSTALL_TEST_MODE", "sdk")
                                .env("INSTALL_EXPECTED", &expected)
                                .env("INSTALL_CWD", cwd)
                                .env("TASTY_PARENT_HOME", root.join("home"))
                                .env("TASTY_PLUGIN_ID", ID)
                                .env("TASTY_PLUGIN_TOKEN", "test")
                                .env("TASTY_HOST_IPC_PORT", "1")
                                .env("TASTY_LOCALE", effective);
                            let output = cmd.output().unwrap();
                            assert!(
                                output.status.success(),
                                "{dir:?} {entry} {locale} {user}: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                            matrix.push(serde_json::json!({"absolute_install":absolute,"entry":entry,"cwd":cwd,"shadow":shadow,"requested_locale":locale,"effective":effective,"user":user,"expected":expected,"exit":output.status.code()}));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(matrix.len(), 360);
    // Missing paths still construct successfully; existence is judged by spawn/entry lookup.
    for entry in ["not-installed-probe", "bin/not-installed-probe"] {
        let cmd = command(&package(Path::new("does-not-exist"), entry)).unwrap();
        assert_eq!(cmd.get_program(), OsStr::new(entry));
        assert!(cmd.get_current_dir().unwrap().is_absolute());
    }
    let mut missing = command(&package(
        Path::new("does-not-exist"),
        binary.to_str().unwrap(),
    ))
    .unwrap();
    assert_eq!(
        missing.spawn().unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    if let Some(path) = std::env::var_os("INSTALL_MATRIX_PATH") {
        std::fs::write(path, serde_json::to_vec_pretty(&matrix).unwrap()).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn path_fallback_and_missing_or_nonexecutable_entries_keep_os_errors() {
    let root = tempfile::tempdir().unwrap();
    let mut shell = command(&package(root.path(), "sh")).unwrap();
    assert_eq!(shell.get_program(), OsStr::new("sh"));
    assert!(
        shell
            .env("PATH", "/bin:/usr/bin")
            .args(["-c", "exit 0"])
            .status()
            .unwrap()
            .success()
    );
    for entry in ["missing-launch-probe", "bin/missing-launch-probe"] {
        let mut cmd = command(&package(root.path(), entry)).unwrap();
        assert_eq!(cmd.get_program(), OsStr::new(entry));
        assert_eq!(
            cmd.env("PATH", root.path()).spawn().unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }
    let missing_absolute = root.path().join("missing-absolute");
    let mut cmd = command(&package(root.path(), missing_absolute.to_str().unwrap())).unwrap();
    assert_eq!(cmd.get_program(), missing_absolute.as_os_str());
    assert_eq!(
        cmd.spawn().unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    write(
        &root.path().join("not-executable"),
        "no executable permission",
    );
    assert_eq!(
        command(&package(root.path(), "not-executable"))
            .unwrap()
            .spawn()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::PermissionDenied
    );
    // The command retains the symbolic installation name; only the OS resolves it.
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(root.path(), &alias).unwrap();
    let cmd = command(&package(&alias, "/bin/sh")).unwrap();
    assert_eq!(cmd.get_current_dir(), Some(alias.as_path()));
}
