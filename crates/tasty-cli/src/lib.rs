//! Tasty CLI — clap subcommand surface + transport/request/run dispatch.
//!
//! Phase F.B.13-3 에서 본 바이너리 src/adapters/cli/ 의 전 내용을 흡수.

pub mod commands;
pub mod cwd_resolve;
pub mod dynamic;
pub mod format;
pub mod help;
pub mod plugin;
pub mod remote_browse;
pub mod request;
pub mod run;
pub mod ssh;
pub mod stream;
pub mod transport;

use clap::{Parser, Subcommand};

pub use commands::*;
pub use help::{format_parse_error, print_augmented_help, print_command_tree};
pub use run::{run_client, try_run_plugin_cli};

#[derive(Parser)]
#[command(
    name = "tasty",
    about = "GPU-accelerated terminal emulator for AI coding agents",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Custom port file path (for test isolation)
    #[arg(long)]
    pub port_file: Option<String>,

    /// Force GUI launch even inside a tasty terminal
    #[arg(long)]
    pub launch: bool,

    /// Run as headless terminal emulator (no GUI, IPC + PTY + plugin only).
    /// Default-features 빌드에서는 GUI 부팅을 skip. no-default-features 빌드는 항상 headless.
    #[arg(long, default_value_t = false)]
    pub headless: bool,

    /// Show all commands in a tree (use with -h)
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Enable input simulation IPC (debug builds only).
    /// Required for debug.inject_mouse, debug.inject_key, etc.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub enable_input_simulation: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new resource (window, workspace, tab, split)
    New {
        #[command(subcommand)]
        command: NewCommands,
    },
    /// Close a resource (tab, pane, surface)
    Close {
        #[command(subcommand)]
        command: CloseCommands,
    },
    /// List/query resources (workspaces, windows, tree, surfaces, panes, info, hooks, etc.)
    List {
        #[command(subcommand)]
        command: ListCommands,
    },
    /// Set/update resources (hook, mark, workspace, global-hook)
    Set {
        #[command(subcommand)]
        command: SetCommands,
    },
    /// Move/reorder a resource (tab, workspace)
    Move {
        #[command(subcommand)]
        command: MoveCommands,
    },
    /// Split a pane group or surface
    Split {
        /// Split level: pane (upper layout) or surface (lower layout)
        #[arg(long)]
        level: String,
        /// Target surface: numeric surface ID, "this" (TASTY_SURFACE_ID), or nickname
        #[arg(long)]
        target_surface: Option<String>,
        /// Target pane: numeric pane ID (only for --level pane)
        #[arg(long)]
        target_pane: Option<u32>,
        /// Split direction: vertical (default) or horizontal
        #[arg(long, default_value = "vertical")]
        direction: String,
        /// Surface type: terminal (default), markdown, explorer, html, image
        #[arg(long, default_value = "terminal")]
        r#type: String,
        /// Metadata JSON to set on the new surface (e.g. '{"nickname":"build"}')
        #[arg(long)]
        meta: Option<String>,
        /// Working directory (for terminal type)
        #[arg(long)]
        cwd: Option<String>,
        /// File path (for markdown/image type)
        #[arg(long)]
        file: Option<String>,
        /// Directory path (for explorer type)
        #[arg(long)]
        path: Option<String>,
        /// URL (for html type)
        #[arg(long)]
        url: Option<String>,
    },
    /// Remote (SSH) attach to a surface/workspace on another host — `remote attach`
    Remote {
        #[command(subcommand)]
        command: RemoteCommands,
    },
    /// Send text, key, or queue message
    Send {
        #[command(subcommand)]
        command: SendCommands,
    },
    /// Read from surface or queue
    Read {
        #[command(subcommand)]
        command: ReadCommands,
    },
    /// Create a notification
    Notify {
        /// Notification body
        body: String,
        /// Optional notification title
        #[arg(long, default_value = "Notification")]
        title: String,
    },
    /// Remove resources (hook, global-hook)
    Unset {
        #[command(subcommand)]
        command: UnsetCommands,
    },
    /// Manage per-surface metadata
    SurfaceMeta {
        #[command(subcommand)]
        command: SurfaceMetaCommands,
    },
    /// Check if a surface is currently typing (received key input within 5 seconds)
    IsTyping {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Spawn the PTY of a deferred surface (no-op if already running)
    ///
    /// Send commands auto-wake the target; use this only to start the PTY without sending input.
    Wake {
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Debug and diagnostic commands (IME simulation, raw key input, etc.) — debug builds only
    #[cfg(debug_assertions)]
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
    },
    /// Internal tools (SSH connection profiles, passkeys, etc.)
    Tool {
        #[command(subcommand)]
        command: ToolCommands,
    },
    /// Manage plugins (list, install, remove, enable, disable, logs)
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },
    /// Agent memory store (~/.tasty/memory.db) — persistent key-value
    Memory {
        #[command(subcommand)]
        command: MemoryCommands,
    },
    /// Structured output observer (parsers + sink fan-out)
    Output {
        #[command(subcommand)]
        command: OutputCommands,
    },
    /// Human-handoff approval gates (request/respond/await/cancel/list/get)
    Approval {
        #[command(subcommand)]
        command: ApprovalCommands,
    },
    /// Agent telemetry (record metrics, summary, timeseries, top-N)
    Telemetry {
        #[command(subcommand)]
        command: TelemetryCommands,
    },
    /// Agent collaboration primitives (task DAG; barrier/semaphore/lease/reducer/rate-limit follow)
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },
    /// File handler — 사용자 설정 (`~/.tasty/file-handlers.toml`) 재로드.
    FileHandler {
        #[command(subcommand)]
        command: FileHandlerCommands,
    },
    /// Manage layout presets (workspace / tab / pane).
    Preset {
        #[command(subcommand)]
        command: PresetCommands,
    },
    /// Manage workspace categories (sidebar folders) — list/create/rename/delete/move.
    WorkspaceCategory {
        #[command(subcommand)]
        command: WorkspaceCategoryCommands,
    },
    /// Print this instance's IPC port to stdout (first step of the auto remote
    /// port-discovery chain, `ssh host tasty port`). Reads the port file only — no IPC.
    Port,
}

#[cfg(test)]
mod attach_surface_tests {
    use super::*;
    use clap::Parser;

    /// 원격 attach 는 `tasty remote attach` 네임스페이스로 파싱된다.
    #[test]
    fn remote_attach_parses() {
        let cli =
            Cli::try_parse_from(["tasty", "remote", "attach", "5", "--ssh", "user@host"]).unwrap();
        let Some(Commands::Remote {
            command: RemoteCommands::Attach { surface, ssh, .. },
        }) = cli.command
        else {
            panic!("expected remote attach");
        };
        assert_eq!(surface, Some(5));
        assert_eq!(ssh.as_deref(), Some("user@host"));
    }

    /// `--profile` / `--into-gui` 등 원격 부분집합 플래그가 remote 네임스페이스에 있다.
    #[test]
    fn remote_attach_into_gui_parses() {
        let cli = Cli::try_parse_from([
            "tasty",
            "remote",
            "attach",
            "--profile",
            "gx10",
            "--workspace",
            "2",
            "--into-gui",
            "--target-port",
            "45123",
        ])
        .unwrap();
        let Some(Commands::Remote {
            command:
                RemoteCommands::Attach {
                    profile,
                    workspace,
                    into_gui,
                    target_port,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected remote attach");
        };
        assert_eq!(profile.as_deref(), Some("gx10"));
        assert_eq!(workspace, Some(2));
        assert!(into_gui);
        assert_eq!(target_port, Some(45123));
    }

    /// top-level `tasty attach` 는 release 표면에서 완전히 제거되었다.
    #[test]
    fn top_level_attach_removed() {
        assert!(Cli::try_parse_from(["tasty", "attach", "5"]).is_err());
    }

    /// remote attach 는 `--force-detach`(원격 클라이언트 attach 락 강제해제)를 갖는다.
    #[test]
    fn remote_attach_force_detach_parses() {
        let cli =
            Cli::try_parse_from(["tasty", "remote", "attach", "5", "--force-detach"]).unwrap();
        let Some(Commands::Remote {
            command: RemoteCommands::Attach { force_detach, .. },
        }) = cli.command
        else {
            panic!("expected remote attach");
        };
        assert!(force_detach);
    }

    /// remote attach 의 런타임 가드: `--ssh` 와 `--force-detach` 는 상호배타.
    #[test]
    fn remote_attach_ssh_force_detach_rejected() {
        let cli = Cli::try_parse_from([
            "tasty",
            "remote",
            "attach",
            "5",
            "--ssh",
            "h",
            "--force-detach",
        ])
        .unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("--ssh 와 --force-detach"),
            "unexpected error: {err}"
        );
    }

    /// `tasty remote check --ssh user@host` 가 Check 변형으로 파싱된다(기본값 포함).
    #[test]
    fn remote_check_parses() {
        let cli = Cli::try_parse_from(["tasty", "remote", "check", "--ssh", "user@host"]).unwrap();
        let Some(Commands::Remote {
            command:
                RemoteCommands::Check {
                    ssh,
                    profile,
                    remote_tasty,
                    remote_port_mode,
                },
        }) = cli.command
        else {
            panic!("expected remote check");
        };
        assert_eq!(ssh.as_deref(), Some("user@host"));
        assert_eq!(profile, None);
        // attach 와 동일한 기본값.
        assert_eq!(remote_tasty, "tasty");
        assert_eq!(remote_port_mode, "auto");
    }

    /// `remote check --profile <name>` + 발견 모드 오버라이드가 파싱된다.
    #[test]
    fn remote_check_profile_parses() {
        let cli = Cli::try_parse_from([
            "tasty",
            "remote",
            "check",
            "--profile",
            "gx10",
            "--remote-port-mode",
            "file-unix",
        ])
        .unwrap();
        let Some(Commands::Remote {
            command:
                RemoteCommands::Check {
                    profile,
                    remote_port_mode,
                    ..
                },
        }) = cli.command
        else {
            panic!("expected remote check");
        };
        assert_eq!(profile.as_deref(), Some("gx10"));
        assert_eq!(remote_port_mode, "file-unix");
    }

    /// 런타임 가드: `remote check` 의 `--ssh` 와 `--profile` 는 상호배타.
    #[test]
    fn remote_check_ssh_profile_rejected() {
        let cli = Cli::try_parse_from(["tasty", "remote", "check", "--ssh", "h", "--profile", "p"])
            .unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("--ssh 와 --profile"),
            "unexpected error: {err}"
        );
    }

    /// 런타임 가드: 대상(`--ssh`/`--profile`) 없이 `remote check` → 명확한 거부.
    #[test]
    fn remote_check_no_target_rejected() {
        let cli = Cli::try_parse_from(["tasty", "remote", "check"]).unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("원격 check 대상이 필요합니다"),
            "unexpected error: {err}"
        );
    }

    /// `tasty remote workspaces --ssh user@host` 가 Workspaces 변형으로 파싱된다.
    #[test]
    fn remote_workspaces_parses() {
        let cli = Cli::try_parse_from([
            "tasty",
            "remote",
            "workspaces",
            "--ssh",
            "user@host",
            "--json",
        ])
        .unwrap();
        let Some(Commands::Remote {
            command:
                RemoteCommands::Workspaces {
                    ssh,
                    profile,
                    remote_tasty,
                    remote_port_mode,
                    json,
                },
        }) = cli.command
        else {
            panic!("expected remote workspaces");
        };
        assert_eq!(ssh.as_deref(), Some("user@host"));
        assert_eq!(profile, None);
        assert_eq!(remote_tasty, "tasty");
        assert_eq!(remote_port_mode, "auto");
        assert!(json);
    }

    /// 런타임 가드: `remote workspaces` 의 `--ssh` 와 `--profile` 는 상호배타.
    #[test]
    fn remote_workspaces_ssh_profile_rejected() {
        let cli = Cli::try_parse_from([
            "tasty",
            "remote",
            "workspaces",
            "--ssh",
            "h",
            "--profile",
            "p",
        ])
        .unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("--ssh 와 --profile"),
            "unexpected error: {err}"
        );
    }

    /// 런타임 가드: 대상(`--ssh`/`--profile`) 없이 `remote workspaces` → 명확한 거부.
    #[test]
    fn remote_workspaces_no_target_rejected() {
        let cli = Cli::try_parse_from(["tasty", "remote", "workspaces"]).unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("원격 대상이 필요합니다"),
            "unexpected error: {err}"
        );
    }

    /// 존재하지 않는 프로필 `remote check --profile nope` → "찾을 수 없습니다".
    #[test]
    fn remote_check_unknown_profile_rejected() {
        let cli =
            Cli::try_parse_from(["tasty", "remote", "check", "--profile", "__nope__"]).unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("찾을 수 없습니다"),
            "unexpected error: {err}"
        );
    }

    /// 로컬 loopback attach 는 debug 빌드 `tasty debug attach` 로만 파싱된다.
    #[cfg(debug_assertions)]
    #[test]
    fn debug_attach_parses() {
        let cli = Cli::try_parse_from(["tasty", "debug", "attach", "5", "--raw"]).unwrap();
        let Some(Commands::Debug {
            command: DebugCommands::Attach { surface, raw, .. },
        }) = cli.command
        else {
            panic!("expected debug attach");
        };
        assert_eq!(surface, Some(5));
        assert!(raw);
    }

    /// debug 로컬 attach 에는 ssh/profile 같은 원격 플래그가 없다.
    #[cfg(debug_assertions)]
    #[test]
    fn debug_attach_has_no_ssh() {
        assert!(Cli::try_parse_from(["tasty", "debug", "attach", "5", "--ssh", "h"]).is_err());
    }

    /// remote attach 의 런타임 가드: 원격 대상(--ssh/--profile) 없이는 거부된다
    /// (로컬 attach 로 폴백하지 않는다 — 로컬은 debug 빌드 전용).
    #[test]
    fn remote_attach_without_target_rejected() {
        let cli = Cli::try_parse_from(["tasty", "remote", "attach", "5"]).unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("원격 attach 대상이 필요"),
            "unexpected error: {err}"
        );
    }

    /// remote attach 의 런타임 가드: surface 와 --workspace 는 상호배타.
    #[test]
    fn remote_attach_surface_workspace_exclusive() {
        let cli = Cli::try_parse_from([
            "tasty",
            "remote",
            "attach",
            "5",
            "--workspace",
            "2",
            "--ssh",
            "h",
        ])
        .unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("함께 쓸 수 없습니다"),
            "unexpected error: {err}"
        );
    }

    /// remote attach 의 런타임 가드: --ssh 와 --profile 은 상호배타.
    #[test]
    fn remote_attach_ssh_profile_exclusive() {
        let cli = Cli::try_parse_from([
            "tasty",
            "remote",
            "attach",
            "5",
            "--ssh",
            "h",
            "--profile",
            "p",
        ])
        .unwrap();
        let err = run::run_client(cli.command.unwrap(), None).unwrap_err();
        assert!(
            err.to_string().contains("--ssh 와 --profile"),
            "unexpected error: {err}"
        );
    }
}

#[cfg(test)]
mod workspace_category_tests {
    use super::*;
    use crate::request::command_to_request;
    use clap::Parser;

    fn req(args: &[&str]) -> tasty_ipc::protocol::JsonRpcRequest {
        let cli = Cli::try_parse_from(args).unwrap();
        command_to_request(&cli.command.unwrap())
    }

    #[test]
    fn category_list_maps_to_ipc() {
        let r = req(&["tasty", "workspace-category", "list"]);
        assert_eq!(r.method, "workspace_category.list");
    }

    #[test]
    fn category_create_maps_to_ipc() {
        let r = req(&["tasty", "workspace-category", "create", "--name", "Work"]);
        assert_eq!(r.method, "workspace_category.create");
        assert_eq!(r.params["name"], "Work");
    }

    #[test]
    fn category_rename_delete_move_map() {
        let r = req(&[
            "tasty",
            "workspace-category",
            "rename",
            "--id",
            "3",
            "--name",
            "Play",
        ]);
        assert_eq!(r.method, "workspace_category.rename");
        assert_eq!(r.params["id"], 3);
        assert_eq!(r.params["name"], "Play");

        let r = req(&["tasty", "workspace-category", "delete", "--id", "3"]);
        assert_eq!(r.method, "workspace_category.delete");
        assert_eq!(r.params["id"], 3);

        let r = req(&[
            "tasty",
            "workspace-category",
            "move",
            "--from",
            "2",
            "--to",
            "1",
        ]);
        assert_eq!(r.method, "workspace_category.move");
        assert_eq!(r.params["from_index"], 2);
        assert_eq!(r.params["to_index"], 1);
    }

    #[test]
    fn new_workspace_carries_category() {
        let r = req(&[
            "tasty",
            "new",
            "workspace",
            "--name",
            "x",
            "--category",
            "Work",
        ]);
        assert_eq!(r.method, "workspace.create");
        assert_eq!(r.params["category"], "Work");
    }

    #[test]
    fn set_workspace_carries_category() {
        let r = req(&["tasty", "set", "workspace", "--id", "2", "--category", "5"]);
        assert_eq!(r.method, "workspace.update");
        assert_eq!(r.params["category"], "5");
    }
}
