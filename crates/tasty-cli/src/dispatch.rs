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
        Commands::Port => Box::new(local::Port),
        Commands::Tool {
            command: ToolCommands::Ssh { profile, command },
        } => Box::new(local::ToolSsh { profile, command }),
        Commands::Tool {
            command: ToolCommands::RemoteProfile { command },
        } => Box::new(local::ToolRemoteProfile { command }),
        Commands::Tool {
            command: ToolCommands::Passkey { command },
        } => Box::new(local::ToolPasskey { command }),
        Commands::Plugin {
            command: PluginCommands::Doctor { id },
        } => Box::new(local::PluginDoctor { id }),

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
        } => Box::new(local::ToolAttach {
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
                "--ssh 와 --force-detach 는 함께 쓸 수 없습니다 (원격 force-detach 는 미지원)."
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
        } => Box::new(local::RemoteAttach {
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
        } => Box::new(local::RemoteCheck {
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
        } => Box::new(local::RemoteWorkspaces {
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
        } => Box::new(local::RemoteNewWorkspace {
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
        } => Box::new(local::PluginLogs {
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
        } => Box::new(local::PluginAuditFollow {
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
        } => Box::new(local::DebugStreamEcho {
            payload,
            count: *count,
        }),
        #[cfg(debug_assertions)]
        Commands::Debug {
            command: crate::commands::DebugCommands::Sim { cmd },
        } => Box::new(local::DebugSim { cmd }),
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
        } => Box::new(local::DebugAttach {
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

/// 클라이언트 주도 실행 단위들. 구현은 `local/` 로 위임한다.
mod local {
    use anyhow::Result;

    use super::{ClientCommand, ClientCtx};
    use crate::commands::passkey::PasskeyCommands;
    use crate::commands::remote_profile::RemoteProfileCommands;

    /// `tasty port` — 포트 파일을 읽어 출력한다. 로컬 전용(IPC 미경유):
    /// auto 원격 포트 발견 체인(`ssh host tasty port`)의 첫 단계 역할을 한다.
    /// 셸 독립성은 체인 전체(subcommand → file-unix → file-windows)가 주는 것이지
    /// 이 단계 하나가 주는 것이 아니다 — subcommand 단계는 Windows GUI release
    /// 셸에서 조용히 실패한다.
    pub struct Port;
    impl ClientCommand for Port {
        fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()> {
            crate::commands::port::run_port(ctx.port_file)
        }
    }

    /// `tasty tool ssh <profile>` — 저장된 ssh 프로필로 대화형 ssh 접속(로컬, IPC 미경유).
    /// remote-profiles.toml 은 client 로컬 파일이라 직접 resolve → 시스템 ssh spawn.
    pub struct ToolSsh<'a> {
        pub profile: &'a str,
        pub command: &'a [String],
    }
    impl ClientCommand for ToolSsh<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            let profiles = tasty_remote_profiles::RemoteProfiles::load();
            let passkeys = tasty_remote_profiles::Passkeys::load();
            let Some(p) = profiles.get(self.profile) else {
                anyhow::bail!(
                    "ssh 프로필 '{}' 을 찾을 수 없습니다 (tasty tool remote-profile list).",
                    self.profile
                );
            };
            if p.as_ssh().is_none() {
                anyhow::bail!(
                    "'{}' 은 ssh kind 가 아닙니다 (kind='{}'). tool ssh 는 ssh 프로필 전용.",
                    self.profile,
                    p.kind
                );
            }
            let target = crate::ssh::SshTarget::from_remote_profile(p, &passkeys)?;
            crate::ssh::run_ssh_connect(&target, self.command)
        }
    }

    /// `tasty tool remote-profile ...` — 프로필 CRUD (ssh + tasty-attach), 로컬 (IPC 미경유).
    pub struct ToolRemoteProfile<'a> {
        pub command: &'a RemoteProfileCommands,
    }
    impl ClientCommand for ToolRemoteProfile<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            crate::commands::remote_profile::run(self.command)
        }
    }

    /// `tasty tool passkey ...` — passkeys.toml 로컬 파일 (IPC 미경유).
    pub struct ToolPasskey<'a> {
        pub command: &'a PasskeyCommands,
    }
    impl ClientCommand for ToolPasskey<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            crate::commands::passkey::run(self.command)
        }
    }

    /// `tasty tool attach ...` — tasty-attach 프로필로 attach 실행 또는 `--list` 목록만.
    ///
    /// `--list` 면 tasty-attach kind 목록만 출력하고 종료. name 이 있으면 그 프로필을
    /// resolve(ADR-0032 ref/inline)해 기존 attach 엔진을 재사용한다.
    pub struct ToolAttach<'a> {
        pub name: &'a Option<String>,
        pub surface: Option<u32>,
        pub workspace: Option<u32>,
        pub send: &'a Option<String>,
        pub send_to: Option<u32>,
        pub dump_after: Option<u64>,
        pub raw: bool,
        pub no_reconnect: bool,
        pub list: bool,
    }
    impl ClientCommand for ToolAttach<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            if self.list {
                return crate::commands::remote_profile::run(&RemoteProfileCommands::List {
                    json: false,
                    kind: Some("tasty-attach".to_string()),
                });
            }
            let Some(name) = self.name.as_deref() else {
                anyhow::bail!(
                    "attach 대상이 필요합니다: `tasty tool attach <profile> <surface|--workspace id>` \
                     또는 `tasty tool attach --list` (tasty-attach 목록)."
                );
            };
            if self.surface.is_some() && self.workspace.is_some() {
                anyhow::bail!("surface 와 --workspace 는 함께 쓸 수 없습니다.");
            }
            let profiles = tasty_remote_profiles::RemoteProfiles::load();
            let passkeys = tasty_remote_profiles::Passkeys::load();
            let Some(p) = profiles.get(name) else {
                anyhow::bail!(
                    "tasty-attach 프로필 '{name}' 을 찾을 수 없습니다 (tasty tool attach --list)."
                );
            };
            let (target, rt, pm, pf) = crate::ssh::resolve_attach_target(p, &profiles, &passkeys)?;
            if let Some(ws) = self.workspace {
                if self.raw {
                    anyhow::bail!(
                        "--raw 는 workspace attach 와 함께 쓸 수 없습니다 (다중화 스트림)."
                    );
                }
                return crate::commands::attach::run_attach_workspace_ssh(
                    target,
                    &rt,
                    &pm,
                    pf.as_deref(),
                    ws,
                    self.dump_after,
                    self.send.as_deref(),
                    self.send_to,
                    !self.no_reconnect,
                );
            }
            let Some(surface) = self.surface else {
                anyhow::bail!("attach 대상이 필요합니다: <surface_id> 또는 --workspace <id>.");
            };
            crate::commands::attach::run_attach_ssh(
                target,
                &rt,
                &pm,
                pf.as_deref(),
                surface,
                self.dump_after,
                self.send.as_deref(),
                self.raw,
                !self.no_reconnect,
            )
        }
    }

    /// `tasty remote attach` — SSH 터널 + raw 스트림 attach, 백오프 재연결.
    pub struct RemoteAttach<'a> {
        pub surface: Option<u32>,
        pub workspace: Option<u32>,
        pub dump_after: Option<u64>,
        pub send: &'a Option<String>,
        pub send_to: Option<u32>,
        pub raw: bool,
        pub ssh: &'a Option<String>,
        pub profile: &'a Option<String>,
        pub remote_tasty: &'a str,
        pub remote_port_mode: &'a str,
        pub no_reconnect: bool,
    }
    impl ClientCommand for RemoteAttach<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            if self.surface.is_some() && self.workspace.is_some() {
                anyhow::bail!("surface 와 --workspace 는 함께 쓸 수 없습니다.");
            }
            if self.ssh.is_some() && self.profile.is_some() {
                anyhow::bail!("--ssh 와 --profile 는 함께 쓸 수 없습니다.");
            }
            let (target, rt, pm, pf) = resolve_attach_spec(
                self.profile.as_deref(),
                self.ssh.as_deref(),
                self.remote_tasty,
                self.remote_port_mode,
                "원격 attach 대상이 필요합니다 (--ssh 또는 --profile). \
                     로컬 attach 는 `tasty debug attach` (debug 빌드).",
            )?;
            // workspace 단위 attach (단계 6): 트리 N-터미널 다중화 mirror.
            if let Some(ws) = self.workspace {
                if self.raw {
                    anyhow::bail!(
                        "--raw 는 workspace attach 와 함께 쓸 수 없습니다 (다중화 스트림)."
                    );
                }
                return crate::commands::attach::run_attach_workspace_ssh(
                    target,
                    &rt,
                    &pm,
                    pf.as_deref(),
                    ws,
                    self.dump_after,
                    self.send.as_deref(),
                    self.send_to,
                    !self.no_reconnect,
                );
            }
            // surface 단위 attach (단계 4/5).
            let Some(surface) = self.surface else {
                anyhow::bail!("attach 대상이 필요합니다: <surface_id> 또는 --workspace <id>.");
            };
            crate::commands::attach::run_attach_ssh(
                target,
                &rt,
                &pm,
                pf.as_deref(),
                surface,
                self.dump_after,
                self.send.as_deref(),
                self.raw,
                !self.no_reconnect,
            )
        }
    }

    /// `tasty remote check` — 원격 tasty 생존 확인. 포트 발견 + ssh -L 터널 + 터널 포트로
    /// 가벼운 IPC(system.info) 1 회. 포트 발견만으로 alive 단정하지 않는다(stale 포트
    /// 거짓 alive 방지).
    pub struct RemoteCheck<'a> {
        pub ssh: &'a Option<String>,
        pub profile: &'a Option<String>,
        pub remote_tasty: &'a str,
        pub remote_port_mode: &'a str,
    }
    impl ClientCommand for RemoteCheck<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            if self.ssh.is_some() && self.profile.is_some() {
                anyhow::bail!("--ssh 와 --profile 는 함께 쓸 수 없습니다.");
            }
            let (target, rt, pm, pf) = resolve_attach_spec(
                self.profile.as_deref(),
                self.ssh.as_deref(),
                self.remote_tasty,
                self.remote_port_mode,
                "원격 check 대상이 필요합니다 (--ssh 또는 --profile).",
            )?;
            crate::commands::remote_check::run_remote_check(target, &rt, &pm, pf.as_deref())
        }
    }

    /// `tasty remote workspaces` — 원격 tasty 의 워크스페이스 목록 조회(browse).
    /// 순수 조회라 로컬 사용자 상태(focus)에 닿지 않는다(원칙 1).
    pub struct RemoteWorkspaces<'a> {
        pub ssh: &'a Option<String>,
        pub profile: &'a Option<String>,
        pub remote_tasty: &'a str,
        pub remote_port_mode: &'a str,
        pub json: bool,
    }
    impl ClientCommand for RemoteWorkspaces<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            if self.ssh.is_some() && self.profile.is_some() {
                anyhow::bail!("--ssh 와 --profile 는 함께 쓸 수 없습니다.");
            }
            // 접속 스펙 resolve(profile/ssh) — CLI 와 호스트 IPC 워커가 공유하는 helper.
            let (target, rt, pm, pf) = crate::remote_browse::resolve_connection_spec(
                self.profile.as_deref(),
                self.ssh.as_deref(),
                self.remote_tasty,
                self.remote_port_mode,
            )?;
            crate::commands::remote_workspaces::run_remote_workspaces(
                target,
                &rt,
                &pm,
                pf.as_deref(),
                self.json,
            )
        }
    }

    /// `tasty remote new-workspace` — 원격 tasty 에 워크스페이스 생성(원격 mutate).
    /// 로컬 사용자 상태에 닿지 않는다(원칙 1).
    pub struct RemoteNewWorkspace<'a> {
        pub ssh: &'a Option<String>,
        pub profile: &'a Option<String>,
        pub remote_tasty: &'a str,
        pub remote_port_mode: &'a str,
        pub name: &'a Option<String>,
        pub cwd: &'a Option<String>,
        pub json: bool,
    }
    impl ClientCommand for RemoteNewWorkspace<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            if self.ssh.is_some() && self.profile.is_some() {
                anyhow::bail!("--ssh 와 --profile 는 함께 쓸 수 없습니다.");
            }
            let (target, rt, pm, pf) = crate::remote_browse::resolve_connection_spec(
                self.profile.as_deref(),
                self.ssh.as_deref(),
                self.remote_tasty,
                self.remote_port_mode,
            )?;
            crate::commands::remote_new_workspace::run_remote_new_workspace(
                target,
                &rt,
                &pm,
                pf.as_deref(),
                self.name.as_deref(),
                self.cwd.as_deref(),
                self.json,
            )
        }
    }

    /// plugin logs is local-only — read the log file directly.
    pub struct PluginLogs<'a> {
        pub id: &'a str,
        pub follow: bool,
    }
    impl ClientCommand for PluginLogs<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            crate::plugin::run_plugin_logs(self.id, self.follow)
        }
    }

    /// plugin doctor is local-only — read manifest from disk, no IPC needed.
    pub struct PluginDoctor<'a> {
        pub id: &'a str,
    }
    impl ClientCommand for PluginDoctor<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            crate::plugin::run_plugin_doctor(self.id)
        }
    }

    /// plugin audit-follow is a polling loop over plugin.audit_follow IPC.
    pub struct PluginAuditFollow<'a> {
        pub caller_kind: &'a Option<String>,
        pub caller_id: &'a Option<String>,
        pub method_prefix: &'a Option<String>,
        pub decision: &'a Option<String>,
        pub batch: u64,
        pub interval_ms: u64,
    }
    impl ClientCommand for PluginAuditFollow<'_> {
        fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()> {
            crate::plugin::run_audit_follow(
                self.caller_kind.as_deref(),
                self.caller_id.as_deref(),
                self.method_prefix.as_deref(),
                self.decision.as_deref(),
                self.batch,
                self.interval_ms,
                ctx.port_file,
            )
        }
    }

    /// debug stream-echo uses a raw framed streaming channel, not the JSON-RPC
    /// request-response path (debug builds only).
    #[cfg(debug_assertions)]
    pub struct DebugStreamEcho<'a> {
        pub payload: &'a str,
        pub count: u32,
    }
    #[cfg(debug_assertions)]
    impl ClientCommand for DebugStreamEcho<'_> {
        fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()> {
            crate::commands::debug::run_stream_echo(self.payload, self.count, ctx.port_file)
        }
    }

    /// debug sim emits raw VTE to its own stdout from inside the current surface —
    /// no host instance / IPC involved (debug builds only).
    #[cfg(debug_assertions)]
    pub struct DebugSim<'a> {
        pub cmd: &'a Option<tasty_tui_simulator::Commands>,
    }
    #[cfg(debug_assertions)]
    impl ClientCommand for DebugSim<'_> {
        fn run(self: Box<Self>, _ctx: &ClientCtx<'_>) -> Result<()> {
            tasty_tui_simulator::run(self.cmd);
            Ok(())
        }
    }

    /// `tasty debug attach <id>` — 로컬 loopback raw 스트림 (debug 빌드 전용).
    #[cfg(debug_assertions)]
    pub struct DebugAttach<'a> {
        pub surface: Option<u32>,
        pub workspace: Option<u32>,
        pub dump_after: Option<u64>,
        pub send: &'a Option<String>,
        pub send_to: Option<u32>,
        pub raw: bool,
    }
    #[cfg(debug_assertions)]
    impl ClientCommand for DebugAttach<'_> {
        fn run(self: Box<Self>, ctx: &ClientCtx<'_>) -> Result<()> {
            if self.surface.is_some() && self.workspace.is_some() {
                anyhow::bail!("surface 와 --workspace 는 함께 쓸 수 없습니다.");
            }
            if let Some(ws) = self.workspace {
                if self.raw {
                    anyhow::bail!(
                        "--raw 는 workspace attach 와 함께 쓸 수 없습니다 (다중화 스트림)."
                    );
                }
                return crate::commands::debug::attach::run_attach_workspace(
                    ws,
                    self.dump_after,
                    self.send.as_deref(),
                    self.send_to,
                    ctx.port_file,
                );
            }
            let Some(surface) = self.surface else {
                anyhow::bail!("attach 대상이 필요합니다: <surface_id> 또는 --workspace <id>.");
            };
            crate::commands::debug::attach::run_attach(
                surface,
                self.dump_after,
                self.send.as_deref(),
                self.raw,
                ctx.port_file,
            )
        }
    }

    /// 원격 접속 스펙 결정: 저장 프로필(비활성 거부) → `SshTarget`(+remote_tasty/
    /// port_mode 대체), 없으면 1회성 `--ssh`. 둘 다 없으면 `missing` 문구로 거부한다.
    ///
    /// `attach` 와 `check` 가 같은 규칙을 쓰되 대상 미지정 문구만 다르다.
    fn resolve_attach_spec(
        profile: Option<&str>,
        ssh: Option<&str>,
        remote_tasty: &str,
        remote_port_mode: &str,
        missing: &str,
    ) -> Result<(crate::ssh::SshTarget, String, String, Option<String>)> {
        match profile {
            Some(name) => {
                let profiles = tasty_remote_profiles::RemoteProfiles::load();
                let passkeys = tasty_remote_profiles::Passkeys::load();
                let Some(p) = profiles.get(name) else {
                    anyhow::bail!(
                        "{}",
                        tasty_i18n::t_fmt("cli.remote_profile.not_found_list_hint", name)
                    );
                };
                crate::ssh::resolve_attach_target(p, &profiles, &passkeys)
            }
            None => match ssh {
                Some(dest) => Ok((
                    crate::ssh::SshTarget::parse(dest),
                    remote_tasty.to_string(),
                    remote_port_mode.to_string(),
                    None,
                )),
                None => anyhow::bail!("{missing}"),
            },
        }
    }
}
