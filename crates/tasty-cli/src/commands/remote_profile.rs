//! `tasty tool remote-profile ...` — 원격 접속 프로필 통합 CRUD (ssh + tasty-attach).
//!
//! `~/.tasty/remote-profiles.toml` / `passkeys.toml` 는 client(이 머신) 로컬 파일이라
//! IPC 미경유로 직접 읽고 쓴다. 포커스 비의존(원칙 3): 모든 명령이 `--name` 으로 대상을
//! 지정한다. 비밀 값은 저장하지 않는다 — `--identity <path>` 는 path kind passkey
//! `<name>-key` 로 분리 저장되고, 프로필은 그 passkey 를 이름으로 참조한다.
//!
//! 2-레이어 모델(ADR): **ssh** = 순수 연결 정보(host/user/port/identity/options/shell),
//! **tasty-attach** = attach 스펙(ssh_ref 참조 또는 인라인 연결 + remote_tasty/port_mode/
//! port_file). attach 동작 자체는 `tasty tool attach` 에서 tasty-attach 프로필을 소비한다.

use anyhow::Result;
use clap::Subcommand;
use tasty_i18n::{t, t_args, t_fmt, t_fmt2};

use tasty_remote_profiles::{
    ImportError, Passkeys, RemoteProfile, RemoteProfiles, SshConfigHost, config_availability,
    enumerate_hosts, imported_as, is_valid_shell, prepare_import, sanitize_passkey_name,
    user_config_path,
};

#[derive(Subcommand)]
pub enum RemoteProfileCommands {
    /// 저장된 프로필 목록 출력(ssh + tasty-attach).
    List {
        #[arg(long)]
        json: bool,
        /// kind 필터: ssh | tasty-attach.
        #[arg(long)]
        kind: Option<String>,
    },
    /// 한 프로필 상세 출력.
    Show {
        #[arg(long)]
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// ssh 연결 프로필 추가(순수 연결 정보 — attach 스펙 없음).
    AddSsh {
        /// 프로필 고유 식별자.
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
        /// 원격 셸: powershell | cmd | bash | zsh | auto(기본).
        #[arg(long, default_value = "auto")]
        shell: String,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
    /// tasty-attach 프로필 추가. 연결은 `--ssh-ref <name>` 참조 또는 인라인 필드(host/…).
    AddAttach {
        /// 프로필 고유 식별자.
        #[arg(long)]
        name: String,
        /// 참조할 ssh 프로필 name(라이브 팔로우). 지정 시 인라인 연결 필드는 무시된다.
        #[arg(long = "ssh-ref")]
        ssh_ref: Option<String>,
        /// 인라인 연결: ssh destination(host | user@host | alias). `--ssh-ref` 없을 때.
        #[arg(long)]
        host: Option<String>,
        /// 인라인 연결: ssh 유저.
        #[arg(long)]
        user: Option<String>,
        /// 인라인 연결: ssh 포트.
        #[arg(long)]
        port: Option<u16>,
        /// 인라인 연결: identity 파일 경로(-i). path kind passkey 로 분리 저장.
        #[arg(long)]
        identity: Option<String>,
        /// 인라인 연결: 추가 ssh -o 옵션(반복 가능).
        #[arg(long = "option")]
        options: Vec<String>,
        /// 원격 tasty 바이너리 경로(포트 발견용). 기본 "tasty".
        #[arg(long, default_value = "tasty")]
        remote_tasty: String,
        /// 원격 포트 발견 모드: auto(기본) | subcommand | file-unix | file-windows.
        #[arg(long, default_value = "auto")]
        port_mode: String,
        /// 원격 port 파일의 명시 경로(비표준 위치). 지정 시 관례 경로보다 최우선.
        #[arg(long)]
        port_file: Option<String>,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
    /// 기존 프로필의 일부 필드 갱신(지정한 필드만 덮어쓴다). kind 는 유지된다.
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
        /// tasty-attach: 참조 ssh 프로필 name 갱신.
        #[arg(long = "ssh-ref")]
        ssh_ref: Option<String>,
        /// tasty-attach: 원격 tasty 바이너리 경로.
        #[arg(long)]
        remote_tasty: Option<String>,
        /// tasty-attach: 원격 포트 발견 모드.
        #[arg(long)]
        port_mode: Option<String>,
        /// tasty-attach: 원격 port 파일 경로.
        #[arg(long)]
        port_file: Option<String>,
        /// ssh: 원격 셸(powershell | cmd | bash | zsh | auto).
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
    /// 프로필을 재감지한다(ssh: 셸 감지 프로브 / tasty-attach: 원격 포트 검증). SSH 접속 발생.
    Detect {
        #[arg(long)]
        name: String,
    },
    /// 로컬 ssh config(`~/.ssh/config` + Include)의 Host alias 목록. 접속하지 않는다.
    ListLocal {
        #[arg(long)]
        json: bool,
    },
    /// 로컬 ssh config alias 를 ssh 프로필로 가져온다(alias 만 저장 — 값 해석은 ssh 몫).
    Import {
        /// 가져올 ssh config alias (`list-local` 의 ALIAS 열).
        #[arg(long)]
        from: String,
        /// 새로 만들 프로필 고유 식별자.
        #[arg(long)]
        name: String,
        /// UI 표시용 라벨(옵션).
        #[arg(long)]
        label: Option<String>,
    },
}

/// `--identity <path>` 를 path kind passkey `<name>-key` 로 분리 저장하고 그 이름을
/// 반환한다(프로필 `passkey_ref` 로 연결). 같은 path 면 그대로 갱신(upsert).
fn link_identity_passkey(passkeys: &mut Passkeys, name: &str, identity: &str) -> Result<String> {
    let pk_name = format!("{}-key", sanitize_passkey_name(name));
    passkeys.upsert_path(&pk_name, identity.to_string())?;
    Ok(pk_name)
}

/// `tasty tool remote-profile ...` 로컬 분기 진입점(IPC 미경유).
#[allow(clippy::cognitive_complexity)] // complexity-exempt: CLI 서브커맨드 평면 match 디스패치 — arm 수 많으나 중첩 얕음, 분해 시 라우팅 추적만 어려워짐
pub fn run(command: &RemoteProfileCommands) -> Result<()> {
    match command {
        RemoteProfileCommands::AddSsh {
            name,
            host,
            user,
            port,
            identity,
            options,
            shell,
            label,
        } => {
            if !is_valid_shell(shell) {
                anyhow::bail!("{}", t_fmt("cli.remote_profile.unknown_shell", shell));
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
            let key = if replaced {
                "cli.remote_profile.ssh_updated"
            } else {
                "cli.remote_profile.ssh_added"
            };
            println!("{}", t_fmt2(key, name, host));
            report_detect(name, &detect);
            Ok(())
        }
        RemoteProfileCommands::AddAttach {
            name,
            ssh_ref,
            host,
            user,
            port,
            identity,
            options,
            remote_tasty,
            port_mode,
            port_file,
            label,
        } => {
            if ssh_ref.is_some() && host.is_some() {
                anyhow::bail!("{}", t("cli.remote_profile.attach_ref_xor_host"));
            }
            if ssh_ref.is_none() && host.is_none() {
                anyhow::bail!("{}", t("cli.remote_profile.attach_needs_ref_or_host"));
            }
            let mut profiles = RemoteProfiles::load();
            let mut passkeys = Passkeys::load();
            let mut p = RemoteProfile::new(name.clone(), "tasty-attach");
            if let Some(r) = ssh_ref {
                if profiles.get(r).is_none() {
                    eprintln!("{}", t_fmt("cli.remote_profile.attach_ref_missing", r));
                }
                p.set_field("ssh_ref", r.clone());
            } else {
                // 인라인 연결 필드.
                if let Some(h) = host {
                    p.set_field("host", h.clone());
                }
                if let Some(u) = user {
                    p.set_field("user", u.clone());
                }
                if let Some(pt) = port {
                    p.set_field("port", pt.to_string());
                }
                if !options.is_empty() {
                    p.set_field("extra_options", options.clone());
                }
                if let Some(idf) = identity {
                    p.passkey_ref = Some(link_identity_passkey(&mut passkeys, name, idf)?);
                }
            }
            p.set_field("remote_tasty", remote_tasty.clone());
            p.set_field("port_mode", port_mode.clone());
            if let Some(pf) = port_file {
                p.set_field("port_file", pf.clone());
            }
            p.label = label.clone();
            let replaced = profiles.get(name).is_some();
            profiles.upsert(p);
            passkeys.save()?;
            profiles.save()?;
            let key = if replaced {
                "cli.remote_profile.attach_updated"
            } else {
                "cli.remote_profile.attach_added"
            };
            println!("{}", t_fmt(key, name));
            Ok(())
        }
        RemoteProfileCommands::List { json, kind } => {
            let profiles = RemoteProfiles::load();
            let matches_kind = |p: &RemoteProfile| kind.as_deref().is_none_or(|k| p.kind == k);
            if *json {
                let arr: Vec<_> = profiles
                    .profiles
                    .iter()
                    .filter(|p| matches_kind(p))
                    .map(profile_json)
                    .collect();
                println!("{}", serde_json::to_string_pretty(&arr)?);
            } else {
                let rows: Vec<&RemoteProfile> = profiles
                    .profiles
                    .iter()
                    .filter(|p| matches_kind(p))
                    .collect();
                if rows.is_empty() {
                    println!("{}", t("cli.remote_profile.list_empty"));
                } else {
                    // 헤더는 컬럼 폭을 언어별로 맞춰야 해서(CJK 는 표시 폭이 2배)
                    // 패딩까지 포함한 한 줄 전체를 번역 값으로 둔다.
                    println!("{}", t("cli.remote_profile.list_header"));
                    for p in rows {
                        let (dest, detail) = summarize(p);
                        println!(
                            "{:<16} {:<13} {:<24} {:<16} {}",
                            p.name,
                            p.kind,
                            dest,
                            detail,
                            status_of(p),
                        );
                    }
                }
            }
            Ok(())
        }
        RemoteProfileCommands::Show { name, json } => {
            let profiles = RemoteProfiles::load();
            let passkeys = Passkeys::load();
            let Some(p) = profiles.get(name) else {
                anyhow::bail!("{}", t_fmt("cli.remote_profile.not_found", name));
            };
            if *json {
                println!("{}", serde_json::to_string_pretty(p)?);
                return Ok(());
            }
            // 왼쪽 라벨 열은 산문이 아니라 **필드 식별자 열**이라 번역하지 않는다.
            // 등록 kind 는 프로필이 실제로 갖는 필드명을 그대로 찍고(`shell` /
            // `port_mode` / `remote_tasty` / `ssh_ref` / `extra_options` 는
            // `[profile.fields]` 의 toml 키 그 자체다), 미등록 kind 는 아래
            // fallback 이 `p.fields` 의 raw 키를 같은 열에 찍는다. 등록 kind 쪽만
            // 번역하면 한 명령의 같은 열이 절반은 번역어, 절반은 raw 필드명이 된다.
            // (`--json` 키와 일치해서가 아니다 — `--json` 은 `kind` 를 내보내고
            // `destination` / `status` / `label` / `passkey` / `extra_options` 는
            // 아예 없다. 라벨 옆의 자연어만 번역 대상이다.)
            println!("name          : {}", p.name);
            println!("type          : {}", p.kind);
            if let Some(l) = &p.label {
                println!("label         : {l}");
            }
            if let Some(pk) = &p.passkey_ref {
                let status = if passkeys.get(pk).is_some() {
                    ""
                } else {
                    t("cli.remote_profile.show_passkey_missing")
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
                println!("shell         : {}", v.shell());
                if v.is_disabled() {
                    println!(
                        "status        : {}",
                        t_fmt("cli.remote_profile.show_status_disabled", name)
                    );
                }
            } else if let Some(a) = p.as_attach() {
                match a.ssh_ref() {
                    Some(r) => {
                        println!(
                            "ssh_ref       : {r} {}",
                            t("cli.remote_profile.show_ref_note")
                        )
                    }
                    None => {
                        println!("destination   : {}", a.ssh_destination());
                        if let Some(port) = a.port() {
                            println!("port          : {port}");
                        }
                        if !a.extra_options().is_empty() {
                            println!("extra_options : {}", a.extra_options().join(", "));
                        }
                    }
                }
                println!("remote_tasty  : {}", a.remote_tasty());
                println!("port_mode     : {}", a.port_mode());
                if let Some(pf) = a.port_file() {
                    println!("port_file     : {pf}");
                }
            } else {
                for (k, val) in &p.fields {
                    println!("{k:<14}: {val:?}");
                }
            }
            Ok(())
        }
        RemoteProfileCommands::Edit {
            name,
            host,
            user,
            port,
            identity,
            options,
            ssh_ref,
            remote_tasty,
            port_mode,
            port_file,
            shell,
            label,
        } => {
            if let Some(s) = shell
                && !is_valid_shell(s)
            {
                anyhow::bail!("{}", t_fmt("cli.remote_profile.unknown_shell", s));
            }
            let mut profiles = RemoteProfiles::load();
            let mut passkeys = Passkeys::load();
            let Some(mut p) = profiles.get(name).cloned() else {
                anyhow::bail!("{}", t_fmt("cli.remote_profile.not_found", name));
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
            if let Some(r) = ssh_ref {
                p.set_field("ssh_ref", r.clone());
            }
            if let Some(rt) = remote_tasty {
                p.set_field("remote_tasty", rt.clone());
            }
            if let Some(pm) = port_mode {
                p.set_field("port_mode", pm.clone());
            }
            if let Some(pf) = port_file {
                p.set_field("port_file", pf.clone());
            }
            if label.is_some() {
                p.label = label.clone();
            }
            // ssh kind 에 --shell 이 주어지면 셸 갱신 + 발견 모드 재도출/재감지.
            let detect = if let Some(s) = shell {
                p.set_field("shell", s.clone());
                if p.kind == "ssh" {
                    crate::ssh::apply_shell_to_profile(&mut p, &passkeys)
                } else {
                    None
                }
            } else {
                None
            };
            profiles.upsert(p);
            passkeys.save()?;
            profiles.save()?;
            println!("{}", t_fmt("cli.remote_profile.updated", name));
            report_detect(name, &detect);
            Ok(())
        }
        RemoteProfileCommands::Detect { name } => {
            let profiles = RemoteProfiles::load();
            let Some(p) = profiles.get(name).cloned() else {
                anyhow::bail!("{}", t_fmt("cli.remote_profile.not_found", name));
            };
            if p.as_attach().is_some() {
                return detect_attach(&profiles, &p);
            }
            // ssh kind(또는 기타): 셸 감지 프로브.
            match crate::ssh::detect_and_persist(name) {
                Ok(mode) => println!(
                    "{}",
                    t_fmt2("cli.remote_profile.detect_ok", name, mode.as_str())
                ),
                Err(e) => println!(
                    "{}",
                    t_args(
                        "cli.remote_profile.detect_failed",
                        &[&e.to_string(), name, name]
                    )
                ),
            }
            Ok(())
        }
        RemoteProfileCommands::Remove { name } => {
            let mut profiles = RemoteProfiles::load();
            if profiles.remove(name) {
                profiles.save()?;
                println!("{}", t_fmt("cli.remote_profile.removed", name));
            } else {
                anyhow::bail!("{}", t_fmt("cli.remote_profile.not_found", name));
            }
            Ok(())
        }
        RemoteProfileCommands::ListLocal { json } => list_local(*json),
        RemoteProfileCommands::Import { from, name, label } => {
            let mut profiles = RemoteProfiles::load();
            let p = prepare_import(&profiles, &enumerate_hosts(), from, name, label.clone())
                .map_err(import_error_message)?;
            profiles.upsert(p);
            profiles.save()?;
            println!("{}", t_fmt2("cli.remote_profile.import_added", name, from));
            // 셸 감지는 SSH 접속을 발생시키므로 가져오기에 묶지 않는다 — 여러 건을
            // 가져올 때 접속이 연쇄로 일어난다. 필요할 때 사용자가 직접 돌린다.
            println!("{}", t_fmt("cli.remote_profile.import_detect_hint", name));
            Ok(())
        }
    }
}

/// [`ImportError`] 를 사용자 문구로. 크레이트는 문자열을 갖지 않으므로 여기서 만든다.
fn import_error_message(e: ImportError) -> anyhow::Error {
    match e {
        ImportError::UnknownAlias(a) => {
            anyhow::anyhow!("{}", t_fmt("cli.remote_profile.import_unknown_alias", &a))
        }
        ImportError::NameTaken(n) => {
            anyhow::anyhow!("{}", t_fmt("cli.remote_profile.import_name_taken", &n))
        }
    }
}

/// `list-local` — 로컬 ssh config 의 alias 열거. ssh 를 실행하지 않는 순수 파일 읽기다.
fn list_local(json: bool) -> Result<()> {
    let hosts = enumerate_hosts();
    let profiles = RemoteProfiles::load();
    if json {
        let arr: Vec<_> = hosts
            .iter()
            .map(|h| {
                serde_json::json!({
                    "name": h.alias,
                    "source": h.source.display().to_string(),
                    "hostname": h.hostname,
                    "user": h.user,
                    "port": h.port,
                    "imported_as": imported_as(&profiles, &h.alias),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }
    if hosts.is_empty() {
        // 빈 목록의 이유는 셋 중 하나고, 사용자가 할 일이 전부 다르다 — 파일을
        // 만든다 / 권한을 고친다 / Host 항목을 추가한다. IPC 쪽은 같은 구분을
        // `config_exists`·`config_readable` 로 실어 보낸다.
        let path = user_config_path();
        let avail = config_availability(path.as_deref());
        let shown = path
            .map(|p| tasty_utils::path::tilde_abbreviate(&p))
            .unwrap_or_else(|| "~/.ssh/config".into());
        let key = if !avail.exists {
            "cli.remote_profile.list_local_no_config"
        } else if !avail.readable {
            "cli.remote_profile.list_local_unreadable"
        } else {
            "cli.remote_profile.list_local_no_alias"
        };
        println!("{}", t_fmt(key, &shown));
        return Ok(());
    }
    // 헤더는 `list` 와 같은 이유로 패딩 포함 한 줄 전체를 번역 값으로 둔다.
    println!("{}", t("cli.remote_profile.list_local_header"));
    for h in &hosts {
        println!(
            "{:<20} {:<28} {:<20} {}",
            elide(&h.alias, 20),
            elide(&tasty_utils::path::tilde_abbreviate(&h.source), 28),
            elide(&target_hint(h), 20),
            imported_as(&profiles, &h.alias).unwrap_or("-"),
        );
    }
    Ok(())
}

/// 표시용 hint 한 칸 — `HostName[:Port]`(둘 다 없으면 빈 칸). 프로필에는 저장되지
/// 않는 값이라 "이 alias 가 어디를 가리키나" 를 눈으로 확인하는 용도다.
fn target_hint(h: &SshConfigHost) -> String {
    match (&h.hostname, h.port) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.clone(),
        (None, Some(port)) => format!(":{port}"),
        (None, None) => "-".into(),
    }
}

/// 표 한 칸을 넘기는 값은 말줄임한다. alias 는 사용자가 쓴 로컬 파일에서 오므로 길이
/// 상한이 없다 — 열이 밀려 표가 통째로 어긋나는 것만 막는다(`--json` 은 원본 그대로).
fn elide(s: &str, width: usize) -> String {
    if s.chars().count() <= width {
        return s.to_string();
    }
    let head: String = s.chars().take(width.saturating_sub(1)).collect();
    format!("{head}…")
}

/// tasty-attach 프로필의 원격 포트를 실제로 발견해 검증한다(SSH 접속 1회).
fn detect_attach(profiles: &RemoteProfiles, p: &RemoteProfile) -> Result<()> {
    let passkeys = Passkeys::load();
    let (target, remote_tasty, port_mode, port_file) =
        crate::ssh::resolve_attach_target(p, profiles, &passkeys)?;
    let ssh = crate::ssh::resolve_ssh_path();
    let mode = crate::ssh::PortMode::parse(&port_mode)?;
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);
    match crate::ssh::discover_remote_port(
        &ssh,
        &target,
        &remote_tasty,
        mode,
        verify,
        debug,
        port_file.as_deref(),
    ) {
        Ok(port) => {
            println!(
                "{}",
                t_fmt2(
                    "cli.remote_profile.attach_detect_ok",
                    &p.name,
                    &port.to_string()
                )
            );
            Ok(())
        }
        Err(e) => {
            println!(
                "{}",
                t_fmt("cli.remote_profile.attach_detect_failed", &e.to_string())
            );
            Ok(())
        }
    }
}

/// list --json 한 항목. kind 별 대표 필드를 노출한다.
fn profile_json(p: &RemoteProfile) -> serde_json::Value {
    if let Some(v) = p.as_ssh() {
        serde_json::json!({
            "name": p.name,
            "kind": p.kind,
            "host": v.host(),
            "user": v.user(),
            "port": v.port(),
            "passkey_ref": p.passkey_ref,
            "shell": v.shell(),
            "detect_failed": v.detect_failed(),
        })
    } else if let Some(a) = p.as_attach() {
        serde_json::json!({
            "name": p.name,
            "kind": p.kind,
            "ssh_ref": a.ssh_ref(),
            "host": a.host(),
            "port": a.port(),
            "passkey_ref": p.passkey_ref,
            "remote_tasty": a.remote_tasty(),
            "port_mode": a.port_mode(),
            "port_file": a.port_file(),
        })
    } else {
        serde_json::json!({ "name": p.name, "kind": p.kind })
    }
}

/// list 표의 (HOST/REF, DETAIL) 두 컬럼을 kind 별로 채운다.
fn summarize(p: &RemoteProfile) -> (String, String) {
    if let Some(v) = p.as_ssh() {
        (v.ssh_destination(), format!("shell={}", v.shell()))
    } else if let Some(a) = p.as_attach() {
        let host_ref = match a.ssh_ref() {
            Some(r) => format!("ref:{r}"),
            None => a.ssh_destination(),
        };
        (host_ref, format!("tasty={}", a.remote_tasty()))
    } else {
        (String::new(), String::new())
    }
}

/// list STATUS 컬럼. 비활성(ssh 감지 실패)·미등록 타입을 표시.
fn status_of(p: &RemoteProfile) -> &'static str {
    if p.as_ssh().map(|v| v.is_disabled()).unwrap_or(false) {
        t("cli.remote_profile.status_detect_failed")
    } else if !p.is_builtin_kind() {
        t("cli.remote_profile.status_unknown_kind")
    } else {
        ""
    }
}

/// `apply_shell_to_profile` 결과를 사용자에게 출력한다. 명시 셸(None)은 조용히 넘어간다.
fn report_detect(name: &str, detect: &Option<Result<crate::ssh::PortMode>>) {
    match detect {
        Some(Ok(mode)) => println!(
            "{}",
            t_fmt("cli.remote_profile.auto_detect_ok", mode.as_str())
        ),
        Some(Err(e)) => println!(
            "{}",
            t_args(
                "cli.remote_profile.auto_detect_failed",
                &[&e.to_string(), name, name]
            )
        ),
        None => {}
    }
}
