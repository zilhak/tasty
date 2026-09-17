//! Execute the production builders through real shells with hostile codex wrappers.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

// Only child startup configuration is isolated. Ordinary inherited environment
// remains available so the sentinel and surface-ID assertions still test it.
fn isolated_shell(shell: &str) -> Command {
    let mut process = Command::new(shell);
    for key in ["BASH_ENV", "ENV", "ZDOTDIR", "SHELLOPTS", "BASHOPTS"] {
        process.env_remove(key);
    }
    if shell == "bash" {
        process.args(["--noprofile", "--norc", "-O", "expand_aliases"]);
    } else if shell == "zsh" {
        process.arg("-f");
    }
    process
}

#[test]
fn external_codex_preserves_argv_environment_and_prompt() {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let prompt_dir = std::env::temp_dir();
    let root = prompt_dir.join(format!(
        "tasty-codex-shell-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let fake = root.join("codex");
    std::fs::write(
        &fake,
        "#!/bin/sh\nprintf '%s\\0' \"$TASTY_SURFACE_ID\" \"$CODEX_TEST_SENTINEL\" \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:/usr/bin:/bin", root.display());
    let bypass = "--dangerously-bypass-approvals-and-sandbox";
    let prompt = "한글 [!NOTE] 'quoted' \"double\" $HOME $(false) `false` \\path\nsecond line";
    let surface_id = 900_000_000 + std::process::id();
    let mut executions = 0;
    for shell in ["sh", "bash", "zsh"] {
        if !shell_is_installed(shell) {
            continue;
        }
        for wrapper in ["alias", "function"] {
            executions += exercise_wrapper(shell, wrapper, &path, bypass, prompt, surface_id);
            tracing::info!("PASS {shell}/{wrapper}: positive control + 12 builder executions");
        }
    }
    std::fs::remove_file(prompt_file::path_for(
        &prompt_dir,
        PROMPT_FILE_PREFIX,
        surface_id,
    ))
    .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(executions >= 24);
    tracing::info!("PASS {executions} production builder executions");
}

/// sh 는 어디에나 있어야 한다. 나머지 셸은 설치돼 있을 때만 돌린다 — 없는 것과
/// 있는데 못 뜨는 것은 다른 사실이라 뒤쪽은 그대로 터뜨린다.
fn shell_is_installed(shell: &str) -> bool {
    match isolated_shell(shell).args(["-c", ":"]).output() {
        Ok(output) => {
            assert!(output.status.success(), "{shell}: {output:?}");
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && shell != "sh" => {
            tracing::warn!("SKIP {shell}: shell not installed");
            false
        }
        Err(error) => panic!("cannot start {shell}: {error}"),
    }
}

/// 한 셸 × 한 래퍼 조합에서 프로덕션 빌더가 만든 명령줄을 전부 실행하고, 실행한
/// 횟수를 돌려준다. 그 수가 호출자의 하한 단정을 지탱한다 — 조합이 조용히 건너뛰어도
/// 합이 안 늘어 드러난다.
fn exercise_wrapper(
    shell: &str,
    wrapper: &str,
    path: &str,
    bypass: &str,
    prompt: &str,
    surface_id: u32,
) -> usize {
    let setup = if wrapper == "alias" {
        format!("alias codex='codex {bypass}'\n")
    } else {
        format!("codex() {{ command codex {bypass} \"$@\"; }}\n")
    };
    let run = |command: &str| {
        let mut process = isolated_shell(shell);
        let output = process
            .args([
                "-c",
                &format!("{setup}eval \"$1\""),
                "codex-shell-test",
                command.trim_end_matches('\r'),
            ])
            .env("PATH", path)
            .env("TASTY_SURFACE_ID", "parent-surface")
            .env("CODEX_TEST_SENTINEL", "inherited value $ ! 한글")
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell}: {output:?}");
        let mut fields: Vec<String> = output
            .stdout
            .split(|b| *b == 0)
            .map(|s| String::from_utf8(s.to_vec()).unwrap())
            .collect();
        assert_eq!(fields.pop().as_deref(), Some(""));
        fields
    };
    // Positive control: the same wrapper really injects the conflicting flag.
    assert!(run("codex -a never").iter().any(|a| a == bypass));
    let mut executions = 0;
    for policy in [
        "-a never",
        "-a on-request -s read-only",
        "-a untrusted -s workspace-write",
        bypass,
    ] {
        for initial_prompt in [None, Some(prompt)] {
            let command = make_codex_command(surface_id, initial_prompt, policy);
            let actual = run(&command);
            let mut expected = vec![
                surface_id.to_string(),
                "inherited value $ ! 한글".into(),
                "--dangerously-bypass-hook-trust".into(),
            ];
            expected.extend(policy.split_whitespace().map(String::from));
            if let Some(p) = initial_prompt {
                expected.push(p.into());
            }
            assert_eq!(actual, expected, "{shell}/{wrapper}: {command}");
            executions += 1;
        }
        let actual = run(&crate::reboot::resume_command("019f55e7-3dfa", policy));
        let mut expected = vec![
            "parent-surface".into(),
            "inherited value $ ! 한글".into(),
            "resume".into(),
            "--dangerously-bypass-hook-trust".into(),
        ];
        expected.extend(policy.split_whitespace().map(String::from));
        expected
            .extend(["-c", "check_for_update_on_startup=false", "019f55e7-3dfa"].map(String::from));
        assert_eq!(actual, expected, "{shell}/{wrapper} resume");
        executions += 1;
    }
    executions
}
