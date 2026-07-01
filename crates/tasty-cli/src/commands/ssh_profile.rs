//! `tasty tool ssh ...` — ssh kind 원격 프로필 CRUD (attach/detach 단계 7).
//!
//! `~/.tasty/remote-profiles.toml` / `passkeys.toml` 는 client(이 머신) 로컬 파일이라
//! IPC 미경유로 직접 읽고 쓴다. 포커스 비의존(원칙 3): 모든 명령이 `--name` 으로 대상을
//! 지정한다. 비밀 값은 저장하지 않는다 — `--identity <path>` 는 path kind passkey
//! `<name>-key` 로 분리 저장되고, 프로필은 그 passkey 를 이름으로 참조한다.

use anyhow::Result;
use clap::Subcommand;

use tasty_remote_profiles::{
    Passkeys, RemoteProfile, RemoteProfiles, is_valid_shell, sanitize_passkey_name,
};

/// ssh 프로필의 legacy attach 필드(remote_tasty/port_mode) raw 접근. SshView 는 더
/// 이상 이 필드를 노출하지 않는다(01 데이터 모델 분리) — 이 CLI 표시부는 TODO 04 에서
/// tasty-attach 편집으로 이관될 전환기 shim 이다.
fn raw_field<'a>(p: &'a RemoteProfile, key: &str, default: &'static str) -> &'a str {
    p.fields
        .get(key)
        .and_then(|f| f.as_str())
        .unwrap_or(default)
}

#[derive(Subcommand)]
pub enum SshProfileCommands {
    /// 새 ssh 프로필 추가(같은 name 이 있으면 교체).
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
        /// identity 파일 경로(-i). path kind passkey `<name>-key` 로 분리 저장된다.
        #[arg(long)]
        identity: Option<String>,
        /// 추가 ssh -o 옵션(반복 가능). 예: --option ServerAliveInterval=30
        #[arg(long = "option")]
        options: Vec<String>,
        /// 원격 tasty 바이너리 경로(포트 발견용). 기본 "tasty".
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto | subcommand | file-unix | file-windows.
        #[arg(long, default_value = "auto")]
        port_mode: String,
        /// 원격 셸: powershell | cmd | bash | zsh | auto(기본).
        #[arg(long, default_value = "auto")]
        shell: String,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
    /// 저장된 프로필 목록 출력.
    List {
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
        #[arg(long = "option")]
        options: Vec<String>,
        #[arg(long)]
        remote_tasty: Option<String>,
        #[arg(long)]
        port_mode: Option<String>,
        /// 원격 셸: powershell | cmd | bash | zsh | auto.
        #[arg(long)]
        shell: Option<String>,
        #[arg(long)]
        label: Option<String>,
    },
    /// 프로필 제거(참조 passkey 는 공유 가능성 때문에 보존).
    Remove {
        #[arg(long)]
        name: String,
    },
    /// 저장된 프로필을 재감지한다(프로브 체인 1회 — SSH 접속 발생).
    Detect {
        #[arg(long)]
        name: String,
    },
}

/// `--identity <path>` 를 path kind passkey `<name>-key` 로 분리 저장하고 그 이름을
/// 반환한다(프로필 `passkey_ref` 로 연결). 같은 path 면 그대로 갱신(upsert).
fn link_identity_passkey(passkeys: &mut Passkeys, name: &str, identity: &str) -> Result<String> {
    let pk_name = format!("{}-key", sanitize_passkey_name(name));
    passkeys.upsert_path(&pk_name, identity.to_string())?;
    Ok(pk_name)
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
            options,
            remote_tasty,
            port_mode,
            shell,
            label,
        } => {
            if !is_valid_shell(shell) {
                anyhow::bail!("알 수 없는 --shell '{shell}' (powershell|cmd|bash|zsh|auto)");
            }
            let mut profiles = RemoteProfiles::load();
            let mut passkeys = Passkeys::load();
            let mut p = RemoteProfile::new(name.clone(), "ssh");
            p.set_field("host", host.clone());
            if let Some(u) = user {
                p.set_field("user", u.clone());
            }
            if let Some(pt) = port {
                p.set_field("port", pt.to_string());
            }
            if !options.is_empty() {
                p.set_field("extra_options", options.clone());
            }
            p.set_field("remote_tasty", remote_tasty.clone());
            p.set_field("port_mode", port_mode.clone());
            p.set_field("shell", shell.clone());
            p.label = label.clone();
            if let Some(idf) = identity {
                p.passkey_ref = Some(link_identity_passkey(&mut passkeys, name, idf)?);
            }
            let replaced = profiles.get(name).is_some();
            // shell 적용: 명시 셸 → 매핑(즉시), auto → SSH 프로브 1회(수 초 블록 가능).
            let detect = crate::ssh::apply_shell_to_profile(&mut p, &passkeys);
            profiles.upsert(p);
            passkeys.save()?;
            profiles.save()?;
            println!(
                "{} ssh 프로필 '{name}' ({host}).",
                if replaced { "갱신:" } else { "추가:" }
            );
            report_detect(name, &detect);
            Ok(())
        }
        SshProfileCommands::List { json } => {
            let profiles = RemoteProfiles::load();
            if *json {
                let arr: Vec<_> = profiles
                    .profiles
                    .iter()
                    .map(|p| {
                        let v = p.as_ssh();
                        serde_json::json!({
                            "name": p.name,
                            "kind": p.kind,
                            "host": v.as_ref().and_then(|v| v.host()),
                            "user": v.as_ref().and_then(|v| v.user()),
                            "port": v.as_ref().and_then(|v| v.port()),
                            "passkey_ref": p.passkey_ref,
                            "remote_tasty": v.as_ref().map(|_| raw_field(p, "remote_tasty", "tasty")),
                            "port_mode": v.as_ref().map(|_| raw_field(p, "port_mode", "auto")),
                            "shell": v.as_ref().map(|v| v.shell()),
                            "detect_failed": v.as_ref().map(|v| v.detect_failed()).unwrap_or(false),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else if profiles.profiles.is_empty() {
                println!("저장된 원격 프로필이 없습니다 (tasty tool ssh add ...).");
            } else {
                println!(
                    "{:<16} {:<8} {:<24} {:<11} {:<11} STATUS",
                    "NAME", "TYPE", "HOST", "SHELL", "PORT-MODE"
                );
                for p in &profiles.profiles {
                    let v = p.as_ssh();
                    let dest = v.as_ref().map(|v| v.ssh_destination()).unwrap_or_default();
                    let shell = v.as_ref().map(|v| v.shell()).unwrap_or("");
                    let pm = if v.is_some() {
                        raw_field(p, "port_mode", "auto")
                    } else {
                        ""
                    };
                    let status = if v.as_ref().map(|v| v.is_disabled()).unwrap_or(false) {
                        "감지 실패(비활성)"
                    } else if !p.is_builtin_kind() {
                        "미등록 타입"
                    } else {
                        ""
                    };
                    println!(
                        "{:<16} {:<8} {:<24} {:<11} {:<11} {}",
                        p.name, p.kind, dest, shell, pm, status
                    );
                }
            }
            Ok(())
        }
        SshProfileCommands::Show { name, json } => {
            let profiles = RemoteProfiles::load();
            let passkeys = Passkeys::load();
            let Some(p) = profiles.get(name) else {
                anyhow::bail!("원격 프로필 '{name}' 을 찾을 수 없습니다.");
            };
            if *json {
                println!("{}", serde_json::to_string_pretty(p)?);
            } else {
                println!("name          : {}", p.name);
                println!("type          : {}", p.kind);
                if let Some(l) = &p.label {
                    println!("label         : {l}");
                }
                if let Some(pk) = &p.passkey_ref {
                    let status = if passkeys.get(pk).is_some() {
                        ""
                    } else {
                        "  (passkey 없음)"
                    };
                    println!("passkey       : {pk}{status}");
                }
                if let Some(v) = p.as_ssh() {
                    println!("destination   : {}", v.ssh_destination());
                    if let Some(port) = v.port() {
                        println!("port          : {port}");
                    }
                    if !v.extra_options().is_empty() {
                        println!("extra_options : {}", v.extra_options().join(", "));
                    }
                    println!("remote_tasty  : {}", raw_field(p, "remote_tasty", "tasty"));
                    println!("shell         : {}", v.shell());
                    println!("port_mode     : {}", raw_field(p, "port_mode", "auto"));
                    if v.is_disabled() {
                        println!(
                            "status        : 감지 실패(비활성) — tasty tool ssh detect {name}"
                        );
                    }
                } else {
                    for (k, val) in &p.fields {
                        println!("{k:<14}: {val:?}");
                    }
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
            options,
            remote_tasty,
            port_mode,
            shell,
            label,
        } => {
            if let Some(s) = shell
                && !is_valid_shell(s)
            {
                anyhow::bail!("알 수 없는 --shell '{s}' (powershell|cmd|bash|zsh|auto)");
            }
            let mut profiles = RemoteProfiles::load();
            let mut passkeys = Passkeys::load();
            let Some(mut p) = profiles.get(name).cloned() else {
                anyhow::bail!("원격 프로필 '{name}' 을 찾을 수 없습니다.");
            };
            if let Some(h) = host {
                p.set_field("host", h.clone());
            }
            if let Some(u) = user {
                p.set_field("user", u.clone());
            }
            if let Some(pt) = port {
                p.set_field("port", pt.to_string());
            }
            if let Some(idf) = identity {
                p.passkey_ref = Some(link_identity_passkey(&mut passkeys, name, idf)?);
            }
            if !options.is_empty() {
                p.set_field("extra_options", options.clone());
            }
            if let Some(rt) = remote_tasty {
                p.set_field("remote_tasty", rt.clone());
            }
            if let Some(pm) = port_mode {
                p.set_field("port_mode", pm.clone());
            }
            if label.is_some() {
                p.label = label.clone();
            }
            // --shell 이 주어지면 셸 갱신 + 발견 모드 재도출(명시) / 재감지(auto).
            let detect = if let Some(s) = shell {
                p.set_field("shell", s.clone());
                crate::ssh::apply_shell_to_profile(&mut p, &passkeys)
            } else {
                None
            };
            profiles.upsert(p);
            passkeys.save()?;
            profiles.save()?;
            println!("갱신: 원격 프로필 '{name}'.");
            report_detect(name, &detect);
            Ok(())
        }
        SshProfileCommands::Detect { name } => {
            {
                let profiles = RemoteProfiles::load();
                if profiles.get(name).is_none() {
                    anyhow::bail!("원격 프로필 '{name}' 을 찾을 수 없습니다.");
                }
            }
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
            let mut profiles = RemoteProfiles::load();
            if profiles.remove(name) {
                profiles.save()?;
                println!("제거: 원격 프로필 '{name}'.");
            } else {
                anyhow::bail!("원격 프로필 '{name}' 을 찾을 수 없습니다.");
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
