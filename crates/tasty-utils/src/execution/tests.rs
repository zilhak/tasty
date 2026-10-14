use super::*;

#[test]
fn relative_execution_context_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    assert!(ExecutionContext::from_parts("codex".into(), root.path().into()).is_err());
    assert!(ExecutionContext::from_parts(root.path().join("codex"), "relative".into()).is_err());
}

#[cfg(unix)]
#[test]
fn a_launcher_and_its_interpreter_use_the_captured_path_from_a_different_cwd() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("caller bin");
    let elsewhere = root.path().join("plugin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::create_dir(&elsewhere).unwrap();
    let launcher = bin.join("codex");
    let interpreter = bin.join("tasty-probe-node");
    std::fs::write(&launcher, "#!/usr/bin/env tasty-probe-node\n").unwrap();
    std::fs::write(
        &interpreter,
        "#!/bin/sh\nprintf '%s\\n' interpreter-found\n",
    )
    .unwrap();
    for p in [&launcher, &interpreter] {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let ctx = ExecutionContext::resolve("codex", OsStr::new("caller bin"), root.path()).unwrap();
    assert_eq!(ctx.executable, launcher);
    assert_eq!(ctx.search_path, bin.as_os_str());
    let output = ctx.command().current_dir(elsewhere).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(output.stdout, b"interpreter-found\n");
    std::fs::remove_file(&launcher).unwrap();
    assert!(
        ctx.command().output().is_err(),
        "missing captured executable must not fall back to another install"
    );
}

#[cfg(unix)]
#[test]
fn non_executable_files_are_skipped_in_search_order() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    for dir in [&first, &second] {
        std::fs::create_dir(dir).unwrap();
    }
    std::fs::write(first.join("codex"), "not executable").unwrap();
    std::fs::write(second.join("codex"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(second.join("codex"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = std::env::join_paths([&first, &second]).unwrap();
    assert_eq!(
        ExecutionContext::resolve("codex", &path, root.path())
            .unwrap()
            .executable,
        second.join("codex")
    );
}

#[cfg(windows)]
#[test]
fn windows_npm_cmd_launcher_is_found_without_shell_interpolation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("codex.cmd");
    std::fs::write(&path, "@echo off\r\n").unwrap();
    assert_eq!(
        ExecutionContext::resolve("codex", root.path().as_os_str(), root.path())
            .unwrap()
            .executable,
        path
    );
}
