//! CLI and host-shaped backend participation in isolated subprocesses, without app or SSH input.
use clap::Parser;
use std::{
    fs,
    process::{Command, Stdio},
};
use tasty_cli::{Cli, Commands, ToolCommands};
use tasty_remote_profiles::{ProfileCommand, RemoteFileStore, RemoteProfile};

#[test]
fn local_command_child() {
    let Some(role) = std::env::var_os("TASTY_BACKEND_TEST_ROLE") else {
        return;
    };
    let root = std::env::var_os("TASTY_HOME").expect("isolated home required");
    if role == "host" {
        let store = RemoteFileStore::at(root);
        for i in 0..8 {
            store
                .profile(
                    ProfileCommand::Put(
                        RemoteProfile::new(format!("host-{i}"), "ssh")
                            .with_field("host", "example.invalid"),
                    ),
                    None,
                )
                .unwrap();
        }
        return;
    }
    for i in 0..8 {
        run(&[
            "tool",
            "remote-profile",
            "add-ssh",
            "--name",
            &format!("cli-{i}"),
            "--host",
            "example.invalid",
            "--shell",
            "bash",
            "--identity",
            "fixture-only-key",
        ]);
        run(&[
            "tool",
            "passkey",
            "add",
            "--name",
            &format!("cli-{i}"),
            "--path",
            "fixture-only-key",
        ]);
    }
    run(&[
        "tool",
        "passkey",
        "add",
        "--name",
        "inline-key",
        "--inline",
        "--value",
        "fixture-only-payload",
    ]);
    run(&["tool", "passkey", "remove", "--name", "inline-key"]);
}
fn run(args: &[&str]) {
    let cli = Cli::try_parse_from(std::iter::once("tasty").chain(args.iter().copied())).unwrap();
    let Some(Commands::Tool { command }) = cli.command else {
        panic!("tool command required")
    };
    match command {
        ToolCommands::RemoteProfile { command } => {
            tasty_cli::local::remote_profile::run(&command).unwrap()
        }
        ToolCommands::Passkey { command } => tasty_cli::local::passkey::run(&command).unwrap(),
        _ => panic!("unexpected command"),
    }
}
#[test]
fn cli_and_host_shaped_processes_share_latest_registry_without_ssh_or_app() {
    let dir = tempfile::tempdir().unwrap();
    let spawn = |role: &str| {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "local_command_child", "--nocapture"])
            .env("TASTY_HOME", dir.path())
            .env("TASTY_BACKEND_TEST_ROLE", role)
            .env_remove("TASTY_PARENT_HOME")
            .env_remove("TASTY_SESSION_TOKEN")
            .output()
            .unwrap()
    };
    // Keep both participants alive concurrently rather than constructing stale in-memory stores.
    let exe = std::env::current_exe().unwrap();
    let mut host = Command::new(exe)
        .args(["--exact", "local_command_child", "--nocapture"])
        .env("TASTY_HOME", dir.path())
        .env("TASTY_BACKEND_TEST_ROLE", "host")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let cli = spawn("cli");
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert!(host.wait().unwrap().success());
    let p: toml::Value =
        toml::from_str(&fs::read_to_string(dir.path().join("remote-profiles.toml")).unwrap())
            .unwrap();
    assert_eq!(p["profile"].as_array().unwrap().len(), 16);
    let k: toml::Value =
        toml::from_str(&fs::read_to_string(dir.path().join("passkeys.toml")).unwrap()).unwrap();
    assert_eq!(k["passkey"].as_array().unwrap().len(), 16);
    assert!(!dir.path().join("passkeys/inline-key").exists());
    assert!(!String::from_utf8_lossy(&cli.stdout).contains("fixture-only-payload"));
}
