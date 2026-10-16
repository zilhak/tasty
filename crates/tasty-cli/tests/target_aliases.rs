//! 대상 옵션의 추천 표기와 호환 표기가 같은 요청이 되고, 타입·필수성·중복 거절은 그대로인지 확인한다.
//! 서버에 연결하지 않고 파서와 요청 변환만 사용한다.

use clap::Parser;
use tasty_cli::Cli;

fn request(args: &[&str]) -> (String, serde_json::Value) {
    let cli = Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
    let req = tasty_cli::request::command_to_request(&cli.command.expect("subcommand"));
    (req.method, req.params)
}

fn rejected(args: &[&str]) -> clap::error::ErrorKind {
    match Cli::try_parse_from(args) {
        Ok(_) => panic!("{args:?} was accepted"),
        Err(e) => e.kind(),
    }
}

#[test]
fn recommended_and_compatible_names_build_the_same_request() {
    let pairs: &[(&[&str], &[&str])] = &[
        (
            &["tasty", "close", "workspace", "--id", "7"],
            &["tasty", "close", "workspace", "--workspace", "7"],
        ),
        (
            &["tasty", "close", "window", "--id", "7"],
            &["tasty", "close", "window", "--window", "7"],
        ),
        (
            &["tasty", "agent", "task-list", "--workspace-id", "7"],
            &["tasty", "agent", "task-list", "--workspace", "7"],
        ),
        (
            &["tasty", "agent", "dag-list", "--workspace-id", "7"],
            &["tasty", "agent", "dag-list", "--workspace", "7"],
        ),
        (
            &[
                "tasty",
                "split",
                "--level",
                "surface",
                "--target-surface",
                "this",
            ],
            &["tasty", "split", "--level", "surface", "--surface", "this"],
        ),
        (
            &[
                "tasty",
                "approval",
                "request",
                "--title",
                "t",
                "--workspace-id",
                "7",
                "--surface-id",
                "9",
            ],
            &[
                "tasty",
                "approval",
                "request",
                "--title",
                "t",
                "--workspace",
                "7",
                "--surface",
                "9",
            ],
        ),
        (
            &["tasty", "approval", "list", "--workspace-id", "7"],
            &["tasty", "approval", "list", "--workspace", "7"],
        ),
        (
            &["tasty", "approval", "history", "--workspace-id", "7"],
            &["tasty", "approval", "history", "--workspace", "7"],
        ),
        (
            &["tasty", "approval", "summary", "get", "--workspace-id", "7"],
            &["tasty", "approval", "summary", "get", "--workspace", "7"],
        ),
        (
            &[
                "tasty",
                "approval",
                "summary",
                "set",
                "--workspace-id",
                "7",
                "--content",
                "x",
            ],
            &[
                "tasty",
                "approval",
                "summary",
                "set",
                "--workspace",
                "7",
                "--content",
                "x",
            ],
        ),
        (
            &["tasty", "split", "--level", "pane", "--target-pane", "3"],
            &["tasty", "split", "--level", "pane", "--pane", "3"],
        ),
        (
            &[
                "tasty",
                "pty",
                "attach-surface",
                "--pty-id",
                "2",
                "--pane-id",
                "3",
            ],
            &[
                "tasty",
                "pty",
                "attach-surface",
                "--pty",
                "2",
                "--pane",
                "3",
            ],
        ),
    ];
    for (old, new) in pairs {
        assert_eq!(request(old), request(new), "{old:?} / {new:?}");
    }
    // 생략 가능한 인자는 생략 의미를 그대로 둔다.
    assert_eq!(
        request(&["tasty", "agent", "dag-list"]),
        request(&["tasty", "agent", "dag-list"])
    );
    let (_, params) = request(&["tasty", "approval", "list"]);
    assert!(params.get("workspace_id").is_none(), "{params}");
}

#[test]
fn giving_both_names_is_refused_before_any_request() {
    use clap::error::ErrorKind;
    for args in [
        &[
            "tasty",
            "close",
            "workspace",
            "--id",
            "7",
            "--workspace",
            "8",
        ][..],
        &["tasty", "close", "window", "--id", "7", "--window", "7"],
        &[
            "tasty",
            "agent",
            "task-list",
            "--workspace-id",
            "7",
            "--workspace",
            "8",
        ],
        &[
            "tasty",
            "approval",
            "request",
            "--title",
            "t",
            "--surface-id",
            "1",
            "--surface",
            "2",
        ],
        &[
            "tasty",
            "approval",
            "summary",
            "get",
            "--workspace-id",
            "1",
            "--workspace",
            "2",
        ],
        &[
            "tasty",
            "split",
            "--level",
            "pane",
            "--target-pane",
            "1",
            "--pane",
            "2",
        ],
        &[
            "tasty",
            "pty",
            "attach-surface",
            "--pty-id",
            "1",
            "--pty",
            "2",
            "--pane",
            "3",
        ],
    ] {
        assert_eq!(rejected(args), ErrorKind::ArgumentConflict, "{args:?}");
    }
}

#[test]
fn types_and_required_flags_stay_as_declared() {
    use clap::error::ErrorKind;
    // workspace·pane ID는 u32, window ID는 u64다.
    for args in [
        &["tasty", "close", "workspace", "--workspace", "4294967296"][..],
        &["tasty", "close", "workspace", "--workspace", "-1"],
        &["tasty", "close", "workspace", "--workspace", "main"],
        &[
            "tasty",
            "close",
            "window",
            "--window",
            "18446744073709551616",
        ],
        &["tasty", "agent", "task-list", "--workspace", "4294967296"],
        &["tasty", "split", "--level", "pane", "--pane", "abc"],
        &["tasty", "approval", "list", "--workspace", "4294967296"],
    ] {
        let kind = rejected(args);
        assert!(
            matches!(
                kind,
                ErrorKind::ValueValidation | ErrorKind::UnknownArgument
            ),
            "{args:?}: {kind:?}"
        );
    }
    let (method, params) = request(&["tasty", "close", "window", "--window", "4294967296"]);
    assert_eq!(method, "window.close");
    assert_eq!(params["id"], serde_json::json!(4294967296u64));
    // 필수 인자는 여전히 필수다.
    assert_eq!(
        rejected(&["tasty", "agent", "task-list"]),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        rejected(&["tasty", "close", "workspace"]),
        ErrorKind::MissingRequiredArgument
    );
    assert_eq!(
        rejected(&["tasty", "approval", "summary", "get"]),
        ErrorKind::MissingRequiredArgument
    );
}

#[test]
fn help_shows_the_recommended_name_with_the_compatible_one() {
    let mut root = tasty_cli::localized_command();
    let close = root
        .find_subcommand_mut("close")
        .unwrap()
        .find_subcommand_mut("workspace")
        .unwrap();
    let help = close.render_help().to_string();
    assert!(help.contains("--workspace <ID>"), "{help}");
    assert!(help.contains("--id"), "{help}");
}

#[test]
fn approval_help_shows_the_recommended_names_with_the_compatible_ones() {
    let mut root = tasty_cli::localized_command();
    let request = root
        .find_subcommand_mut("approval")
        .unwrap()
        .find_subcommand_mut("request")
        .unwrap();
    let help = request.render_help().to_string();
    for name in [
        "--workspace <",
        "--workspace-id",
        "--surface <",
        "--surface-id",
    ] {
        assert!(help.contains(name), "{name}: {help}");
    }
}
