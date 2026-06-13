//! `tasty tool ssh ...` — SSH 연결 프로필 CRUD (attach/detach 단계 7).
//!
//! `~/.tasty/ssh-profiles.toml` 는 client(이 머신)의 로컬 파일이라 IPC 미경유로 직접
//! 읽고 쓴다(`tasty port` / `tasty file-handler` 와 같은 로컬 분기). 포커스 비의존
//! (원칙 3): 모든 명령이 `--name` 으로 대상을 지정한다. 비밀번호는 저장하지 않는다.

use anyhow::Result;
use clap::Subcommand;

use tasty_ssh_profiles::{SshProfile, SshProfiles};

#[derive(Subcommand)]
pub enum SshProfileCommands {
    /// 새 SSH 프로필 추가(같은 name 이 있으면 교체).
    Add {
        /// 프로필 고유 식별자(워크스페이스 매핑이 참조).
        #[arg(long)]
        name: String,
        /// ssh destination: host | user@host | ssh config alias.
        #[arg(long)]
        host: String,
        /// ssh 유저(host 에 user@ 가 없을 때).
        #[arg(long)]
        user: Option<String>,
        /// ssh 포트(기본: ssh config / 22).
        #[arg(long)]
        port: Option<u16>,
        /// identity 파일 경로(-i). 없고 agent 사용 시 agent 위임.
        #[arg(long)]
        identity: Option<String>,
        /// ssh-agent 위임 비활성(기본: agent 사용).
        #[arg(long)]
        no_agent: bool,
        /// 추가 ssh -o 옵션(반복 가능). 예: --option ServerAliveInterval=30
        #[arg(long = "option")]
        options: Vec<String>,
        /// 원격 tasty 바이너리 경로(포트 발견용). 기본 "tasty".
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto | subcommand | file-unix | file-windows.
        /// (`--shell` 이 명시 셸이거나 auto 감지 성공 시 덮어쓰여진다.)
        #[arg(long, default_value = "auto")]
        port_mode: String,
        /// 원격 셸: powershell | cmd | bash | zsh | auto(기본). 명시 셸이면 발견
        /// 모드를 즉시 도출하고, auto 면 등록 시점에 1회 자동감지(SSH 프로브)한다.
        #[arg(long, default_value = "auto")]
        shell: String,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
    /// 저장된 프로필 목록 출력.
    List {
        /// JSON 출력.
        #[arg(long)]
        json: bool,
    },
    /// 한 프로필 상세 출력.
    Show {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// 기존 프로필의 일부 필드 갱신(지정한 필드만 덮어쓴다).
    Edit {
        #[arg(long)]
        name: String,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        identity: Option<String>,
        /// agent 사용 강제(true) / 비활성(false).
        #[arg(long)]
        use_agent: Option<bool>,
        #[arg(long = "option")]
        options: Vec<String>,
        #[arg(long)]
        remote_tasty: Option<String>,
        #[arg(long)]
        port_mode: Option<String>,
        /// 원격 셸: powershell | cmd | bash | zsh | auto. 지정 시 발견 모드를 재도출
        /// (명시 셸) 하거나 자동감지(auto)를 다시 실행한다.
        #[arg(long)]
        shell: Option<String>,
        #[arg(long)]
        label: Option<String>,
    },
    /// 프로필 제거.
    Remove {
        #[arg(long)]
        name: String,
    },
    /// 저장된 프로필을 재감지한다(프로브 체인 1회 — SSH 접속 발생).
    /// 성공 시 발견 모드를 갱신·활성화, 전 프로브 실패 시 "감지 실패"(비활성)로 기록.
    Detect {
        #[arg(long)]
        name: String,
    },
}

/// `tasty tool ssh ...` 로컬 분기 진입점(IPC 미경유).
pub fn run(command: &SshProfileCommands) -> Result<()> {
    match command {
        SshProfileCommands::Add {
            name,
            host,
            user,
            port,
            identity,
            no_agent,
            options,
            remote_tasty,
            port_mode,
            shell,
            label,
        } => {
            if !tasty_ssh_profiles::is_valid_shell(shell) {
                anyhow::bail!("알 수 없는 --shell '{shell}' (powershell|cmd|bash|zsh|auto)");
            }
            let mut profiles = SshProfiles::load();
            let mut p = SshProfile::new(name.clone(), host.clone());
            p.user = user.clone();
            p.port = *port;
            p.identity_file = identity.clone();
            p.use_agent = !*no_agent;
            p.extra_options = options.clone();
            p.remote_tasty = remote_tasty.clone();
            p.port_mode = port_mode.clone();
            p.shell = shell.clone();
            p.label = label.clone();
            let replaced = profiles.get(name).is_some();
            // shell 적용: 명시 셸 → 매핑(즉시), auto → SSH 프로브 1회(수 초 블록 가능).
            let detect = crate::ssh::apply_shell_to_profile(&mut p);
            profiles.upsert(p);
            profiles.save()?;
            println!(
                "{} SSH 프로필 '{name}' ({host}).",
                if replaced { "갱신:" } else { "추가:" }
            );
            report_detect(name, &detect);
            Ok(())
        }
        SshProfileCommands::List { json } => {
            let profiles = SshProfiles::load();
            if *json {
                let arr: Vec<_> = profiles
                    .profiles
                    .iter()
                    .map(|p| {
                        serde_json::json!({
                            "name": p.name,
                            "host": p.host,
                            "user": p.user,
                            "port": p.port,
                            "remote_tasty": p.remote_tasty,
                            "port_mode": p.port_mode,
                            "shell": p.shell,
                            "detect_failed": p.detect_failed,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if profiles.profiles.is_empty() {
                println!("저장된 SSH 프로필이 없습니다 (tasty tool ssh add ...).");
            } else {
                println!(
                    "{:<16} {:<24} {:<11} {:<11} STATUS",
                    "NAME", "HOST", "SHELL", "PORT-MODE"
                );
                for p in &profiles.profiles {
                    let dest = p.ssh_destination();
                    let status = if p.is_disabled() {
                        "감지 실패(비활성)"
                    } else {
                        ""
                    };
                    println!(
                        "{:<16} {:<24} {:<11} {:<11} {}",
                        p.name, dest, p.shell, p.port_mode, status
                    );
                }
            }
            Ok(())
        }
        SshProfileCommands::Show { name, json } => {
            let profiles = SshProfiles::load();
            let Some(p) = profiles.get(name) else {
                anyhow::bail!("SSH 프로필 '{name}' 을 찾을 수 없습니다.");
            };
            if *json {
                println!("{}", serde_json::to_string_pretty(p)?);
            } else {
                println!("name          : {}", p.name);
                if let Some(l) = &p.label {
                    println!("label         : {l}");
                }
                println!("host          : {}", p.host);
                println!("destination   : {}", p.ssh_destination());
                if let Some(port) = p.port {
                    println!("port          : {port}");
                }
                println!(
                    "identity_file : {}",
                    p.identity_file.as_deref().unwrap_or("(none)")
                );
                println!("use_agent     : {}", p.use_agent);
                if !p.extra_options.is_empty() {
                    println!("extra_options : {}", p.extra_options.join(", "));
                }
                println!("remote_tasty  : {}", p.remote_tasty);
                println!("shell         : {}", p.shell);
                println!("port_mode     : {}", p.port_mode);
                if p.is_disabled() {
                    println!("status        : 감지 실패(비활성) — tasty tool ssh detect {name}");
                }
            }
            Ok(())
        }
        SshProfileCommands::Edit {
            name,
            host,
            user,
            port,
            identity,
            use_agent,
            options,
            remote_tasty,
            port_mode,
            shell,
            label,
        } => {
            if let Some(s) = shell
                && !tasty_ssh_profiles::is_valid_shell(s)
            {
                anyhow::bail!("알 수 없는 --shell '{s}' (powershell|cmd|bash|zsh|auto)");
            }
            let mut profiles = SshProfiles::load();
            let Some(existing) = profiles.get(name).cloned() else {
                anyhow::bail!("SSH 프로필 '{name}' 을 찾을 수 없습니다.");
            };
            let mut p = existing;
            if let Some(h) = host {
                p.host = h.clone();
            }
            if user.is_some() {
                p.user = user.clone();
            }
            if port.is_some() {
                p.port = *port;
            }
            if identity.is_some() {
                p.identity_file = identity.clone();
            }
            if let Some(a) = use_agent {
                p.use_agent = *a;
            }
            if !options.is_empty() {
                p.extra_options = options.clone();
            }
            if let Some(rt) = remote_tasty {
                p.remote_tasty = rt.clone();
            }
            if let Some(pm) = port_mode {
                p.port_mode = pm.clone();
            }
            if label.is_some() {
                p.label = label.clone();
            }
            // --shell 이 주어지면 셸을 갱신하고 발견 모드를 재도출(명시) / 재감지(auto).
            // 주어지지 않으면 기존 shell/port_mode/detect_failed 를 보존한다.
            let detect = if let Some(s) = shell {
                p.shell = s.clone();
                crate::ssh::apply_shell_to_profile(&mut p)
            } else {
                None
            };
            profiles.upsert(p);
            profiles.save()?;
            println!("갱신: SSH 프로필 '{name}'.");
            report_detect(name, &detect);
            Ok(())
        }
        SshProfileCommands::Detect { name } => {
            {
                let profiles = SshProfiles::load();
                if profiles.get(name).is_none() {
                    anyhow::bail!("SSH 프로필 '{name}' 을 찾을 수 없습니다.");
                }
            }
            // 셸 무관하게 프로브 체인을 다시 돌려 발견 모드를 갱신·저장(비활성 복귀 포함).
            match crate::ssh::detect_and_persist(name) {
                Ok(mode) => println!(
                    "재감지 성공: '{name}' → port_mode={} (활성).",
                    mode.as_str()
                ),
                Err(e) => println!(
                    "재감지 실패: {e}\n  '{name}' 은 비활성 상태입니다 — 원격 환경 확인 후 \
                     다시 'tasty tool ssh detect {name}'."
                ),
            }
            Ok(())
        }
        SshProfileCommands::Remove { name } => {
            let mut profiles = SshProfiles::load();
            if profiles.remove(name) {
                profiles.save()?;
                println!("제거: SSH 프로필 '{name}'.");
            } else {
                anyhow::bail!("SSH 프로필 '{name}' 을 찾을 수 없습니다.");
            }
            Ok(())
        }
    }
}

/// `apply_shell_to_profile` 결과를 사용자에게 출력한다. 명시 셸(None)은 조용히 넘어간다.
fn report_detect(name: &str, detect: &Option<Result<crate::ssh::PortMode>>) {
    match detect {
        Some(Ok(mode)) => {
            println!(
                "자동감지 성공: 원격 환경 → port_mode={} (활성).",
                mode.as_str()
            )
        }
        Some(Err(e)) => println!(
            "자동감지 실패: {e}\n  '{name}' 은 비활성 상태로 저장되었습니다 — \
             'tasty tool ssh detect {name}' 로 재시도하세요."
        ),
        None => {}
    }
}
