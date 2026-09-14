//! Execute the production builders through real shells with hostile codex wrappers.
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn external_codex_preserves_argv_environment_and_prompt() {
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .without_time()
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let root = std::env::temp_dir().join(format!("tasty-codex-shell-{}", std::process::id()));
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
        // sh is required everywhere; additional shells are exercised when installed.
        if Command::new(shell).arg("-c").arg(":").status().is_err() {
            assert_ne!(shell, "sh", "POSIX sh is required");
            tracing::warn!("SKIP {shell}: shell not installed");
            continue;
        }
        for wrapper in ["alias", "function"] {
            let setup = if wrapper == "alias" {
                format!("alias codex='codex {bypass}'\n")
            } else {
                format!("codex() {{ command codex {bypass} \"$@\"; }}\n")
            };
            let run = |command: &str| {
                let mut process = Command::new(shell);
                if shell == "bash" {
                    process.args(["--noprofile", "--norc", "-O", "expand_aliases"]);
                } else if shell == "zsh" {
                    process.arg("-f");
                }
                let output = process
                    .args([
                        "-c",
                        &format!("{setup}eval \"$1\""),
                        "codex-shell-test",
                        command.trim_end_matches('\r'),
                    ])
                    .env("PATH", &path)
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
                expected.extend(
                    ["-c", "check_for_update_on_startup=false", "019f55e7-3dfa"].map(String::from),
                );
                assert_eq!(actual, expected, "{shell}/{wrapper} resume");
                executions += 1;
            }
            tracing::info!("PASS {shell}/{wrapper}: positive control + 12 builder executions");
        }
    }
    std::fs::remove_file(prompt_file::path_for(
        &std::env::temp_dir(),
        PROMPT_FILE_PREFIX,
        surface_id,
    ))
    .unwrap();
    std::fs::remove_dir_all(root).unwrap();
    assert!(executions >= 24);
    tracing::info!("PASS {executions} production builder executions");
}
