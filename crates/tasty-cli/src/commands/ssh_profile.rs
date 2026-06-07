//! `tasty ssh-profile ...` — SSH 연결 프로필 CRUD (attach/detach 단계 7).
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
        #[arg(long, default_value = "auto")]
        port_mode: String,
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
        #[arg(long)]
        label: Option<String>,
    },
    /// 프로필 제거.
    Remove {
        #[arg(long)]
        name: String,
    },
}

/// `tasty ssh-profile ...` 로컬 분기 진입점(IPC 미경유).
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
            label,
        } => {
            let mut profiles = SshProfiles::load();
            let mut p = SshProfile::new(name.clone(), host.clone());
            p.user = user.clone();
            p.port = *port;
            p.identity_file = identity.clone();
            p.use_agent = !*no_agent;
            p.extra_options = options.clone();
            p.remote_tasty = remote_tasty.clone();
            p.port_mode = port_mode.clone();
            p.label = label.clone();
            let replaced = profiles.get(name).is_some();
            profiles.upsert(p);
            profiles.save()?;
            println!(
                "{} SSH 프로필 '{name}' ({host}).",
                if replaced { "갱신:" } else { "추가:" }
            );
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
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if profiles.profiles.is_empty() {
                println!("저장된 SSH 프로필이 없습니다 (tasty ssh-profile add ...).");
            } else {
                println!("{:<16} {:<28} {:<10} REMOTE-TASTY", "NAME", "HOST", "PORT-MODE");
                for p in &profiles.profiles {
                    let dest = p.ssh_destination();
                    println!(
                        "{:<16} {:<28} {:<10} {}",
                        p.name, dest, p.port_mode, p.remote_tasty
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
                println!("identity_file : {}", p.identity_file.as_deref().unwrap_or("(none)"));
                println!("use_agent     : {}", p.use_agent);
                if !p.extra_options.is_empty() {
                    println!("extra_options : {}", p.extra_options.join(", "));
                }
                println!("remote_tasty  : {}", p.remote_tasty);
                println!("port_mode     : {}", p.port_mode);
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
            label,
        } => {
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
            profiles.upsert(p);
            profiles.save()?;
            println!("갱신: SSH 프로필 '{name}'.");
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
