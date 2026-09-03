//! 명령이 어디로 가는지를 타입으로 결정한다.
//!
//! 이 크레이트에는 원래 **선언/전송 2갈래 대칭**이 있다 — `commands/`(clap 선언)
//! ↔ `request/`(JSON-RPC 변환). 그런데 단발 RPC 로 끝나지 않는 명령들은 그 대칭에서
//! 빠져 진입점의 조건 분기와 선언 계층 내부 실행 함수로 흩어져 있었다. 여기서
//! [`Dispatch`] 로 갈래를 명시해 **세 번째 갈래**(`local/` — 클라이언트 주도 실행)를
//! 대칭에 복귀시킨다.
//!
//! 분류 축은 하나다: **`request/` 가 만든 단발 JSON-RPC 하나로 끝나는가.**
//! 아니면(로컬 파일·프로세스 조작 · raw 스트림 · 폴링 루프 · SSH 터널 경유 조회)
//! 전부 "클라이언트가 주도해 여러 번 통신한다" 는 같은 성격이라 한 갈래로 묶인다.
//!
//! 새 클라이언트 주도 명령을 추가할 때 고칠 곳은 [`classify`] 하나다 — 진입점
//! (`run.rs`)은 열 필요가 없다.

use anyhow::Result;

use crate::Commands;
use crate::commands::{PluginCommands, RemoteCommands, ToolCommands};

/// 클라이언트 주도 실행이 진입점에서 받는 문맥.
///
/// 지금은 `--port-file` 오버라이드 하나다. 실측 근거: 기존 진입점 시그니처가
/// `run_client(command, port_file)` 이고, 분기 16개 중 포트 파일 밖의 값을 진입점
/// 에서 받아 쓰는 것이 없다(나머지는 각자 clap 인자에서 온다).
pub struct ClientCtx<'a> {
    pub port_file: Option<&'a str>,
}

/// 클라이언트가 주도하는 실행 하나.
///
/// `run` 이 `Box<Self>` 를 받는 것은 [`Dispatch::ClientDriven`] 이 트레잇 객체를
/// 담기 때문이다 — 소유권을 그대로 넘겨 내부 필드를 move 로 쓸 수 있다.
pub trait ClientCommand {
    fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()>;
}

/// 모든 CLI 명령은 둘 중 하나다.
///
/// 명령을 빌려온다 — 클라이언트 주도 실행 단위가 clap 이 이미 파싱해 둔 값을
/// 그대로 참조하면 복제도, clap enum 에 `Clone` 을 새로 다는 일도 없다. 단발 RPC
/// 갈래가 아무것도 담지 않는 것도 같은 이유다(진입점이 원 명령을 계속 들고 있다).
pub enum Dispatch<'a> {
    /// `request/` 가 만든 단발 JSON-RPC — 보내고 응답을 출력하면 끝.
    Rpc,
    /// 클라이언트가 주도하는 실행 — 로컬 파일/프로세스, raw 스트림, 폴링 루프,
    /// SSH 터널 경유 조회. 공통점은 "단발 RPC 가 아니다" 하나다.
    ClientDriven(Box<dyn ClientCommand + 'a>),
}

impl Commands {
    /// 이 명령이 어느 갈래인지 판정한다. 인자 조합 검증도 여기서 끝낸다 —
    /// 검증 실패는 통신을 시작하기 전에 나야 한다.
    pub fn dispatch(&self) -> Result<Dispatch<'_>> {
        match classify(self)? {
            Some(cmd) => Ok(Dispatch::ClientDriven(cmd)),
            None => Ok(Dispatch::Rpc),
        }
    }
}

/// 클라이언트 주도 명령이면 그 실행 단위를, 아니면 `None`(= 단발 RPC)을 돌려준다.
///
/// arm 순서가 의미를 갖는다 — 앞선 arm 이 이긴다. 특히 `Remote::Attach` 는 거부
/// 조합(`--ssh` + `--force-detach`)을 먼저 걸러야 그 아래 raw-stream arm 의 패턴
/// (`force_detach: false`)에 걸리지 않고 fall-through 하는 경로와 구분된다.
#[allow(clippy::too_many_lines)] // 평면 라우팅 표 — 쪼개면 "어디로 가는가" 를 한눈에 못 본다
fn classify(command: &Commands) -> Result<Option<Box<dyn ClientCommand + '_>>> {
    let driven: Box<dyn ClientCommand + '_> = match command {
        // ── 로컬 파일/프로세스 (통신 없음) ──────────────────────────────────
        Commands::Port => Box::new(crate::local::Port),
        Commands::Tool {
            command: ToolCommands::Ssh { profile, command },
        } => Box::new(crate::local::ToolSsh { profile, command }),
        Commands::Tool {
            command: ToolCommands::RemoteProfile { command },
        } => Box::new(crate::local::ToolRemoteProfile { command }),
        Commands::Tool {
            command: ToolCommands::Passkey { command },
        } => Box::new(crate::local::ToolPasskey { command }),
        Commands::Plugin {
            command: PluginCommands::Doctor { id },
        } => Box::new(crate::local::PluginDoctor { id }),

        // ── raw 스트림 / 터널 경유 조회 ─────────────────────────────────────
        Commands::Tool {
            command:
                ToolCommands::Attach {
                    name,
                    surface,
                    workspace,
                    send,
                    send_to,
                    dump_after,
                    raw,
                    no_reconnect,
                    list,
                },
        } => Box::new(crate::local::ToolAttach {
            name,
            surface: *surface,
            workspace: *workspace,
            send,
            send_to: *send_to,
            dump_after: *dump_after,
            raw: *raw,
            no_reconnect: *no_reconnect,
            list: *list,
        }),
        // `--ssh` + `--force-detach` 는 미지원 — 터널 너머 force-detach 가 아니라 로컬
        // surface 를 강제해제할 위험이 있어 명시적으로 거부한다. `--force-detach`(no ssh)
        // 는 로컬 JSON-RPC(attach.force_detach)라 아래 raw-stream arm(force_detach:false)
        // 에 안 걸리고 fall-through 한다.
        Commands::Remote {
            command:
                RemoteCommands::Attach {
                    ssh: Some(_),
                    force_detach: true,
                    ..
                },
        } => {
            anyhow::bail!(
                "{}",
                tasty_i18n::t("cli.dispatch.ssh_force_detach_exclusive")
            );
        }
        // `tasty remote attach` (non-force, non-into_gui) — SSH 터널(포트발견 + ssh -L) +
        // 단계 4 raw 스트림 attach, 백오프 재연결. `--into-gui` 는 JSON-RPC(attach.into_gui)
        // 로 fall-through(로컬 GUI 가 client 가 되어 원격 워크스페이스 mirror 재구성),
        // `--force-detach` 는 JSON-RPC(attach.force_detach)로 fall-through.
        // 로컬 loopback attach 는 release 표면에 없다 — `tasty debug attach`(debug 빌드).
        Commands::Remote {
            command:
                RemoteCommands::Attach {
                    surface,
                    workspace,
                    dump_after,
                    send,
                    send_to,
                    raw,
                    force_detach: false,
                    ssh,
                    profile,
                    remote_tasty,
                    remote_port_mode,
                    no_reconnect,
                    into_gui: false,
                    target_port: _,
                },
        } => Box::new(crate::local::RemoteAttach {
            surface: *surface,
            workspace: *workspace,
            dump_after: *dump_after,
            send,
            send_to: *send_to,
            raw: *raw,
            ssh,
            profile,
            remote_tasty,
            remote_port_mode,
            no_reconnect: *no_reconnect,
        }),
        Commands::Remote {
            command:
                RemoteCommands::Check {
                    ssh,
                    profile,
                    remote_tasty,
                    remote_port_mode,
                },
        } => Box::new(crate::local::RemoteCheck {
            ssh,
            profile,
            remote_tasty,
            remote_port_mode,
        }),
        Commands::Remote {
            command:
                RemoteCommands::Workspaces {
                    ssh,
                    profile,
                    remote_tasty,
                    remote_port_mode,
                    json,
                },
        } => Box::new(crate::local::RemoteWorkspaces {
            ssh,
            profile,
            remote_tasty,
            remote_port_mode,
            json: *json,
        }),
        Commands::Remote {
            command:
                RemoteCommands::NewWorkspace {
                    ssh,
                    profile,
                    remote_tasty,
                    remote_port_mode,
                    name,
                    cwd,
                    json,
                },
        } => Box::new(crate::local::RemoteNewWorkspace {
            ssh,
            profile,
            remote_tasty,
            remote_port_mode,
            name,
            cwd,
            json: *json,
        }),

        // ── 폴링/스트리밍 루프 (반복 IPC) ───────────────────────────────────
        Commands::Plugin {
            command: PluginCommands::Logs { id, follow },
        } => Box::new(crate::local::PluginLogs {
            id,
            follow: *follow,
        }),
        Commands::Plugin {
            command:
                PluginCommands::AuditFollow {
                    caller_kind,
                    caller_id,
                    method_prefix,
                    decision,
                    batch,
                    interval_ms,
                },
        } => Box::new(crate::local::PluginAuditFollow {
            caller_kind,
            caller_id,
            method_prefix,
            decision,
            batch: *batch,
            interval_ms: *interval_ms,
        }),

        // ── debug 빌드 전용 ────────────────────────────────────────────────
        #[cfg(debug_assertions)]
        Commands::Debug {
            command: crate::commands::DebugCommands::StreamEcho { payload, count },
        } => Box::new(crate::local::DebugStreamEcho {
            payload,
            count: *count,
        }),
        #[cfg(debug_assertions)]
        Commands::Debug {
            command: crate::commands::DebugCommands::Sim { cmd },
        } => Box::new(crate::local::DebugSim { cmd }),
        // `tasty debug attach <id>` (non-force, 로컬 loopback) — 단계 4 raw 스트림. 로컬
        // self-attach 는 사용자 입력 재현 성격이라 debug 빌드 전용으로 격리한다(원칙 1 ②).
        // `--force-detach` 는 일반 JSON-RPC(attach.force_detach)라 fall-through.
        #[cfg(debug_assertions)]
        Commands::Debug {
            command:
                crate::commands::DebugCommands::Attach {
                    surface,
                    workspace,
                    dump_after,
                    send,
                    send_to,
                    raw,
                    force_detach: false,
                },
        } => Box::new(crate::local::DebugAttach {
            surface: *surface,
            workspace: *workspace,
            dump_after: *dump_after,
            send,
            send_to: *send_to,
            raw: *raw,
        }),

        _ => return Ok(None),
    };
    Ok(Some(driven))
}
