#![forbid(unsafe_code)]

//! 시스템 ssh 위임 — SSH 1회성 터널 전송 (attach/detach 단계 5).
//!
//! tasty 는 자체 원격 프로토콜/암호화를 만들지 않고 **시스템 ssh 에 위임**한다.
//! 이 크레이트는 attach **client 측**에만 존재한다 — CLI 와 본체 GUI 가 함께 쓰고,
//! IPC 서버는 SSH 를 전혀 모르고 loopback(`127.0.0.1`) 만 안다. "원격성" 은 전부
//! client 가 흡수한다 (`docs/adr/0007-attach-targets-remote.md`).
//!
//! SSH *프로토콜* 을 구현하지 않는다 — 시스템 `ssh` 바이너리를 프로세스로 띄우고
//! 그 수명·터널·포트 발견·취소를 관리하는 위임 계층이다.
//!
//! 흐름(단계 4 attach 를 SSH 너머로):
//! 1. [`resolve_ssh_path`] — 시스템 ssh 경로(Windows 는 System32 OpenSSH 풀경로).
//! 2. [`discover_remote_port`] — 원격 tasty 데몬의 IPC 포트 발견(auto fallback 체인).
//! 3. [`SshTunnel::establish`] — `ssh -L 127.0.0.1:local:127.0.0.1:remote -N` 백그라운드.
//! 4. client 가 `127.0.0.1:local` 로 단계 4 attach (commands::attach).
//!
//! # 공개 계약
//!
//! 크레이트 밖에서 쓰는 것은 아래가 전부다. 나머지 `pub` 이 아닌 항목은 내부
//! 구현이고, `#[doc(hidden)]` 이 붙은 항목은 `tasty-cli` 커맨드 구현 전용이라
//! 계약에 포함되지 않는다.
//!
//! | 항목 | 소비자 |
//! |------|--------|
//! | [`SshTarget`] · [`PortMode`] · [`Backoff`] | 본체 · CLI |
//! | [`SshTunnel`] · [`SshCancel`] ([`SshCancelScope`] 는 그 반환형) | 본체 · CLI |
//! | [`resolve_ssh_path`] · [`resolve_attach_target`] · [`discover_remote_port`] | 본체 · CLI |
//! | [`detect_and_persist`] · [`tunnel_drop_totals`] | 본체 |
//! | [`SSH_CONNECT_TIMEOUT`] · [`PORT_DISCOVERY_STEP_TIMEOUT`] · [`PORT_DISCOVERY_TOTAL_TIMEOUT`] | 소비자가 진행 표시·문구를 같은 값에 맞추도록 노출(`docs/adr/0070-port-discovery-timeout.md`) |
//! | [`PortDiscoveryError`] · [`PortDiscoveryFailureKind`] | 실패 원인으로 분기하려는 소비자 |
//!
//! Windows 는 반드시 시스템 OpenSSH 풀경로를 쓴다 — git 번들 ssh 는 윈도우
//! ssh-agent(named pipe `\\.\pipe\openssh-ssh-agent`) 를 못 봐 무암호 인증이 실패한다.

use std::cell::RefCell;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tasty_remote_profiles::{Passkeys, RemoteProfile, RemoteProfiles, shell_to_port_mode};

/// 시스템 ssh 바이너리 경로.
///
/// Windows 는 시스템 OpenSSH 풀경로(`%WINDIR%\System32\OpenSSH\ssh.exe`)를 우선
/// 한다 — git 번들 ssh(`C:\Program Files\Git\usr\bin\ssh.exe`)는 윈도우 ssh-agent 를
/// 못 보기 때문. 풀경로 미발견 시 PATH 의 `ssh.exe` 로 fallback(경고 로그).
/// mac/linux 는 PATH 의 `ssh` 가 보편적이라 그대로 사용한다.
pub fn resolve_ssh_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            let full = PathBuf::from(windir).join(r"System32\OpenSSH\ssh.exe");
            if full.exists() {
                return full;
            }
        }
        tracing::warn!(
            "시스템 OpenSSH(%WINDIR%\\System32\\OpenSSH\\ssh.exe) 미발견 — PATH 의 ssh.exe 로 \
             fallback. git 번들 ssh 면 윈도우 agent 를 못 봐 무암호 인증이 실패할 수 있음."
        );
        PathBuf::from("ssh.exe")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("ssh")
    }
}

/// SSH 접속 대상. tasty 는 파싱하지 않고 ssh 에 그대로 위임한다
/// (`~/.ssh/config` 의 `Host` alias / `ProxyJump` / 포트 전부 ssh 가 해석).
///
/// 단계 7: 저장 프로필의 `identity_file`/`extra_options` 도 함께 실어 ssh 에 전달한다
/// (`push_common_opts` 가 `-i`/`-o` 로 emit). 1회성(`--ssh`) 경로는 이 둘이 비어
/// 동작이 불변하다.
#[derive(Clone, Debug, Default)]
pub struct SshTarget {
    /// ssh 에 그대로 넘길 destination (`user@host` | `host` | config alias).
    pub destination: String,
    /// 사용자가 명시한 ssh 포트(없으면 ssh config / 22 위임).
    pub ssh_port: Option<u16>,
    /// identity 파일 경로(`-i`). `~` 는 spawn 시 직접 확장(셸 비경유라 ssh 가 못 풂).
    pub identity_file: Option<String>,
    /// 추가 ssh `-o` 옵션. `"Key=Value"` → `-o Key=Value`.
    pub extra_options: Vec<String>,
}

impl SshTarget {
    /// `user@host` / `host` / config alias 를 그대로 보관한다. ssh 포트는
    /// `~/.ssh/config` 에 위임하므로 여기서는 destination 만 받는다(1회성 `--ssh`).
    pub fn parse(dest: &str) -> Self {
        Self {
            destination: dest.to_string(),
            ssh_port: None,
            identity_file: None,
            extra_options: Vec::new(),
        }
    }

    /// 저장 프로필을 ssh 연결 대상으로 변환한다(단계 7 `attach --profile` / 자동 attach).
    /// destination = `user@host` 합성, port/extra_options 결선. `identity_file` 은
    /// `passkey_ref` → [`Passkeys`] 에서 resolve 한 **파일 경로**(inline/path 무관).
    ///
    /// ssh kind 가 아니거나(`attach` 는 ssh 전용) passkey 참조가 깨졌으면 에러.
    pub fn from_remote_profile(p: &RemoteProfile, passkeys: &Passkeys) -> Result<Self> {
        let v = p.as_ssh().ok_or_else(|| {
            anyhow::anyhow!("attach 는 ssh kind 프로필만 지원합니다 (kind='{}')", p.kind)
        })?;
        let identity_file = match &p.passkey_ref {
            Some(name) => Some(
                passkeys
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("passkey '{name}' 을 찾을 수 없습니다"))?
                    .path
                    .clone(),
            ),
            None => None,
        };
        Ok(Self {
            destination: v.ssh_destination(),
            ssh_port: v.port(),
            identity_file,
            extra_options: v.extra_options(),
        })
    }

    /// **인라인 tasty-attach** 프로필을 ssh 연결 대상으로 변환한다(ssh_ref 없는 경우).
    /// 연결 필드는 attach 프로필 자신의 fields 에서 읽는다([`AttachView`] 인라인 모드가
    /// [`SshView`] 로직을 위임 재사용). `identity_file` 은 `passkey_ref` → [`Passkeys`].
    pub fn from_attach_inline(p: &RemoteProfile, passkeys: &Passkeys) -> Result<Self> {
        let v = p.as_attach().ok_or_else(|| {
            anyhow::anyhow!(
                "인라인 attach 대상이 tasty-attach kind 가 아닙니다 (kind='{}')",
                p.kind
            )
        })?;
        let identity_file = match &p.passkey_ref {
            Some(name) => Some(
                passkeys
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("passkey '{name}' 을 찾을 수 없습니다"))?
                    .path
                    .clone(),
            ),
            None => None,
        };
        Ok(Self {
            destination: v.ssh_destination(),
            ssh_port: v.port(),
            identity_file,
            extra_options: v.extra_options(),
        })
    }
}

/// attach 소비자용 헬퍼 — tasty-attach 프로필을
/// `(SshTarget, remote_tasty, port_mode, port_file)` 로 resolve 한다.
///
/// 연결 정보(SshTarget)는 두 갈래:
/// - `ssh_ref` 있으면 → `profiles` 에서 참조 ssh 프로필을 **매 resolve 마다 재로드**해
///   (라이브 팔로우) `SshTarget::from_remote_profile` 로 결선. dangling ref 는 명확한
///   에러(목록/GUI 표시는 이 에러를 잡아 소프트 배지로).
/// - `ssh_ref` 없으면(인라인) → attach 프로필 자체 fields 로 `from_attach_inline`.
///
/// 비활성 게이트: 유효 ssh 소스(참조 ssh 프로필 or 인라인)의 `detect_failed`.
/// attach 전용 remote_tasty/port_mode/port_file 은 tasty-attach 프로필이 소유한다.
pub fn resolve_attach_target(
    p: &RemoteProfile,
    profiles: &RemoteProfiles,
    passkeys: &Passkeys,
) -> Result<(SshTarget, String, String, Option<String>)> {
    let v = p.as_attach().ok_or_else(|| {
        anyhow::anyhow!(
            "attach 는 tasty-attach kind 프로필만 지원합니다 (kind='{}')",
            p.kind
        )
    })?;

    // 유효 ssh 소스에서 (SshTarget, 비활성, 셸) 을 얻는다. 셸은 port_mode 도출용
    // (detect-split: 셸 감지는 ssh 레이어, port_mode 도출은 attach 레이어 —
    // `docs/adr/0032-remote-attach-two-layer-split.md`).
    let (target, disabled, shell) = match v.ssh_ref() {
        Some(ref_name) => {
            let ssh_profile = profiles.get(ref_name).ok_or_else(|| {
                anyhow::anyhow!(
                    "attach 프로필 '{}' 이 참조하는 ssh 프로필 '{ref_name}' 을 찾을 수 없습니다",
                    p.name
                )
            })?;
            let sv = ssh_profile.as_ssh().ok_or_else(|| {
                anyhow::anyhow!(
                    "attach 프로필 '{}' 의 ssh_ref '{ref_name}' 은 ssh kind 가 아닙니다 (kind='{}')",
                    p.name,
                    ssh_profile.kind
                )
            })?;
            let disabled = sv.is_disabled();
            let shell = sv.shell().to_string();
            (
                SshTarget::from_remote_profile(ssh_profile, passkeys)?,
                disabled,
                shell,
            )
        }
        None => (
            SshTarget::from_attach_inline(p, passkeys)?,
            v.detect_failed(),
            v.shell().to_string(),
        ),
    };
    if disabled {
        bail!(
            "attach 프로필 '{}' 의 ssh 소스가 비활성(셸 감지 실패) — 재감지가 필요합니다",
            p.name
        );
    }
    let remote_tasty = v.remote_tasty().to_string();
    // port_mode 결정: attach 가 명시(auto 이외)했으면 그 값, "auto" 면 ssh 소스의 셸에서
    // 도출(bash→subcommand 등), 셸도 auto/미상이면 "auto"(fallback 체인).
    let explicit = v.port_mode();
    let port_mode = if explicit == "auto" {
        shell_to_port_mode(&shell).unwrap_or("auto").to_string()
    } else {
        explicit.to_string()
    };
    let port_file = v.port_file().map(|s| s.to_string());
    Ok((target, remote_tasty, port_mode, port_file))
}

/// `tasty tool ssh <profile>` — 저장된 ssh 프로필로 대화형 ssh 접속을 띄운다.
///
/// 프로필의 identity/port/user/extra_options 를 조립해 시스템 ssh 를 그대로 spawn 한다
/// (stdio 상속 = 대화형). `command` 가 비어있지 않으면 원격에서 그 명령을 1회 실행한다
/// (`tasty tool ssh gb10 --command hostname`). ssh 종료코드를 그대로 전파한다.
/// **공개 계약 아님** — `tasty-cli` 의 커맨드 구현만 쓴다. 크레이트 밖 소비자가
/// 하나뿐이라 `pub(crate)` 로 못 내릴 뿐이며, 새 소비자가 이걸 쓰기 시작하면
/// 그건 계약 확장이므로 `#[doc(hidden)]` 을 떼고 문서화하는 결정을 먼저 한다.
#[doc(hidden)]
pub fn run_ssh_connect(target: &SshTarget, command: &[String]) -> Result<()> {
    let ssh = resolve_ssh_path();
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let mut args: Vec<String> = Vec::new();
    push_common_opts(&mut args, target, verify);
    args.push(target.destination.clone());
    for c in command {
        args.push(c.clone());
    }
    // stdio 는 기본 상속(대화형 셸). 종료코드를 그대로 프로세스 exit 로 전파한다.
    // 이 함수는 항상 콘솔을 가진 CLI 프로세스(`tasty tool ssh`)에서만 호출되므로
    // hide_console 을 걸지 않는다 — 걸면 상속받아야 할 대화형 stdio 콘솔이 사라진다.
    let status = Command::new(&ssh)
        .args(&args)
        .status()
        .map_err(|e| anyhow::anyhow!("ssh spawn 실패({}): {e}", ssh.display()))?;
    if !status.success() {
        if let Some(code) = status.code() {
            std::process::exit(code);
        }
        bail!("ssh 가 시그널로 종료되었습니다");
    }
    Ok(())
}

/// 경로 앞의 `~` / `~/` 를 홈 디렉토리로 확장한다. ssh 를 셸 없이 spawn 하면 셸의
/// 틸드 확장이 일어나지 않으므로 `-i ~/.ssh/id_ed25519` 가 그대로 깨진다.
fn expand_tilde(path: &str) -> String {
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok();
        if let Some(home) = home {
            let rest = &path[1..]; // 선행 '~' 제거 → "/.ssh/..." or ""
            return format!("{home}{rest}");
        }
    }
    path.to_string()
}

/// 원격 포트 발견 모드(plan §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortMode {
    /// auto 체인 첫 단계: `ssh <dest> <remote-tasty> port`. git bash·unix 에서 성공.
    /// Windows GUI release 바이너리(PowerShell/cmd)는 빈 출력으로 실패할 수 있다.
    Subcommand,
    /// Unix 원격: `cat ~/<.tasty|.tasty-debug>/tasty.port`.
    FileUnix,
    /// Windows 원격: `type %USERPROFILE%\<.tasty|.tasty-debug>\tasty.port`.
    FileWindows,
    /// subcommand → file-unix → file-windows 순서로 시도(기본). 원격 SSH
    /// DefaultShell(PowerShell/cmd/bash/zsh)에 무관하게 4개 셸 전부를 커버한다.
    Auto,
}

impl PortMode {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "auto" => Self::Auto,
            "subcommand" => Self::Subcommand,
            "file-unix" => Self::FileUnix,
            "file-windows" => Self::FileWindows,
            other => bail!(
                "알 수 없는 --remote-port-mode '{other}' (auto|subcommand|file-unix|file-windows)"
            ),
        })
    }

    /// toml/CLI 직렬화용 문자열(`parse` 의 역). 프로필 `port_mode` 필드에 기록.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Subcommand => "subcommand",
            Self::FileUnix => "file-unix",
            Self::FileWindows => "file-windows",
        }
    }
}

/// SSH **연결 수립**(TCP 핸드셰이크 + 배너) 1회의 상한 — ssh(1) `-o ConnectTimeout`.
///
/// 이 값이 없으면 연결 수립은 OS 기본 SYN 재시도에 맡겨진다(리눅스 `tcp_syn_retries=6`
/// = 약 127초). `ServerAliveInterval`/`CountMax` 는 **연결이 수립된 뒤의** keepalive 라
/// 이 구간을 덮지 않는다. ProxyJump 처럼 홉이 여러 개면 ssh 가 홉마다 이 값을 적용한다.
pub const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 포트 발견 **1회**(ssh 자식 프로세스 1개)의 프로세스 레벨 상한.
///
/// [`SSH_CONNECT_TIMEOUT`] 이 덮지 못하는 구간 — 인증 핸드셰이크, 원격 명령 실행,
/// `BatchMode=no` 로 뜬 프롬프트 대기 — 까지 포함해 자식이 이 시간 안에 끝나지 않으면
/// kill 한다. 연결 상한의 2 배라 2-hop ProxyJump 도 정상 경로로 들어간다.
pub const PORT_DISCOVERY_STEP_TIMEOUT: Duration = Duration::from_secs(20);

/// [`discover_remote_port`] / `detect_port_mode` **호출 1회 전체**의 상한.
///
/// auto 체인(3 단계)이나 명시 `port_file`(`cat`→`type` 2 회)처럼 한 호출이 ssh 를 여러 번
/// 띄우므로, 단계 상한만으로는 총 대기가 단계 수만큼 곱해진다. 이 값이 그 곱을 끊는다 —
/// 각 단계는 `min(`[`PORT_DISCOVERY_STEP_TIMEOUT`]`, 남은 예산)` 만 받고, 예산이 소진되면
/// 남은 단계는 ssh 를 띄우지 않고 즉시 타임아웃으로 떨어진다.
pub const PORT_DISCOVERY_TOTAL_TIMEOUT: Duration = Duration::from_secs(45);

/// 한 번의 포트 발견 호출에 허용된 총 예산의 만료 시각. 체인 각 단계에 남은 예산을
/// 나눠준다(단계 상한 × 단계 수로 총 대기가 곱해지는 것을 막는다).
#[derive(Clone, Copy, Debug)]
struct Deadline {
    at: Instant,
}

impl Deadline {
    fn after(budget: Duration) -> Self {
        Self {
            at: Instant::now() + budget,
        }
    }

    fn remaining(self) -> Duration {
        self.at.saturating_duration_since(Instant::now())
    }

    /// 이번 단계에 줄 상한 — 단계 상한과 전체 잔여 예산 중 작은 쪽.
    /// `Duration::ZERO` 면 예산 소진(ssh 를 띄우지 않는다).
    fn step_budget(self) -> Duration {
        PORT_DISCOVERY_STEP_TIMEOUT.min(self.remaining())
    }
}

/// ssh 공통 `-o` 옵션을 인자 벡터에 추가한다.
///
/// - `BatchMode=no`: 첫 연결의 host key/passphrase 프롬프트 허용(무암호면 무영향).
/// - `ServerAliveInterval`/`CountMax`: 네트워크 단절 감지(~45s 내 ssh 자가 종료).
/// - `verify` 시 `StrictHostKeyChecking=accept-new`: 자동 검증 한정(평상시 기본 strict 유지).
/// - `ConnectTimeout`: 연결 수립 상한([`SSH_CONNECT_TIMEOUT`]).
///
/// **`ConnectTimeout` 은 사용자 `extra_options` 보다 뒤에 push 한다** — ssh(1) 은 같은 키가
/// 여러 번 오면 **먼저 나온 값**을 쓰므로, 프로필에 `ConnectTimeout=...` 을 직접 넣은
/// 사용자의 값이 이긴다(기본값은 미지정 시에만 적용되는 fallback).
fn push_common_opts(args: &mut Vec<String>, target: &SshTarget, verify: bool) {
    args.push("-o".into());
    args.push("BatchMode=no".into());
    args.push("-o".into());
    args.push("ServerAliveInterval=15".into());
    args.push("-o".into());
    args.push("ServerAliveCountMax=3".into());
    if verify {
        args.push("-o".into());
        args.push("StrictHostKeyChecking=accept-new".into());
    }
    if let Some(p) = target.ssh_port {
        args.push("-p".into());
        args.push(p.to_string());
    }
    // 단계 7 — 프로필 identity_file / extra_options 결선(1회성 경로는 비어 무영향).
    if let Some(identity) = &target.identity_file {
        args.push("-i".into());
        args.push(expand_tilde(identity));
    }
    for opt in &target.extra_options {
        args.push("-o".into());
        args.push(opt.clone());
    }
    // 사용자 지정이 이기도록 마지막에 — 위 doc 주석 참고.
    args.push("-o".into());
    args.push(format!("ConnectTimeout={}", SSH_CONNECT_TIMEOUT.as_secs()));
}

/// 원격 포트 발견 실패의 원인 분류(로케일 독립 — exit code 기반).
///
/// [`PortDiscoveryError`] 와 함께 **공개 계약**이다. 포트 발견 결과는 `anyhow` 로
/// 감싸 나가지만, 소비자가 문구가 아니라 원인으로 분기해야 할 때(재시도할지, 프로필을
/// 비활성할지) downcast 해 이 `kind` 를 읽는 것이 의도된 경로다 — 그래서 지금 크레이트
/// 밖 참조가 없어도 노출을 유지한다.
///
/// 원격 stderr 문자열 매칭에 의존하지 않는다 — 원격 로케일에 따라 문자열이 달라져
/// 신뢰할 수 없다(실측: 한국어 로케일은 "그런 파일이나 디렉터리가 없습니다", 영어는
/// "No such file or directory"). exit code 는 로케일 무관하게 안정적이다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortDiscoveryFailureKind {
    /// SSH 연결/인증 자체가 실패했다(exit 255, 시그널 종료, 또는 로컬 spawn 실패).
    SshConnectionFailed,
    /// SSH 연결과 원격 명령 실행은 됐으나 포트를 얻지 못했다(원격에 tasty 인스턴스가
    /// 없는 것으로 추정 — 관례 위치의 포트 파일 부재, `tasty port` 실패 등).
    RemoteInstanceNotRunning,
    /// 명령은 성공했으나(exit 0) stdout 에서 포트 숫자를 파싱하지 못했다.
    PortParseFailed,
    /// 상한([`PORT_DISCOVERY_STEP_TIMEOUT`] / [`PORT_DISCOVERY_TOTAL_TIMEOUT`]) 안에
    /// 아무 답도 오지 않았다 — 무응답 호스트(패킷 소멸), 프롬프트 대기, 예산 소진.
    ///
    /// [`SshConnectionFailed`](Self::SshConnectionFailed) 로 접지 않고 따로 두는 이유:
    /// ① 사용자가 취할 조치가 다르다(도달성/회선 점검 vs 인증·호스트키 점검),
    /// ② [`classify_by_exit_code`] 는 시그널 종료를 `SshConnectionFailed` 로 보는데
    /// 타임아웃 kill 도 시그널 종료라, 이 자리가 없으면 우리가 죽인 것과 원격발 시그널
    /// 종료가 같은 분류로 뭉개진다.
    TimedOut,
    /// 호출자가 [`SshCancel::cancel`] 로 중단했다(자식 ssh 는 kill + reaping 됨).
    /// 사용자 의도이므로 실패가 아니지만, 진행 중이던 발견 시도를 끊는 신호로 같은
    /// 에러 타입에 실어 상위로 올린다.
    ///
    /// [`TimedOut`](Self::TimedOut) 과 겹치지 않는다: 상한은 **시간**이 끊은 것이고
    /// 이쪽은 **사용자 의도**로 끊은 것이라, 취소 kill 이 시그널 종료로 관측되어 상한
    /// 초과처럼 보이더라도 취소가 우선한다([`ChildOwner::finish`]).
    Cancelled,
}

impl PortDiscoveryFailureKind {
    /// 사용자 노출용 문구 키(`lang/{en,ko,ja}.toml` `[ssh.port_discovery]`). ssh.rs 는
    /// CLI 전용이 아니라 GUI(remote_attach 팝업)도 공유하므로 `cli.` prefix 를 쓰지
    /// 않는다(`docs/dev-guide/i18n.md` 일반 네이밍 규칙).
    fn i18n_key(self) -> &'static str {
        match self {
            Self::SshConnectionFailed => "ssh.port_discovery.connection_failed",
            Self::RemoteInstanceNotRunning => "ssh.port_discovery.instance_not_running",
            Self::PortParseFailed => "ssh.port_discovery.parse_failed",
            Self::TimedOut => "ssh.port_discovery.timed_out",
            Self::Cancelled => "ssh.port_discovery.cancelled",
        }
    }
}

/// 원격 포트 발견 실패 에러. `kind` 는 사용자 노출/분기 판정용, `detail` 은 원격 raw
/// stderr·파싱 실패 원문을 담되 **`Display` 에 노출하지 않는다** — 로케일 의존 문자열과
/// 내부 디스커버리 구현(포트 파일 경로·`cat`/`type` 명령)이 최종 사용자 문구로 새어나가지
/// 않게 하기 위함(raw stderr 노출 이슈의 근본 수정 지점). 진단이 필요하면 생성 시점에
/// `tracing::debug!` 로 한 번 남긴다 — 상위에서 `anyhow::Context` 로 감싸도(`.with_context`)
/// 이 타입의 `Display` 자체가 안전하므로 체인 어디서 출력되어도 raw stderr 가 섞이지 않는다.
#[derive(Debug)]
pub struct PortDiscoveryError {
    pub kind: PortDiscoveryFailureKind,
    detail: String,
}

impl PortDiscoveryError {
    fn new(kind: PortDiscoveryFailureKind, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        tracing::debug!(?kind, %detail, "원격 포트 발견 실패");
        Self { kind, detail }
    }

    /// 진단용 원문(로그 전용) — `Display` 에는 포함하지 않는다.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for PortDiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", tasty_i18n::t(self.kind.i18n_key()))
    }
}

impl std::error::Error for PortDiscoveryError {}

/// `ssh` 프로세스 종료코드를 원인 분류로 매핑한다(순수 함수 — 단위 테스트 대상).
///
/// exit 255 는 ssh(1) 자체의 연결/인증 실패 관례 코드(OpenSSH 매뉴얼). 시그널 종료
/// (`code=None`) 도 원격 명령까지 도달했다고 볼 수 없어 같은 분류. 그 외 코드는 원격
/// 셸이 명령을 실행하고 낸 종료코드 — "연결은 됐다"는 확정 신호라 인스턴스 미실행으로
/// 본다. 원격 명령이 이론상 255 를 자체적으로 반환할 가능성은 남지만(문서 한계로 명시),
/// exit code 가 원격 로케일에 무관한 유일한 신호라 그 한계를 감수한다.
fn classify_by_exit_code(code: Option<i32>) -> PortDiscoveryFailureKind {
    match code {
        Some(255) | None => PortDiscoveryFailureKind::SshConnectionFailed,
        Some(_) => PortDiscoveryFailureKind::RemoteInstanceNotRunning,
    }
}

/// [`classify_by_exit_code`] 결과를 **소요 시간**으로 한 번 더 좁힌다(순수 함수).
///
/// `ConnectTimeout` 이 걸린 뒤로 무응답 호스트의 지배적 결말은 "ssh 가 스스로 상한을 세고
/// exit 255" 다 — exit code 만 보면 인증 실패와 구분되지 않아 사용자가 엉뚱한 곳(키/호스트키)
/// 을 뒤지게 된다. ssh 가 [`SSH_CONNECT_TIMEOUT`] 을 다 쓰고 연결 실패로 끝났으면 그건
/// 시간으로 끊긴 것이므로 [`TimedOut`](PortDiscoveryFailureKind::TimedOut) 으로 본다.
/// 판정 입력이 exit code + 경과 시간뿐이라 원격 로케일에 의존하지 않는다(이 모듈의
/// "stderr 문자열 매칭 금지" 원칙 유지).
fn refine_with_elapsed(
    kind: PortDiscoveryFailureKind,
    elapsed: Duration,
) -> PortDiscoveryFailureKind {
    if kind == PortDiscoveryFailureKind::SshConnectionFailed && elapsed >= SSH_CONNECT_TIMEOUT {
        return PortDiscoveryFailureKind::TimedOut;
    }
    kind
}

// ════════════════════════════════════════════════════════════════════════
// 포트 발견 자식 ssh 의 취소(kill) 핸들
// ════════════════════════════════════════════════════════════════════════

// ════════════════════════════════════════════════════════════════════════
// 포트 발견 자식 ssh 의 취소(kill) 핸들
// ════════════════════════════════════════════════════════════════════════

/// 진행 중인 **포트 발견** 자식 ssh 를 다른 스레드에서 중단하기 위한 핸들.
///
/// 포트 발견은 이미 [`PORT_DISCOVERY_STEP_TIMEOUT`] / [`PORT_DISCOVERY_TOTAL_TIMEOUT`]
/// 으로 **시간** 상한이 걸려 있지만, 그건 상한이 다 찰 때까지는 아무도 못 끊는다는 뜻도
/// 된다 — 사용자가 조회를 중단해도 자식 ssh 가 상한까지 살아 있다. 이 핸들이 그 상한
/// **이전에** 끊는 **사용자 의도** 경로다. 워커 스레드가 [`SshCancel::scope`] 로 자신을
/// 등록하면, 그 스레드에서 실행되는 모든 포트 발견 자식이 이 핸들에 붙어
/// [`SshCancel::cancel`] 로 kill + reaping 된다([`SshTunnel`] 이 Drop 으로 자기 자식을
/// 회수하는 것과 같은 계약을 그 앞단에도 주는 셈이다).
///
/// 스레드로컬 스코프를 쓰는 이유: 포트 발견은 `resolve_endpoint` →
/// `discover_remote_port` → 단계별 프로브로 이어지는 깊은 호출 사슬이고, 취소가 필요한
/// 것은 그중 GUI/IPC 워커 경로뿐이다. 전 경로에 취소 인자를 흘리는 대신 "이 스레드에서
/// 도는 발견 작업" 이라는 자연스러운 경계에 붙인다.
#[derive(Clone, Default)]
pub struct SshCancel {
    inner: Arc<CancelInner>,
}

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    /// 지금 실행 중인 포트 발견 자식(단계마다 교체). kill 하는 쪽이 여기서 take 한다 —
    /// `Child::kill` 이 `&mut self` 를 요구하므로 소유권 이동으로 배타를 만든다.
    child: Mutex<Option<Child>>,
}

thread_local! {
    /// 현재 스레드에 설치된 취소 핸들([`SshCancel::scope`]).
    static CURRENT_CANCEL: RefCell<Option<SshCancel>> = const { RefCell::new(None) };
}

impl SshCancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 이 핸들을 **현재 스레드**의 포트 발견 취소 대상으로 설치한다. 반환된 가드가
    /// drop 될 때 이전 설치 상태로 되돌린다(중첩 안전).
    pub fn scope(&self) -> SshCancelScope {
        let prev = CURRENT_CANCEL.with(|c| c.borrow_mut().replace(self.clone()));
        SshCancelScope { prev }
    }

    /// 취소를 요청한다 — 등록된 자식 ssh 를 kill + wait(좀비 방지)하고, 이후의 발견
    /// 시도도 spawn 되지 않게 플래그를 세운다. 여러 번 불러도 안전하다.
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        let child = lock_slot(&self.inner.child).take();
        if let Some(mut child) = child {
            kill_and_reap(&mut child);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// 실행 중 자식을 등록한다. 이미 취소된 뒤라면 등록을 거부하고 자식을 돌려준다
    /// (호출자가 즉시 kill 한다) — 취소와 spawn 사이의 레이스에서 자식이 새지 않게.
    fn register(&self, child: Child) -> Result<(), Child> {
        if self.is_cancelled() {
            return Err(child);
        }
        // 발견 단계는 직렬이라 슬롯이 비어 있어야 정상. 혹시 남아 있으면 이전 단계
        // 자식이 회수되지 않은 것이므로 여기서 정리한다.
        if let Some(mut stale) = lock_slot(&self.inner.child).replace(child) {
            kill_and_reap(&mut stale);
        }
        Ok(())
    }

    /// 자식을 돌려받는다. `None` 이면 [`SshCancel::cancel`] 이 이미 가져가 kill 했다.
    fn reclaim(&self) -> Option<Child> {
        lock_slot(&self.inner.child).take()
    }
}

/// [`SshCancel::scope`] 의 RAII 가드.
///
/// 직접 만들 일은 없지만 `pub` 이어야 한다 — `SshCancel::scope()` 의 반환형이라
/// 내리면 private-in-public 에 걸린다.
pub struct SshCancelScope {
    prev: Option<SshCancel>,
}

impl Drop for SshCancelScope {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CURRENT_CANCEL.with(|c| *c.borrow_mut() = prev);
    }
}

/// 현재 스레드에 설치된 취소 핸들.
fn current_cancel() -> Option<SshCancel> {
    CURRENT_CANCEL.with(|c| c.borrow().clone())
}

/// 현재 스레드의 포트 발견이 취소됐는지(스코프가 없으면 false).
fn cancel_requested() -> bool {
    current_cancel().is_some_and(|c| c.is_cancelled())
}
/// 자식 슬롯 잠금. 이 뮤텍스 안에서 하는 일은 `Option<Child>` 의 take/replace 뿐이라
/// 패닉 지점이 없다 — poisoned 는 도달 불가지만, 도달하더라도 자식을 회수하지 못해
/// 프로세스가 새는 쪽이 더 나쁘므로 값을 복구해 계속 진행한다.
/// 자식 슬롯 락의 poison 복구 공용 보고 좌표(첫-1 회). 임계구역은 `Option<Child>` 의
/// take/replace 뿐이라 복구가 안전하다 — 틀린 것은 흔적이 없다는 것이었다.
const CHILD_SLOT_WHAT: &str = "the ssh child slot";
static CHILD_SLOT_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn lock_slot(m: &Mutex<Option<Child>>) -> std::sync::MutexGuard<'_, Option<Child>> {
    tasty_utils::poison::recover_mutex(m.lock(), CHILD_SLOT_WHAT, &CHILD_SLOT_POISON_REPORTED)
}

/// 타임아웃으로 끊은 자식을 kill + wait 해 좀비를 남기지 않는다
/// ([`SshTunnel::drop`] 과 같은 패턴 — kill 은 이미 종료된 자식에서 실패할 수 있다).
fn kill_and_reap(child: &mut Child) {
    if let Err(e) = child.kill() {
        tracing::debug!("타임아웃 ssh kill 실패(이미 종료됐을 수 있음): {e}");
    }
    if let Err(e) = child.wait() {
        tracing::debug!("타임아웃 ssh reaping 실패: {e}");
    }
}

/// 취소로 끊긴 시도의 에러. `what` 은 detail(로그 전용) 에만 들어간다.
fn cancelled_error(what: &str) -> PortDiscoveryError {
    PortDiscoveryError::new(
        PortDiscoveryFailureKind::Cancelled,
        format!("{what}: 사용자 취소로 중단"),
    )
}

/// [`run_capture_with_budget`] 자식의 소유권 — 취소 스코프가 설치돼 있으면 그쪽에 맡기고
/// (다른 스레드가 상한 만료 전에 kill 할 수 있다), 없으면 이 함수가 그대로 들고 있는다.
///
/// 어느 쪽이든 상한 감시([`wait_with_timeout`])는 이 타입을 통해 `try_wait` 을 돈다 —
/// 스코프에 맡긴 자식도 폴링마다 잠깐 잠그고 보므로 감시와 취소가 같은 핸들을 공유한다.
enum ChildOwner {
    Local(Child),
    Scoped(SshCancel),
}

/// [`ChildOwner::try_exited`] 결과.
enum ChildPoll {
    /// 자식이 종료했다(또는 `try_wait` 자체가 실패해 더 볼 수 없다).
    Exited,
    /// 아직 실행 중.
    Running,
    /// 취소가 자식을 가져갔다 — 더 기다릴 대상이 없다.
    Taken,
}

impl ChildOwner {
    fn adopt(child: Child, what: &str) -> Result<Self, PortDiscoveryError> {
        match current_cancel() {
            Some(handle) => match handle.register(child) {
                Ok(()) => Ok(Self::Scoped(handle)),
                Err(mut rejected) => {
                    kill_and_reap(&mut rejected);
                    Err(cancelled_error(what))
                }
            },
            None => Ok(Self::Local(child)),
        }
    }

    /// 취소가 요청됐는지(스코프에 맡기지 않은 자식은 취소 대상이 아니라 항상 false).
    fn is_cancelled(&self) -> bool {
        match self {
            Self::Local(_) => false,
            Self::Scoped(handle) => handle.is_cancelled(),
        }
    }

    fn try_exited(&mut self) -> ChildPoll {
        fn poll(child: &mut Child) -> ChildPoll {
            match child.try_wait() {
                Ok(Some(_)) => ChildPoll::Exited,
                Ok(None) => ChildPoll::Running,
                // try_wait 이 실패하면 더 기다려도 상태를 알 수 없다 — 종료로 간주하고
                // 호출자의 출력 수집이 같은 에러를 다시 만나 분류하게 둔다.
                Err(e) => {
                    tracing::debug!("ssh try_wait 실패 — 폴링 중단: {e}");
                    ChildPoll::Exited
                }
            }
        }
        match self {
            Self::Local(child) => poll(child),
            Self::Scoped(handle) => match lock_slot(&handle.inner.child).as_mut() {
                Some(child) => poll(child),
                None => ChildPoll::Taken,
            },
        }
    }

    /// 상한 초과로 끊는다 — kill + wait 로 좀비를 남기지 않는다.
    fn kill_and_reap_now(self) {
        let child = match self {
            Self::Local(child) => Some(child),
            // 취소가 이미 가져갔으면 그쪽이 kill + wait 을 끝냈다.
            Self::Scoped(handle) => handle.reclaim(),
        };
        if let Some(mut child) = child {
            kill_and_reap(&mut child);
        }
    }

    /// 자식을 회수해 출력 수집에 넘긴다. 취소가 가져갔거나 취소 플래그가 선 뒤라면
    /// 결과 대신 [`PortDiscoveryFailureKind::Cancelled`] 를 돌려준다 — 취소로 죽은
    /// 자식의 시그널 종료가 다른 사유(연결 실패/상한)로 분류되지 않게 한다.
    fn finish(self, what: &str) -> Result<Child, PortDiscoveryError> {
        match self {
            Self::Local(child) => Ok(child),
            Self::Scoped(handle) => match handle.reclaim() {
                // cancel() 이 자식을 가져가 이미 kill + wait 했다.
                None => Err(cancelled_error(what)),
                Some(mut child) if handle.is_cancelled() => {
                    // 회수와 취소가 겹친 경우 — 여기서 마저 정리한다.
                    kill_and_reap(&mut child);
                    Err(cancelled_error(what))
                }
                Some(child) => Ok(child),
            },
        }
    }
}

/// 자식이 `budget` 안에 끝나면 `true`, 상한을 넘겼으면 `false`(kill 은 호출자 책임).
/// 취소가 자식을 가져간 경우도 `true` — 더 기다릴 대상이 없다는 뜻이고, 그 사유는
/// [`ChildOwner::finish`] 가 [`PortDiscoveryFailureKind::Cancelled`] 로 확정한다.
///
/// 이 크레이트에는 async 런타임이 없으므로 [`SshTunnel::wait_ready`] 와 같은 `try_wait`
/// 폴링을 쓴다. 폴링 간격은 5ms 에서 시작해 50ms 까지 배로 늘린다 — 정상 경로(수백 ms)의
/// 지연을 거의 더하지 않으면서 장기 대기의 wakeup 횟수를 억제한다.
fn wait_with_timeout(owner: &mut ChildOwner, budget: Duration) -> bool {
    let deadline = Instant::now() + budget;
    let mut nap = Duration::from_millis(5);
    loop {
        match owner.try_exited() {
            ChildPoll::Exited | ChildPoll::Taken => return true,
            ChildPoll::Running => {}
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(nap.min(left));
        nap = (nap * 2).min(Duration::from_millis(50));
    }
}

/// 이미 조립된 ssh `Command` 를 상한 안에서 실행한다(포트 발견용 — 프로세스 레벨 감시).
///
/// `Command::output()` 을 쓰지 않는 이유: `output()` 은 자식이 끝날 때까지 무기한 블록하고
/// `Child` 핸들도 주지 않아 중간에 끊을 수단이 없다. 여기서는 직접 `spawn` 해 핸들을 쥐고
/// [`wait_with_timeout`] 으로 감시한다.
///
/// stdout/stderr 를 파이프로 잡으므로 원격이 파이프 버퍼(수십 KB)를 넘겨 쓰면 자식이
/// 블록될 수 있는데, 그 경우도 상한에서 kill 되므로 hang 은 아니다(포트 발견 출력은
/// 한 줄이라 정상 경로에서는 발생하지 않는다).
///
/// 상한과 별개로 **취소**(`SshCancel`)가 이 자식을 상한 만료 전에 끊을 수 있다. 두 경로는
/// 서로를 침범하지 않는다 — 취소가 관여했으면 어느 지점에서 끊겼든
/// [`PortDiscoveryFailureKind::Cancelled`] 로 나가고, 시간이 끊었을 때만 `TimedOut` 이다.
fn run_capture_with_budget(
    mut cmd: Command,
    budget: Duration,
    what: &str,
) -> Result<String, PortDiscoveryError> {
    // 예산 소진과 같은 자리의 조기 반환 — 이미 취소된 조회가 새 ssh 를 띄우지 않게 한다.
    // 취소를 예산보다 **먼저** 본다: 둘 다 해당되면 사용자가 끊었다는 확정 사실이
    // "시간이 다 됐다" 보다 정확하다.
    if cancel_requested() {
        return Err(cancelled_error(what));
    }
    if budget.is_zero() {
        return Err(PortDiscoveryError::new(
            PortDiscoveryFailureKind::TimedOut,
            format!("{what}: 포트 발견 전체 예산 소진 — ssh 를 띄우지 않음"),
        ));
    }
    let started = Instant::now();
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            PortDiscoveryError::new(
                PortDiscoveryFailureKind::SshConnectionFailed,
                format!("{what}: spawn 실패: {e}"),
            )
        })?;
    // 자식 핸들을 취소 스코프에 맡긴다 — 상한 감시와 외부 취소가 같은 핸들을 공유해야
    // 둘 중 먼저 오는 쪽이 자식을 끊을 수 있다.
    let mut owner = ChildOwner::adopt(child, what)?;
    if !wait_with_timeout(&mut owner, budget) {
        // 상한과 취소가 겹쳤다면 취소가 이긴다 — 사용자가 끊은 것을 무응답으로 보고하지
        // 않는다(반대로 취소가 없었으면 순수 타임아웃이다).
        let cancelled = owner.is_cancelled();
        owner.kill_and_reap_now();
        return Err(if cancelled {
            cancelled_error(what)
        } else {
            PortDiscoveryError::new(
                PortDiscoveryFailureKind::TimedOut,
                format!("{what}: {budget:?} 안에 끝나지 않아 강제 종료"),
            )
        });
    }
    let child = owner.finish(what)?;
    let output = child.wait_with_output().map_err(|e| {
        PortDiscoveryError::new(
            PortDiscoveryFailureKind::SshConnectionFailed,
            format!("{what}: 출력 수집 실패: {e}"),
        )
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code();
        let elapsed = started.elapsed();
        return Err(PortDiscoveryError::new(
            refine_with_elapsed(classify_by_exit_code(code), elapsed),
            format!("code={code:?} elapsed={elapsed:?}: {}", stderr.trim()),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 원격에서 한 줄 명령을 실행하고 stdout 을 캡처한다(포트 발견용). 실패 원인은
/// [`classify_by_exit_code`] 로 분류해 [`PortDiscoveryError`] 에 담는다.
///
/// `deadline` 은 호출 1회 전체([`PORT_DISCOVERY_TOTAL_TIMEOUT`])의 예산이다 — 이 단계는
/// 그중 [`Deadline::step_budget`] 만 쓴다. 상한 감시와 취소(`SshCancel`) 배선은 모두
/// [`run_capture_with_budget`] 이 담당한다.
fn run_ssh_capture(
    ssh: &Path,
    target: &SshTarget,
    verify: bool,
    remote_argv: &[&str],
    deadline: Deadline,
) -> Result<String, PortDiscoveryError> {
    let mut args: Vec<String> = Vec::new();
    push_common_opts(&mut args, target, verify);
    args.push(target.destination.clone());
    for a in remote_argv {
        args.push((*a).to_string());
    }
    let mut cmd = Command::new(ssh);
    cmd.args(&args);
    // 호스트 GUI(windows subsystem, 콘솔 없음)가 in-process 로 이 함수를 호출하므로
    // CREATE_NO_WINDOW 를 걸지 않으면 Windows 가 ssh.exe 용 새 콘솔 창을 띄운다.
    tasty_utils::process::hide_console(&mut cmd);
    // `what` 은 detail(로그 전용)에만 들어간다 — Display 계약상 사용자에게는 안 보인다.
    let what = format!("ssh({})", ssh.display());
    run_capture_with_budget(cmd, deadline.step_budget(), &what)
}

/// stdout 텍스트에서 포트 숫자를 파싱한다(trailing newline/CR 허용).
fn parse_port(stdout: &str) -> Result<u16, PortDiscoveryError> {
    stdout
        .trim()
        .lines()
        .next_back()
        .unwrap_or("")
        .trim()
        .parse::<u16>()
        .map_err(|_| {
            PortDiscoveryError::new(PortDiscoveryFailureKind::PortParseFailed, stdout.trim())
        })
}

/// debug/release 빌드에 맞는 원격 루트 디렉터리명.
///
/// 데이터 루트 자체가 debug/release 로 갈리므로(`~/.tasty-debug` vs `~/.tasty`),
/// 디스커버리도 디렉터리로 구분한다. 포트 파일명은 양쪽 모두 `tasty.port` 로 통일.
fn remote_tasty_dir(debug: bool) -> &'static str {
    if debug { ".tasty-debug" } else { ".tasty" }
}

/// auto 체인 첫 단계: `ssh <dest> <remote-tasty> port` → stdout 의 포트 숫자.
fn discover_via_subcommand(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    verify: bool,
    deadline: Deadline,
) -> Result<u16, PortDiscoveryError> {
    let out = run_ssh_capture(ssh, target, verify, &[remote_tasty, "port"], deadline)?;
    parse_port(&out)
}

/// 명시 port 파일을 읽는 원격 명령 후보(순서대로 시도). `cat`(unix/powershell/git
/// bash) → `type`(cmd). 순수 함수라 단위 테스트로 명령 형태를 고정한다.
fn explicit_file_commands(port_file: &str) -> [String; 2] {
    [format!("cat {port_file}"), format!("type {port_file}")]
}

/// 명시된 port 파일 경로를 직접 읽어 포트를 얻는다(관례 경로 무시, 최우선).
///
/// 원격 셸을 모를 수 있으므로 `cat <path>`(unix/powershell/git bash) → `type <path>`
/// (cmd) 순으로 시도한다. 명시 경로는 `~` 확장을 원격 셸에 의존하므로 절대경로 권장.
fn discover_via_explicit_file(
    ssh: &Path,
    target: &SshTarget,
    port_file: &str,
    verify: bool,
    deadline: Deadline,
) -> Result<u16, PortDiscoveryError> {
    let cmds = explicit_file_commands(port_file);
    match run_ssh_capture(ssh, target, verify, &[&cmds[0]], deadline).and_then(|o| parse_port(&o)) {
        Ok(port) => return Ok(port),
        Err(e) => tracing::debug!("port_file `{}` 실패({e}) — 다음 후보 시도", cmds[0]),
    }
    let out = run_ssh_capture(ssh, target, verify, &[&cmds[1]], deadline)?;
    parse_port(&out)
}

/// OS 분기 file 모드(decisions 9 fallback): Unix `cat` / Windows `type`.
fn discover_via_file(
    ssh: &Path,
    target: &SshTarget,
    windows: bool,
    verify: bool,
    debug: bool,
    deadline: Deadline,
) -> Result<u16, PortDiscoveryError> {
    let dir = remote_tasty_dir(debug);
    let remote_cmd = if windows {
        format!("type %USERPROFILE%\\{dir}\\tasty.port")
    } else {
        format!("cat ~/{dir}/tasty.port")
    };
    let out = run_ssh_capture(ssh, target, verify, &[&remote_cmd], deadline)?;
    parse_port(&out)
}

/// `Auto` 모드의 고정 fallback 시도 순서. SSH 는 연결 수단일 뿐 원격
/// DefaultShell(PowerShell/cmd/git bash/unix)이 무엇이든 동작해야 하므로, 단일 셸에
/// 의존하지 않도록 3개 단일 모드를 순서대로 시도해 4개 셸 매트릭스를 전부 커버한다:
/// - subcommand: git bash·unix 성공 (Windows GUI 바이너리 release 는 빈 출력으로 실패).
/// - file-unix (`cat ~/...`): PowerShell(`cat` alias + `~` 확장)·git bash·unix 성공.
/// - file-windows (`type %USERPROFILE%\...`): cmd 성공 — 위 둘이 모두 실패하는 유일 경로.
const AUTO_FALLBACK_CHAIN: [PortMode; 3] = [
    PortMode::Subcommand,
    PortMode::FileUnix,
    PortMode::FileWindows,
];

/// Auto 체인 실패 시 대표로 보여줄 에러 하나를 고른다. 세 단계 모두 실패하면 원인이
/// 섞일 수 있는데(예: subcommand 는 Windows release 에서 빈 출력으로 파싱 실패, 나머지
/// 둘은 포트 파일이 없어 인스턴스 미실행) 마지막 단계 에러만 남기면 정보량이 가장 적은
/// 사유가 사용자에게 보인다(원래 버그). 확실성이 높은 분류를 우선한다:
/// SSH 연결 자체 실패(다른 무엇도 알 수 없음) > 인스턴스 미실행(원격 명령까지는 도달해
/// 얻은 확정적 신호) > 포트 파싱 실패(가장 모호함 — 연결·명령 성공, 출력만 이례적).
/// 동일 kind 가 여럿이면 체인에서 먼저 시도된 것을 쓴다.
///
/// 타임아웃은 그 위에 놓는다 — 아무 답도 못 받은 단계가 하나라도 있으면, 다른 단계가
/// 낸 "연결 실패/미실행" 은 **그 타임아웃이 전체 예산을 먹어 굶긴 결과**일 수 있어
/// 대표로 삼으면 오도한다(체크할 곳이 인증이 아니라 도달성이다).
fn pick_most_informative(mut errors: Vec<PortDiscoveryError>) -> PortDiscoveryError {
    // 취소가 타임아웃보다도 위다 — 사용자가 끊었다는 것은 확정 사실이라, 그 때문에
    // 굶은 다른 단계의 무응답/연결 실패를 대표로 삼으면 오도한다.
    const PRIORITY: [PortDiscoveryFailureKind; 5] = [
        PortDiscoveryFailureKind::Cancelled,
        PortDiscoveryFailureKind::TimedOut,
        PortDiscoveryFailureKind::SshConnectionFailed,
        PortDiscoveryFailureKind::RemoteInstanceNotRunning,
        PortDiscoveryFailureKind::PortParseFailed,
    ];
    for kind in PRIORITY {
        if let Some(pos) = errors.iter().position(|e| e.kind == kind) {
            return errors.remove(pos);
        }
    }
    errors
        .pop()
        .expect("errors 는 AUTO_FALLBACK_CHAIN 시도 횟수만큼 채워져 비어있지 않음")
}

/// 원격 tasty 데몬의 IPC 포트를 발견한다(plan §4.4).
///
/// `Auto` 는 [`AUTO_FALLBACK_CHAIN`] 순서로 단일 모드를 차례로 시도하고, 한 모드라도
/// 포트를 내면 즉시 반환한다. subcommand 는 Windows release 에서 "빈 출력 + exit 0"
/// 으로 조용히 실패할 수 있는데, 이 경우 [`parse_port`] 가 에러를 내며 다음 단계로
/// 넘어간다(exit code 만으로는 감지 불가). 전 단계 실패 시 [`pick_most_informative`] 로
/// 대표 에러를 고른다.
///
/// **상한**: 이 호출 전체가 [`PORT_DISCOVERY_TOTAL_TIMEOUT`] 안에 끝난다(각 ssh 실행은
/// 추가로 [`PORT_DISCOVERY_STEP_TIMEOUT`] 상한). 무응답 호스트에서 무기한 블록하지
/// 않으므로 워커/재연결 루프가 이 함수에 갇히지 않는다.
pub fn discover_remote_port(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    mode: PortMode,
    verify: bool,
    debug: bool,
    port_file: Option<&str>,
) -> Result<u16> {
    let deadline = Deadline::after(PORT_DISCOVERY_TOTAL_TIMEOUT);
    // 명시 port_file 은 관례 체인보다 최우선 — 비표준 위치의 port 파일도 발견 가능.
    if let Some(pf) = port_file {
        return Ok(discover_via_explicit_file(
            ssh, target, pf, verify, deadline,
        )?);
    }
    if mode != PortMode::Auto {
        return Ok(discover_single_mode(
            ssh,
            target,
            remote_tasty,
            mode,
            verify,
            debug,
            deadline,
        )?);
    }

    let mut errors = Vec::with_capacity(AUTO_FALLBACK_CHAIN.len());
    for m in AUTO_FALLBACK_CHAIN {
        match discover_single_mode(ssh, target, remote_tasty, m, verify, debug, deadline) {
            Ok(port) => return Ok(port),
            // 취소는 fallback 대상이 아니다 — 다음 모드를 시도하면 방금 끊은 조회가
            // 새 ssh 를 띄운다.
            Err(e) if e.kind == PortDiscoveryFailureKind::Cancelled => return Err(e.into()),
            Err(e) => {
                tracing::debug!("{m:?} 포트 발견 실패({e}) — 다음 모드로 fallback");
                errors.push(e);
            }
        }
    }
    Err(pick_most_informative(errors).into())
}

/// 단일(비-Auto) 모드 1회 시도. `Auto` 는 호출 전 체인으로 분해되므로 unreachable.
fn discover_single_mode(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    mode: PortMode,
    verify: bool,
    debug: bool,
    deadline: Deadline,
) -> Result<u16, PortDiscoveryError> {
    match mode {
        PortMode::Subcommand => {
            discover_via_subcommand(ssh, target, remote_tasty, verify, deadline)
        }
        PortMode::FileUnix => discover_via_file(ssh, target, false, verify, debug, deadline),
        PortMode::FileWindows => discover_via_file(ssh, target, true, verify, debug, deadline),
        PortMode::Auto => unreachable!("Auto 는 AUTO_FALLBACK_CHAIN 으로 분해됨"),
    }
}

/// 자동감지: 프로브 체인([`AUTO_FALLBACK_CHAIN`])을 순서대로 시도해 **첫 성공 모드**를
/// 돌려준다(셸 종류를 묻는 단일 명령이 없으므로 프로브 성패가 곧 감지 결과 —
/// `docs/adr/0032-remote-attach-two-layer-split.md`).
/// 전 프로브 실패 시 마지막 에러를 반환한다.
///
/// `try_mode` 는 단일 모드를 시도해 포트를 내는 클로저 — 실제 SSH 실행([`detect_port_mode`])
/// 또는 테스트용 mock 을 주입할 수 있다.
fn detect_first_success<F>(mut try_mode: F) -> Result<PortMode>
where
    F: FnMut(PortMode) -> Result<u16, PortDiscoveryError>,
{
    let mut errors = Vec::with_capacity(AUTO_FALLBACK_CHAIN.len());
    for m in AUTO_FALLBACK_CHAIN {
        match try_mode(m) {
            Ok(_) => return Ok(m),
            Err(e) => {
                tracing::debug!("{m:?} 감지 프로브 실패({e}) — 다음 모드");
                errors.push(e);
            }
        }
    }
    if errors.is_empty() {
        return Err(anyhow::anyhow!("프로브 체인이 비어있음"));
    }
    Err(pick_most_informative(errors).into())
}

/// 원격 셸 자동감지 — 프로브 체인을 실제 SSH 로 1회씩 돌려 첫 성공 모드를 반환한다.
/// 네트워크 I/O(1~3 왕복)로 수 초 블록될 수 있다 — 호출자가 적절한 스레드에서 실행.
///
/// [`discover_remote_port`] 와 같은 상한이 걸린다(체인 전체 [`PORT_DISCOVERY_TOTAL_TIMEOUT`],
/// ssh 1회 [`PORT_DISCOVERY_STEP_TIMEOUT`]) — 감지 경로도 같은 `run_ssh_capture` 를 타므로
/// "감지 중" 표시가 무한정 남지 않는다.
pub(crate) fn detect_port_mode(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    verify: bool,
    debug: bool,
) -> Result<PortMode> {
    let deadline = Deadline::after(PORT_DISCOVERY_TOTAL_TIMEOUT);
    detect_first_success(|m| {
        discover_single_mode(ssh, target, remote_tasty, m, verify, debug, deadline)
    })
}

/// ssh 프로필의 `shell` 값으로 **셸 감지 상태(`detect_failed`)** 를 갱신한다(detect-split:
/// ssh 레이어는 셸 도달성만 판정, port_mode 도출은 attach 레이어 — `resolve_attach_target`).
///
/// - 명시 셸(powershell/cmd/bash/zsh) → 도달 가능한 셸로 간주, `detect_failed` 해제.
///   네트워크 I/O 없음. 반환 `None`.
/// - `auto` → 실제 SSH 프로브 체인 1회 실행(블록). 성공 시 `detect_failed` 해제, 실패 시
///   `detect_failed=true`. 반환 `Some(결과 모드)`(리포트용 — 프로필엔 저장하지 않는다).
///
/// `auto` 분기는 네트워크 I/O 로 수 초 블록될 수 있다 — GUI/host 는 워커 스레드에서 호출.
/// **공개 계약 아님** — `tasty-cli` 의 커맨드 구현만 쓴다. 크레이트 밖 소비자가
/// 하나뿐이라 `pub(crate)` 로 못 내릴 뿐이며, 새 소비자가 이걸 쓰기 시작하면
/// 그건 계약 확장이므로 `#[doc(hidden)]` 을 떼고 문서화하는 결정을 먼저 한다.
#[doc(hidden)]
pub fn apply_shell_to_profile(
    profile: &mut RemoteProfile,
    passkeys: &Passkeys,
) -> Option<Result<PortMode>> {
    let shell = profile
        .as_ssh()
        .map(|v| v.shell().to_string())
        .unwrap_or_else(|| "auto".into());
    if shell_to_port_mode(&shell).is_some() {
        // 명시 셸 = 도달 가능으로 간주(프로브 생략). port_mode 는 attach 레이어가 도출.
        profile.remove_field("detect_failed");
        return None;
    }
    // shell == auto (또는 알 수 없는 값) → 프로브로 도달성 검증.
    let outcome = detect_for_profile(profile, passkeys);
    match &outcome {
        Ok(_) => {
            profile.remove_field("detect_failed");
        }
        Err(_) => profile.set_field("detect_failed", "true"),
    }
    Some(outcome)
}

/// 프로필 접속 정보로 자동감지를 1회 실행한다(셸 무관 — 항상 프로브 체인).
fn detect_for_profile(profile: &RemoteProfile, passkeys: &Passkeys) -> Result<PortMode> {
    let ssh = resolve_ssh_path();
    let target = SshTarget::from_remote_profile(profile, passkeys)?;
    // ssh 프로필 셸 감지는 기본 `tasty` 바이너리로 subcommand 프로브를 시도한다.
    // (attach 실행부의 remote_tasty 는 tasty-attach 프로필이 소유 — 셸 감지엔 불필요.)
    let remote_tasty = profile
        .fields
        .get("remote_tasty")
        .and_then(|f| f.as_str())
        .unwrap_or("tasty")
        .to_string();
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);
    detect_port_mode(&ssh, &target, &remote_tasty, verify, debug)
}

/// 이름으로 ssh 프로필을 로드→재감지→저장한다(GUI 새로고침/IPC detect 워커 진입점).
///
/// 셸 무관하게 프로브 체인을 돌려 셸 **도달성**을 확인하고, 성공 시 `detect_failed` 해제,
/// 실패 시 `detect_failed=true`(비활성)로 toml 을 갱신한다(detect-split: port_mode 는
/// 저장하지 않는다 — attach 레이어가 도출). 반환 모드는 리포트용. 네트워크 I/O 블록.
pub fn detect_and_persist(name: &str) -> Result<PortMode> {
    let mut profiles = RemoteProfiles::load();
    let passkeys = Passkeys::load();
    let Some(mut p) = profiles.get(name).cloned() else {
        bail!(
            "{}",
            tasty_i18n::t_fmt("cli.remote_profile.not_found", name)
        );
    };
    let result = detect_for_profile(&p, &passkeys);
    match &result {
        Ok(_) => {
            p.remove_field("detect_failed");
        }
        Err(_) => p.set_field("detect_failed", "true"),
    }
    profiles.upsert(p);
    profiles.save()?;
    result
}

/// `127.0.0.1:0` 바인드로 비어있는 로컬 포트를 확보하고, 그 포트를 점유하는 리스너를
/// 함께 반환한다. 리스너를 잡고 있는 동안 그 포트는 이 프로세스 소유라 TOCTOU 가 없다 —
/// 소비자가 리스너를 drop 하는 순간부터 레이스가 시작된다(ssh 가 rebind, ready-probe
/// 타임아웃 → 재시도로 흡수). 예전엔 함수 안에서 drop 하고 port 만 돌려줬고, 그러면
/// 반환값을 다시 bind 하려는 코드는 전부 그 짧은 창을 노출한 TOCTOU 였다.
fn reserve_local_port() -> Result<(std::net::TcpListener, u16)> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// `ssh -L` 포트포워딩 터널의 자식 프로세스 핸들(plan §5).
///
/// Drop 시 자식 ssh 를 kill 해 고아 터널을 방지한다. 자식 ssh 를 kill 해도 원격
/// tasty 데몬은 생존한다(server-owns-PTY persistence) = detach 의 본질.
pub struct SshTunnel {
    child: Child,
    /// 로컬 끝점 포트 — client 가 `127.0.0.1:local_port` 로 붙는다.
    pub local_port: u16,
}

impl SshTunnel {
    /// `ssh -L 127.0.0.1:local:127.0.0.1:remote -N <dest>` 백그라운드 spawn 후
    /// 로컬 끝점이 LISTEN 상태가 될 때까지 폴링한다(ready-probe).
    pub fn establish(
        ssh: &Path,
        target: &SshTarget,
        remote_port: u16,
        verify: bool,
    ) -> Result<Self> {
        // 포트를 리스너로 점유한 뒤 즉시 놓는다 — 이 drop 부터 ssh 의 rebind 까지가
        // 본질적 TOCTOU 이고, 아래 ready-probe 폴링이 그 창의 충돌을 흡수한다.
        let (reservation, local_port) = reserve_local_port()?;
        drop(reservation);
        // 로컬 끝점도 loopback 한정(`-L *:` / `-g` 금지) — 멀티유저 노출 차단.
        let forward = format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}");

        let mut args: Vec<String> = Vec::new();
        push_common_opts(&mut args, target, verify);
        // 포워드 실패 시 즉시 종료 → ready-probe 가 자식 사망으로 감지.
        args.push("-o".into());
        args.push("ExitOnForwardFailure=yes".into());
        args.push("-N".into()); // 원격 명령 없이 터널만
        args.push("-L".into());
        args.push(forward);
        args.push(target.destination.clone());

        let mut cmd = Command::new(ssh);
        cmd.args(&args).stdin(Stdio::null());
        // stderr 는 인증/에러 노출용으로 상속(첫 연결 host key/passphrase).
        // 호스트 GUI(windows subsystem, 콘솔 없음)가 in-process 로 이 함수를 호출하므로
        // CREATE_NO_WINDOW 를 걸지 않으면 Windows 가 ssh.exe 용 새 콘솔 창을 띄운다.
        tasty_utils::process::hide_console(&mut cmd);
        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("ssh 터널 spawn 실패({}): {e}", ssh.display()))?;

        let mut tunnel = SshTunnel { child, local_port };
        tunnel.wait_ready()?;
        Ok(tunnel)
    }

    /// 로컬 끝점에 `TcpStream::connect` 가 성공할 때까지 폴링(타임아웃 ~5s).
    /// `ExitOnForwardFailure=yes` 라 포워드 실패 시 ssh 가 죽어 try_wait 로도 감지.
    fn wait_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait()? {
                bail!(
                    "ssh 터널이 ready 전에 종료(status={status:?}) — 인증/포워드 실패 가능. \
                     (Windows 는 시스템 OpenSSH 풀경로/agent 확인)"
                );
            }
            if TcpStream::connect(("127.0.0.1", self.local_port)).is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("ssh 터널 ready 타임아웃(127.0.0.1:{})", self.local_port);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// 터널 자식 ssh 가 살아있는지(끊김 감지 — 프로세스 레벨).
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// `SshTunnel::drop` 누적 소요(ns) — [`tunnel_drop_totals`] 참조.
static TUNNEL_DROP_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// `SshTunnel::drop` 누적 횟수 — [`tunnel_drop_totals`] 참조.
static TUNNEL_DROP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 지금까지 `SshTunnel::drop` 이 소비한 (총 시간, 횟수).
///
/// 호스트 종료 계측(`S5c ssh_tunnel_drop`)용이다. `child.wait()` 가 블로킹이라
/// attach 세션 수만큼 직렬로 쌓이고, 그 구간은 `event_loop.exit()` 이후라 화면으로
/// 덮을 수 없다. 호스트가 App drop **전후 델타**로 읽는다 (평시 browse/attach 해제
/// 시의 drop 도 함께 누적되므로 절대값은 의미가 없다).
pub fn tunnel_drop_totals() -> (Duration, u64) {
    use std::sync::atomic::Ordering;
    (
        Duration::from_nanos(TUNNEL_DROP_NANOS.load(Ordering::Relaxed)),
        TUNNEL_DROP_COUNT.load(Ordering::Relaxed),
    )
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;

        let t_drop = Instant::now();
        let _ = self.child.kill(); // best-effort 자식 종료 — 이미 종료됐을 수 있음, 무시
        let _ = self.child.wait(); // 좀비 방지 reaping — 실패 무시
        TUNNEL_DROP_NANOS.fetch_add(
            u64::try_from(t_drop.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        TUNNEL_DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

/// 지수 백오프(자동 재연결 — decisions 7). min 에서 시작해 factor 배씩 증가, max 상한.
/// 성공 시 [`reset`](Self::reset) 으로 min 복귀.
pub struct Backoff {
    cur: Duration,
    min: Duration,
    max: Duration,
}

impl Backoff {
    /// 권장 파라미터: min=500ms, max=30s, factor=2.
    pub fn new() -> Self {
        Self {
            cur: Duration::from_millis(500),
            min: Duration::from_millis(500),
            max: Duration::from_secs(30),
        }
    }

    /// 현재 백오프만큼 sleep 한 뒤 다음 간격을 2 배(상한 max)로 늘린다.
    pub fn sleep(&mut self) {
        std::thread::sleep(self.cur);
        self.advance();
    }

    /// 현재 대기 간격을 조회한다(sleep 하지 않음) — GUI 메인 루프처럼 스레드를
    /// 블록할 수 없는 논블로킹 스케줄러가 "다음 재시도 시각"을 계산하는 데 쓴다.
    pub fn current(&self) -> Duration {
        self.cur
    }

    /// sleep 없이 다음 간격으로 넘어간다(2 배, 상한 max) — 논블로킹 스케줄러 전용.
    /// `sleep()`은 이 메서드 위에 blocking sleep 을 얹은 것과 동일하다.
    pub fn advance(&mut self) {
        self.cur = (self.cur * 2).min(self.max);
    }

    /// 연결 성공 시 백오프를 min 으로 되돌린다.
    pub fn reset(&mut self) {
        self.cur = self.min;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_target_parse_keeps_destination() {
        assert_eq!(SshTarget::parse("user@host").destination, "user@host");
        assert_eq!(SshTarget::parse("gx10").destination, "gx10");
        assert!(SshTarget::parse("user@host").ssh_port.is_none());
        // 1회성 경로는 identity/extra 가 비어 동작 불변.
        assert!(SshTarget::parse("gx10").identity_file.is_none());
        assert!(SshTarget::parse("gx10").extra_options.is_empty());
    }

    #[test]
    fn from_remote_profile_threads_user_port_identity_options() {
        // identity_file 은 이제 passkey_ref → Passkey.path 로 resolve.
        let mut pk = Passkeys::default();
        pk.upsert_path("gx10-key", "~/.ssh/id_ed25519").unwrap();
        let mut p = RemoteProfile::new("gx10", "ssh")
            .with_field("host", "gx10")
            .with_field("user", "zilhak")
            .with_field("port", "2222")
            .with_field("extra_options", vec!["ServerAliveInterval=30".to_string()]);
        p.passkey_ref = Some("gx10-key".into());
        let t = SshTarget::from_remote_profile(&p, &pk).unwrap();
        assert_eq!(t.destination, "zilhak@gx10");
        assert_eq!(t.ssh_port, Some(2222));
        assert_eq!(t.identity_file.as_deref(), Some("~/.ssh/id_ed25519"));
        assert_eq!(t.extra_options, vec!["ServerAliveInterval=30".to_string()]);
    }

    #[test]
    fn from_remote_profile_dangling_passkey_errors() {
        let pk = Passkeys::default();
        let mut p = RemoteProfile::new("x", "ssh").with_field("host", "h");
        p.passkey_ref = Some("missing".into());
        assert!(SshTarget::from_remote_profile(&p, &pk).is_err());
    }

    #[test]
    fn resolve_attach_rejects_ssh_kind_profile() {
        let pk = Passkeys::default();
        let profiles = RemoteProfiles::default();
        // ssh kind 를 attach 대상으로 주면 거부(tasty-attach 만 지원).
        let ssh = RemoteProfile::new("gb10", "ssh").with_field("host", "h");
        assert!(resolve_attach_target(&ssh, &profiles, &pk).is_err());
    }

    #[test]
    fn resolve_attach_inline_uses_own_fields() {
        let pk = Passkeys::default();
        let profiles = RemoteProfiles::default();
        let a = RemoteProfile::new("box-a", "tasty-attach")
            .with_field("host", "h")
            .with_field("port", "2222")
            .with_field("remote_tasty", "/usr/bin/tasty")
            .with_field("port_mode", "file-unix");
        let (t, rt, pm, pf) = resolve_attach_target(&a, &profiles, &pk).unwrap();
        assert_eq!(t.destination, "h");
        assert_eq!(t.ssh_port, Some(2222));
        assert_eq!(rt, "/usr/bin/tasty");
        assert_eq!(pm, "file-unix");
        assert_eq!(pf, None);
    }

    #[test]
    fn resolve_attach_inline_disabled_is_rejected() {
        let pk = Passkeys::default();
        let profiles = RemoteProfiles::default();
        let a = RemoteProfile::new("box-a", "tasty-attach")
            .with_field("host", "h")
            .with_field("detect_failed", "true");
        assert!(resolve_attach_target(&a, &profiles, &pk).is_err());
    }

    #[test]
    fn resolve_attach_ref_follows_referenced_ssh() {
        let pk = Passkeys::default();
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(
            RemoteProfile::new("gb10", "ssh")
                .with_field("host", "gbhost")
                .with_field("port", "10209"),
        );
        let attach = RemoteProfile::new("gb10-a", "tasty-attach")
            .with_field("ssh_ref", "gb10")
            .with_field("port_file", "/data/tasty.port");
        profiles.upsert(attach.clone());

        let (t, _rt, _pm, pf) = resolve_attach_target(&attach, &profiles, &pk).unwrap();
        assert_eq!(t.destination, "gbhost");
        assert_eq!(t.ssh_port, Some(10209));
        assert_eq!(pf.as_deref(), Some("/data/tasty.port"));

        // 라이브 팔로우: 참조 ssh 프로필 port 를 바꾸면 resolve 도 따라간다.
        profiles.upsert(
            RemoteProfile::new("gb10", "ssh")
                .with_field("host", "gbhost")
                .with_field("port", "10210"),
        );
        let (t2, _, _, _) = resolve_attach_target(&attach, &profiles, &pk).unwrap();
        assert_eq!(t2.ssh_port, Some(10210));
    }

    #[test]
    fn resolve_attach_ref_dangling_errors() {
        let pk = Passkeys::default();
        let profiles = RemoteProfiles::default(); // 참조 대상 없음
        let attach = RemoteProfile::new("gb10-a", "tasty-attach").with_field("ssh_ref", "missing");
        assert!(resolve_attach_target(&attach, &profiles, &pk).is_err());
    }

    #[test]
    fn resolve_attach_ref_disabled_ssh_is_rejected() {
        let pk = Passkeys::default();
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(
            RemoteProfile::new("gb10", "ssh")
                .with_field("host", "h")
                .with_field("detect_failed", "true"),
        );
        let attach = RemoteProfile::new("gb10-a", "tasty-attach").with_field("ssh_ref", "gb10");
        assert!(resolve_attach_target(&attach, &profiles, &pk).is_err());
    }

    #[test]
    fn push_common_opts_emits_identity_and_extra_options() {
        let mut t = SshTarget::parse("gx10");
        t.ssh_port = Some(2200);
        t.identity_file = Some("/home/me/.ssh/key".into());
        t.extra_options = vec!["ProxyJump=bastion".into()];
        let mut args: Vec<String> = Vec::new();
        push_common_opts(&mut args, &t, false);
        // -p <port>
        let p = args.iter().position(|a| a == "-p").expect("-p present");
        assert_eq!(args[p + 1], "2200");
        // -i <identity> (절대 경로는 tilde 확장 무영향)
        let i = args.iter().position(|a| a == "-i").expect("-i present");
        assert_eq!(args[i + 1], "/home/me/.ssh/key");
        // -o ProxyJump=bastion
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-o" && w[1] == "ProxyJump=bastion")
        );
    }

    #[test]
    fn push_common_opts_appends_default_connect_timeout() {
        // 연결 수립 상한이 없으면 OS SYN 재시도(리눅스 ~127초)에 맡겨진다 — 기본값 회귀 고정.
        let mut args: Vec<String> = Vec::new();
        push_common_opts(&mut args, &SshTarget::parse("gx10"), false);
        let expected = format!("ConnectTimeout={}", SSH_CONNECT_TIMEOUT.as_secs());
        assert!(args.windows(2).any(|w| w[0] == "-o" && w[1] == expected));
    }

    #[test]
    fn user_connect_timeout_wins_over_default() {
        // ssh(1) 은 같은 키의 **먼저 나온 값**을 쓴다 — 사용자 extra_options 가 기본값보다
        // 앞에 와야 프로필 지정이 유효하다(느린 회선/다단 ProxyJump 대응).
        let mut t = SshTarget::parse("gx10");
        t.extra_options = vec!["ConnectTimeout=60".into()];
        let mut args: Vec<String> = Vec::new();
        push_common_opts(&mut args, &t, false);
        let user = args
            .iter()
            .position(|a| a == "ConnectTimeout=60")
            .expect("user ConnectTimeout present");
        let default = args
            .iter()
            .position(|a| a == &format!("ConnectTimeout={}", SSH_CONNECT_TIMEOUT.as_secs()))
            .expect("default ConnectTimeout present");
        assert!(
            user < default,
            "사용자 지정이 기본값보다 앞에 와야 이긴다: {args:?}"
        );
    }

    /// 시작하면 상한 안에는 절대 끝나지 않는 자식 프로세스(타임아웃 경로 검증용).
    /// 띄우면 **반드시 실패하는** 명령. "ssh 를 안 띄웠다" 를 시계가 아니라 **사건**으로
    /// 만드는 데 쓴다 — 띄웠으면 `spawn` 이 실패해 `SshConnectionFailed` 가 나오고,
    /// 안 띄웠으면 조기 반환의 사유(`TimedOut`/`Cancelled`)가 그대로 나온다. 두 값이
    /// 다르므로 분류 하나로 갈린다. 경과 상한은 같은 것을 훨씬 약하게 물으면서
    /// 굶은 러너에서 빨개지기까지 했다(ADR-0181).
    fn unspawnable_command() -> Command {
        Command::new("/tasty-ssh-test/this-path-must-not-exist")
    }

    fn never_returns_command() -> Command {
        #[cfg(windows)]
        {
            let mut c = Command::new("cmd");
            c.args(["/c", "ping -n 60 127.0.0.1 > nul"]);
            c
        }
        #[cfg(not(windows))]
        {
            let mut c = Command::new("sleep");
            c.arg("60");
            c
        }
    }

    /// 응답하지 않는 대상은 상한 안에 에러로 끝난다(무한 대기 회귀 고정).
    /// `remote_browse::probe_stale_port_eof_is_error_not_hang` 과 같은 성격의 no-hang 테스트.
    #[test]
    fn port_discovery_times_out_instead_of_hanging() {
        const CEILING: Duration = Duration::from_secs(10);
        // 대조군을 뺐다. 이 자리가 기다리는 자원은 fork/exec 이라 계열은 맞았지만,
        // 그 계열이 ADR-0181 의 규칙 3(값이 싸고 변동이 작다)을 못 지킨다 — 부하도
        // 유휴도 없이 기준선 대비 0.8~3.6 배, 유휴를 끼면 4.8 배까지 흔들리는 것이
        // 실측됐다. 그만큼 흔들리는 값에 판정을 붙이면 "러너가 굶었다" 를 근거 없이
        // 말하게 되고, 그것이 그 ADR 이 지금 상태보다 나쁘다고 못 박은 거짓 음성이다.
        let started = Instant::now();
        let r = run_capture_with_budget(
            never_returns_command(),
            Duration::from_millis(300),
            "no-hang test",
        );
        let err = r.expect_err("상한을 넘긴 자식은 에러여야 한다");
        assert_eq!(err.kind, PortDiscoveryFailureKind::TimedOut);
        // 상한(300ms) + 폴링/프로세스 spawn 여유. 무한 대기면 여기서 잡힌다.
        let elapsed = started.elapsed();
        assert!(elapsed < CEILING, "상한 안에 반환되지 않음: {elapsed:?}");
        // 타임아웃 detail 은 로그 전용 — 사용자 문구에 내부 사정이 새지 않는다.
        assert!(!err.to_string().contains("no-hang test"));
    }

    #[test]
    fn exhausted_budget_skips_spawning_ssh() {
        // 전체 예산이 소진되면 남은 체인 단계는 ssh 를 띄우지도 않고 즉시 타임아웃.
        // 띄울 수 없는 명령을 준다 — 띄웠다면 spawn 이 실패해 `SshConnectionFailed` 가
        // 나온다. `TimedOut` 이 나온다는 것이 곧 안 띄웠다는 사건이다.
        let err = run_capture_with_budget(unspawnable_command(), Duration::ZERO, "exhausted")
            .expect_err("예산 0 은 에러");
        assert_eq!(
            err.kind,
            PortDiscoveryFailureKind::TimedOut,
            "spawn 을 시도했으면 SshConnectionFailed 가 나왔을 것이다: {err}"
        );
    }

    #[test]
    fn capture_returns_stdout_within_budget() {
        // 정상 경로 비회귀: 상한 안에 끝나는 자식의 stdout 은 그대로 캡처된다.
        #[cfg(windows)]
        let cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "echo 45123"]);
            c
        };
        #[cfg(not(windows))]
        let cmd = {
            let mut c = Command::new("echo");
            c.arg("45123");
            c
        };
        let out = run_capture_with_budget(cmd, Duration::from_secs(10), "echo").unwrap();
        assert_eq!(parse_port(&out).unwrap(), 45123);
    }

    #[test]
    fn deadline_step_budget_is_capped_by_remaining_total() {
        // 남은 예산이 단계 상한보다 크면 단계 상한이 적용된다.
        let fresh = Deadline::after(PORT_DISCOVERY_TOTAL_TIMEOUT);
        assert!(fresh.step_budget() <= PORT_DISCOVERY_STEP_TIMEOUT);
        // 남은 예산이 더 작으면 그쪽이 적용된다(체인 총 대기가 단계 수만큼 곱해지지 않는다).
        let almost_gone = Deadline::after(Duration::from_millis(50));
        assert!(almost_gone.step_budget() <= Duration::from_millis(50));
        // 이미 만료된 데드라인은 0 예산 = ssh 미실행.
        let expired = Deadline {
            at: Instant::now() - Duration::from_secs(1),
        };
        assert!(expired.step_budget().is_zero());
    }

    #[test]
    fn total_timeout_bounds_the_auto_chain() {
        // auto 체인은 3 단계라 단계 상한만으론 총 대기가 3 배가 된다 — 전체 상한이
        // 그보다 작아야 곱이 끊긴다(값 선정 회귀 고정).
        assert!(
            PORT_DISCOVERY_TOTAL_TIMEOUT
                < PORT_DISCOVERY_STEP_TIMEOUT * AUTO_FALLBACK_CHAIN.len() as u32
        );
        // 단계 상한은 연결 상한보다 커야 ConnectTimeout 실패가 kill 대신 제 분류로 뜬다.
        assert!(PORT_DISCOVERY_STEP_TIMEOUT > SSH_CONNECT_TIMEOUT);
    }

    #[test]
    fn timed_out_display_is_translated_not_raw() {
        // 타임아웃 경로도 raw detail 비노출 계약(`PortDiscoveryError` doc)을 지킨다.
        let err = PortDiscoveryError::new(
            PortDiscoveryFailureKind::TimedOut,
            "ssh(/usr/bin/ssh): 20s 안에 끝나지 않아 강제 종료",
        );
        assert!(!err.to_string().contains("/usr/bin/ssh"));
        assert!(!err.to_string().contains("강제 종료"));
        assert!(err.detail().contains("/usr/bin/ssh"));
    }

    #[test]
    fn ssh_self_timeout_is_refined_to_timed_out() {
        // ConnectTimeout 을 다 쓰고 exit 255 로 끝난 건 인증 실패가 아니라 무응답이다 —
        // 그대로 두면 사용자가 키/호스트키를 뒤지게 되므로 타임아웃으로 좁힌다.
        assert_eq!(
            refine_with_elapsed(
                PortDiscoveryFailureKind::SshConnectionFailed,
                SSH_CONNECT_TIMEOUT + Duration::from_millis(200),
            ),
            PortDiscoveryFailureKind::TimedOut
        );
        // 즉답 실패(DNS 실패·연결 거부·인증 거부)는 그대로 연결 실패.
        assert_eq!(
            refine_with_elapsed(
                PortDiscoveryFailureKind::SshConnectionFailed,
                Duration::from_millis(80),
            ),
            PortDiscoveryFailureKind::SshConnectionFailed
        );
        // 다른 분류는 시간과 무관하게 불변(원격 명령까지 도달한 확정 신호를 덮지 않는다).
        for kind in [
            PortDiscoveryFailureKind::RemoteInstanceNotRunning,
            PortDiscoveryFailureKind::PortParseFailed,
        ] {
            assert_eq!(
                refine_with_elapsed(kind, SSH_CONNECT_TIMEOUT * 3),
                kind,
                "{kind:?} 는 경과 시간으로 재분류하지 않는다"
            );
        }
    }

    #[test]
    fn pick_most_informative_prefers_timed_out() {
        // 한 단계가 무응답이면 나머지 단계의 "연결 실패" 는 예산 굶김의 결과일 수 있어
        // 대표로 삼으면 오도한다 — 타임아웃이 최우선.
        let errors = vec![
            PortDiscoveryError::new(PortDiscoveryFailureKind::SshConnectionFailed, "b"),
            PortDiscoveryError::new(PortDiscoveryFailureKind::TimedOut, "a"),
            PortDiscoveryError::new(PortDiscoveryFailureKind::RemoteInstanceNotRunning, "c"),
        ];
        assert_eq!(
            pick_most_informative(errors).kind,
            PortDiscoveryFailureKind::TimedOut
        );
    }

    #[test]
    fn expand_tilde_only_touches_leading_tilde() {
        // 절대 경로는 불변.
        assert_eq!(expand_tilde("/etc/x"), "/etc/x");
        // 중간 ~ 는 불변.
        assert_eq!(expand_tilde("/a/~/b"), "/a/~/b");
    }

    #[test]
    fn port_mode_parse() {
        assert_eq!(PortMode::parse("auto").unwrap(), PortMode::Auto);
        assert_eq!(PortMode::parse("subcommand").unwrap(), PortMode::Subcommand);
        assert_eq!(PortMode::parse("file-unix").unwrap(), PortMode::FileUnix);
        assert_eq!(
            PortMode::parse("file-windows").unwrap(),
            PortMode::FileWindows
        );
        assert!(PortMode::parse("nope").is_err());
    }

    #[test]
    fn parse_port_trims_and_takes_last_line() {
        assert_eq!(parse_port("45123\n").unwrap(), 45123);
        assert_eq!(parse_port("  45123  ").unwrap(), 45123);
        // type 의 CR 포함.
        assert_eq!(parse_port("45123\r\n").unwrap(), 45123);
        // 앞에 잡음(쉘 배너 등)이 있어도 마지막 줄을 취함.
        assert_eq!(parse_port("Last login: ...\n45123\n").unwrap(), 45123);
        assert!(parse_port("not-a-port").is_err());
        assert!(parse_port("").is_err());
    }

    #[test]
    fn parse_port_failure_is_classified_as_parse_failed() {
        // exit 0 + 빈/비숫자 stdout → 포트 파싱 실패로 분류(완료 확인 방법 절 시나리오).
        assert_eq!(
            parse_port("").unwrap_err().kind,
            PortDiscoveryFailureKind::PortParseFailed
        );
        assert_eq!(
            parse_port("not-a-port").unwrap_err().kind,
            PortDiscoveryFailureKind::PortParseFailed
        );
    }

    #[test]
    fn classify_exit_255_or_signal_is_ssh_connection_failed() {
        // ssh(1) 관례: 255 = 연결/인증 자체 실패. 시그널 종료(None) 도 원격 명령까지
        // 도달 못 했다고 보아 같은 분류(완료 확인 방법 절 시나리오).
        assert_eq!(
            classify_by_exit_code(Some(255)),
            PortDiscoveryFailureKind::SshConnectionFailed
        );
        assert_eq!(
            classify_by_exit_code(None),
            PortDiscoveryFailureKind::SshConnectionFailed
        );
    }

    #[test]
    fn classify_other_exit_codes_are_remote_instance_not_running() {
        // exit 1 (원격 명령은 실행됐으나 실패) → 인스턴스 미실행으로 분류. 로케일과
        // 무관하게 exit code 만으로 판정된다(완료 확인 방법 절 "임의 stderr" 시나리오).
        assert_eq!(
            classify_by_exit_code(Some(1)),
            PortDiscoveryFailureKind::RemoteInstanceNotRunning
        );
        assert_eq!(
            classify_by_exit_code(Some(127)),
            PortDiscoveryFailureKind::RemoteInstanceNotRunning
        );
    }

    #[test]
    fn port_discovery_error_display_never_contains_raw_detail() {
        // raw stderr(내부 명령·경로 포함)가 Display 로 새어나가지 않는지 회귀 고정.
        let raw = "cat: /home/zilhak/.tasty/tasty.port: 그런 파일이나 디렉터리가 없습니다";
        let err = PortDiscoveryError::new(PortDiscoveryFailureKind::RemoteInstanceNotRunning, raw);
        assert_eq!(err.detail(), raw);
        assert!(!err.to_string().contains("tasty.port"));
        assert!(!err.to_string().contains("cat:"));
    }

    #[test]
    fn pick_most_informative_prefers_ssh_connection_failed() {
        let errors = vec![
            PortDiscoveryError::new(PortDiscoveryFailureKind::PortParseFailed, "a"),
            PortDiscoveryError::new(PortDiscoveryFailureKind::SshConnectionFailed, "b"),
            PortDiscoveryError::new(PortDiscoveryFailureKind::RemoteInstanceNotRunning, "c"),
        ];
        assert_eq!(
            pick_most_informative(errors).kind,
            PortDiscoveryFailureKind::SshConnectionFailed
        );
    }

    #[test]
    fn pick_most_informative_prefers_instance_not_running_over_parse_failed() {
        // 실측 시나리오: subcommand 는 빈 출력(parse 실패), file-unix/file-windows 는
        // 포트 파일 없음(인스턴스 미실행) — 더 확정적인 "미실행" 을 대표로 고른다.
        let errors = vec![
            PortDiscoveryError::new(PortDiscoveryFailureKind::PortParseFailed, "empty output"),
            PortDiscoveryError::new(
                PortDiscoveryFailureKind::RemoteInstanceNotRunning,
                "no file",
            ),
            PortDiscoveryError::new(
                PortDiscoveryFailureKind::RemoteInstanceNotRunning,
                "no file",
            ),
        ];
        assert_eq!(
            pick_most_informative(errors).kind,
            PortDiscoveryFailureKind::RemoteInstanceNotRunning
        );
    }

    #[test]
    fn auto_fallback_chain_order_covers_all_shells() {
        // 회귀 고정: Auto 는 subcommand → file-unix → file-windows 순서여야 한다.
        // 이 순서가 깨지면 cmd DefaultShell 원격(file-windows 만 성공)에서 attach 불가.
        assert_eq!(
            AUTO_FALLBACK_CHAIN,
            [
                PortMode::Subcommand,
                PortMode::FileUnix,
                PortMode::FileWindows,
            ]
        );
        // file-windows 가 마지막 단계로 반드시 포함되어야 cmd 원격을 커버한다.
        assert!(AUTO_FALLBACK_CHAIN.contains(&PortMode::FileWindows));
        // Auto 자기 자신은 체인에 들어가면 안 된다(무한 재귀 방지).
        assert!(!AUTO_FALLBACK_CHAIN.contains(&PortMode::Auto));
    }

    #[test]
    fn port_mode_as_str_roundtrips_parse() {
        for m in [
            PortMode::Auto,
            PortMode::Subcommand,
            PortMode::FileUnix,
            PortMode::FileWindows,
        ] {
            assert_eq!(PortMode::parse(m.as_str()).unwrap(), m);
        }
    }

    #[test]
    fn detect_first_success_records_first_passing_mode() {
        // subcommand·file-unix 실패, file-windows 성공 → cmd 원격 감지 결과.
        let order = std::cell::RefCell::new(Vec::new());
        let mode = detect_first_success(|m| {
            order.borrow_mut().push(m);
            if m == PortMode::FileWindows {
                Ok(45123)
            } else {
                Err(PortDiscoveryError::new(
                    PortDiscoveryFailureKind::RemoteInstanceNotRunning,
                    format!("probe {m:?} failed"),
                ))
            }
        })
        .unwrap();
        assert_eq!(mode, PortMode::FileWindows);
        // 시도 순서가 체인과 동일해야 한다(subcommand → file-unix → file-windows).
        assert_eq!(
            *order.borrow(),
            vec![
                PortMode::Subcommand,
                PortMode::FileUnix,
                PortMode::FileWindows
            ]
        );
    }

    #[test]
    fn detect_first_success_takes_earliest_mode() {
        // 첫 모드가 성공하면 즉시 멈춘다.
        let mut calls = 0;
        let mode = detect_first_success(|_| {
            calls += 1;
            Ok(1234)
        })
        .unwrap();
        assert_eq!(mode, PortMode::Subcommand);
        assert_eq!(calls, 1);
    }

    #[test]
    fn detect_first_success_all_fail_is_err() {
        let r = detect_first_success(|m| {
            Err(PortDiscoveryError::new(
                PortDiscoveryFailureKind::RemoteInstanceNotRunning,
                format!("{m:?} down"),
            ))
        });
        assert!(r.is_err());
    }

    #[test]
    fn apply_shell_explicit_clears_detect_failed_without_io_or_port_mode() {
        // detect-split: 명시 셸 → 도달 가능으로 간주(감지 미실행, None 반환), detect_failed
        // 해제. port_mode 는 ssh 프로필에 저장하지 않는다(attach 레이어가 도출).
        let pk = Passkeys::default();
        let mut p = RemoteProfile::new("x", "ssh")
            .with_field("host", "h")
            .with_field("shell", "cmd")
            .with_field("detect_failed", "true");
        let ran = apply_shell_to_profile(&mut p, &pk);
        assert!(ran.is_none()); // 감지 미실행.
        assert!(!p.fields.contains_key("port_mode")); // ssh 는 port_mode 를 갖지 않는다.
        assert!(!p.as_ssh().unwrap().is_disabled()); // detect_failed 해제됨.
    }

    #[test]
    fn resolve_attach_port_mode_derived_from_ref_shell() {
        // attach port_mode=auto(기본) + 참조 ssh 셸=cmd → file-windows 로 도출.
        let pk = Passkeys::default();
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(
            RemoteProfile::new("box", "ssh")
                .with_field("host", "h")
                .with_field("shell", "cmd"),
        );
        let attach = RemoteProfile::new("box-a", "tasty-attach").with_field("ssh_ref", "box");
        let (_t, _rt, pm, _pf) = resolve_attach_target(&attach, &profiles, &pk).unwrap();
        assert_eq!(pm, "file-windows");
    }

    #[test]
    fn resolve_attach_explicit_port_mode_wins_over_shell() {
        // attach 가 port_mode 를 명시하면 셸 도출을 무시하고 그 값을 쓴다.
        let pk = Passkeys::default();
        let mut profiles = RemoteProfiles::default();
        profiles.upsert(
            RemoteProfile::new("box", "ssh")
                .with_field("host", "h")
                .with_field("shell", "cmd"),
        );
        let attach = RemoteProfile::new("box-a", "tasty-attach")
            .with_field("ssh_ref", "box")
            .with_field("port_mode", "subcommand");
        let (_t, _rt, pm, _pf) = resolve_attach_target(&attach, &profiles, &pk).unwrap();
        assert_eq!(pm, "subcommand");
    }

    #[test]
    fn resolve_attach_inline_auto_shell_stays_auto() {
        // 인라인 + 셸 미지정(auto) + port_mode auto → auto(fallback 체인) 유지.
        let pk = Passkeys::default();
        let profiles = RemoteProfiles::default();
        let attach = RemoteProfile::new("box-a", "tasty-attach").with_field("host", "h");
        let (_t, _rt, pm, _pf) = resolve_attach_target(&attach, &profiles, &pk).unwrap();
        assert_eq!(pm, "auto");
    }

    #[test]
    fn explicit_port_file_prefers_cat_then_type() {
        // 명시 port_file 은 관례 `~/.tasty/tasty.port` 대신 그 경로를 직접 읽는다.
        let cmds = explicit_file_commands("/data/x/tasty.port");
        assert_eq!(cmds[0], "cat /data/x/tasty.port"); // 최우선(unix/powershell/git bash)
        assert_eq!(cmds[1], "type /data/x/tasty.port"); // fallback(cmd)
        // 표준 관례 경로 문자열을 포함하지 않는다.
        assert!(!cmds[0].contains(".tasty/tasty.port"));
    }

    #[test]
    fn remote_tasty_dir_branches() {
        assert_eq!(remote_tasty_dir(true), ".tasty-debug");
        assert_eq!(remote_tasty_dir(false), ".tasty");
    }

    #[test]
    fn resolve_ssh_path_nonempty() {
        let p = resolve_ssh_path();
        assert!(!p.as_os_str().is_empty());
        #[cfg(not(windows))]
        assert_eq!(p, std::path::PathBuf::from("ssh"));
    }

    #[test]
    fn backoff_grows_and_resets() {
        let mut b = Backoff::new();
        assert_eq!(b.current(), Duration::from_millis(500));
        // sleep 은 시간을 쓰므로 non-blocking `advance()`(`docs/dev-guide/attach-behavior.md`
        // "non-blocking 스케줄링" 절 — GUI 스케줄러 전용)로 증가 로직만 검증한다.
        b.advance();
        assert_eq!(b.current(), Duration::from_secs(1));
        b.cur = b.max; // 상한 확인
        b.advance();
        assert_eq!(b.current(), Duration::from_secs(30));
        b.reset();
        assert_eq!(b.current(), Duration::from_millis(500));
    }

    #[test]
    fn reserve_local_port_holds_a_usable_port() {
        // 리스너를 잡은 채 검증한다 — 예전 테스트는 확보한 포트를 놓았다가 다시
        // bind 해서, 그 사이 다른 프로세스/테스트가 같은 포트를 채가면 깨지는 TOCTOU 였다.
        let (listener, p) = reserve_local_port().unwrap();
        assert!(p > 0);
        // 판별력: 잡고 있는 동안 같은 포트 재bind 는 실패해야 한다(리스너가 실제 점유 중).
        // local_addr 재읽기는 같은 불변 속성의 재읽기라 항진명제였다 — usable 을 재지 못한다.
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", p)).is_err(),
            "reserved port {p} must be held while the listener is alive"
        );
        // drop 후 재bind 가능 여부는 OS 정책에 대한 단언이지 `reserve_local_port` 의 계약이
        // 아니다 — 커널은 방금 푼 포트를 이 프로세스에 예약하지 않으므로, 여러 워크트리가
        // 병렬로 도는 머신에서는 그 창에 남이 들어와 확률적으로 깨진다(대조군이 창을 옮겼을
        // 뿐 없애지 못했다). 앞 단언(점유 중 재bind 실패)이 계약을 결정적으로 세운다.
        drop(listener);
    }

    /// 등록된 자식은 `cancel()` 로 kill + reaping 된다 — 실제 ssh 없이 오래 사는 더미
    /// 자식(`sleep`)으로 kill+wait 계약만 고정한다. `cancel()` 이 `wait` 까지 하므로
    /// 테스트가 60초를 기다리지 않고 끝나는 것 자체가 kill 이 먹혔다는 증거다.
    #[cfg(unix)]
    #[test]
    fn cancel_kills_and_reaps_registered_child() {
        let handle = SshCancel::new();
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .spawn()
            .expect("sleep spawn");
        handle.register(child).expect("취소 전이므로 등록 성공");

        const CEILING: Duration = Duration::from_secs(10);
        // 대조군을 뺐다. 이 자리가 기다리는 자원은 fork/exec 이라 계열은 맞았지만,
        // 그 계열이 ADR-0181 의 규칙 3(값이 싸고 변동이 작다)을 못 지킨다 — 부하도
        // 유휴도 없이 기준선 대비 0.8~3.6 배, 유휴를 끼면 4.8 배까지 흔들리는 것이
        // 실측됐다. 그만큼 흔들리는 값에 판정을 붙이면 "러너가 굶었다" 를 근거 없이
        // 말하게 되고, 그것이 그 ADR 이 지금 상태보다 나쁘다고 못 박은 거짓 음성이다.
        let t0 = Instant::now();
        handle.cancel();
        let elapsed = t0.elapsed();
        assert!(elapsed < CEILING, "kill 후 즉시 reaping: {elapsed:?}");
        assert!(handle.is_cancelled());
        // cancel 이 자식을 가져가 정리했으므로 회수할 것이 남지 않는다.
        assert!(handle.reclaim().is_none());
    }

    /// 취소된 뒤 도착한 자식 등록은 거부되고 호출자에게 돌려준다(자식이 새지 않게).
    #[cfg(unix)]
    #[test]
    fn register_after_cancel_is_rejected() {
        let handle = SshCancel::new();
        handle.cancel();
        let child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .spawn()
            .expect("sleep spawn");
        let mut rejected = handle.register(child).expect_err("취소 후에는 등록 거부");
        kill_and_reap(&mut rejected);
    }

    /// 취소된 스코프에서는 새 ssh 를 **spawn 하지 않는다** — 예산 소진과 같은 자리의
    /// 조기 반환이고, 사유만 다르다(`Cancelled` vs `TimedOut`). 60초짜리 자식을 주고도
    /// 즉시 돌아오는 것이 spawn 하지 않았다는 증거다.
    #[test]
    fn cancelled_scope_skips_spawn() {
        let handle = SshCancel::new();
        let _scope = handle.scope();
        handle.cancel();
        // 띄울 수 없는 명령 — 띄웠다면 `SshConnectionFailed` 다. `Cancelled` 가 나오는
        // 것이 곧 안 띄웠다는 사건이고, 경과 상한 없이 판정된다.
        let err =
            run_capture_with_budget(unspawnable_command(), Duration::from_secs(10), "cancelled")
                .expect_err("취소된 스코프");
        assert_eq!(
            err.kind,
            PortDiscoveryFailureKind::Cancelled,
            "spawn 을 시도했으면 SshConnectionFailed 가 나왔을 것이다: {err}"
        );
    }

    /// 상한이 남아 있어도 **외부 취소**가 진행 중인 자식을 끊는다(사용자 의도 경로).
    #[cfg(unix)]
    #[test]
    fn external_cancel_cuts_child_before_budget() {
        let handle = SshCancel::new();
        let _scope = handle.scope();
        let killer = handle.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            killer.cancel();
        });
        const CEILING: Duration = Duration::from_secs(5);
        // 대조군을 뺐다. 이 자리가 기다리는 자원은 fork/exec 이라 계열은 맞았지만,
        // 그 계열이 ADR-0181 의 규칙 3(값이 싸고 변동이 작다)을 못 지킨다 — 부하도
        // 유휴도 없이 기준선 대비 0.8~3.6 배, 유휴를 끼면 4.8 배까지 흔들리는 것이
        // 실측됐다. 그만큼 흔들리는 값에 판정을 붙이면 "러너가 굶었다" 를 근거 없이
        // 말하게 되고, 그것이 그 ADR 이 지금 상태보다 나쁘다고 못 박은 거짓 음성이다.
        let started = Instant::now();
        // 예산 10초, 자식은 60초 — 취소가 없으면 10초를 다 쓴다.
        let err = run_capture_with_budget(
            never_returns_command(),
            Duration::from_secs(10),
            "external cancel",
        )
        .expect_err("취소된 조회");
        t.join().expect("killer 스레드");
        assert_eq!(err.kind, PortDiscoveryFailureKind::Cancelled);
        let elapsed = started.elapsed();
        assert!(elapsed < CEILING, "상한 만료 전에 끊겨야 한다: {elapsed:?}");
    }

    /// 취소로 죽은 자식은 **타임아웃으로 분류되지 않는다**. 취소 kill 은 시그널 종료라
    /// exit code 만 보면 상한 초과와 구분되지 않으므로, 소유권 자리에서 확정한다.
    #[cfg(unix)]
    #[test]
    fn cancelled_child_is_not_reported_as_timeout() {
        let handle = SshCancel::new();
        let _scope = handle.scope();
        let child = never_returns_command()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        let mut owner = ChildOwner::adopt(child, "cancel vs timeout").expect("adopt");

        handle.cancel(); // 자식을 가져가 kill + wait

        // 기다릴 대상이 사라졌으므로 상한 만료가 아니라 즉시 종료로 본다.
        assert!(wait_with_timeout(&mut owner, Duration::from_secs(5)));
        let err = owner.finish("cancel vs timeout").expect_err("취소된 자식");
        assert_eq!(err.kind, PortDiscoveryFailureKind::Cancelled);
    }

    /// 예산 소진과 취소가 둘 다 해당되면 취소가 이긴다(예산 0 은 `TimedOut` 의 자리지만,
    /// 사용자가 끊었다는 확정 사실이 더 정확하다).
    #[test]
    fn cancel_wins_over_exhausted_budget() {
        let handle = SshCancel::new();
        let _scope = handle.scope();
        handle.cancel();
        let err = run_capture_with_budget(never_returns_command(), Duration::ZERO, "both")
            .expect_err("취소 + 예산 0");
        assert_eq!(err.kind, PortDiscoveryFailureKind::Cancelled);
    }

    /// 반대 방향 비회귀: 취소 스코프가 설치돼 있어도 **취소하지 않았으면** 상한 초과는
    /// 그대로 `TimedOut` 이다(취소 배선이 타임아웃 분류를 가로채지 않는다).
    #[test]
    fn uncancelled_scope_still_times_out() {
        let handle = SshCancel::new();
        let _scope = handle.scope();
        let err = run_capture_with_budget(
            never_returns_command(),
            Duration::from_millis(300),
            "still timeout",
        )
        .expect_err("상한 초과");
        assert_eq!(err.kind, PortDiscoveryFailureKind::TimedOut);
        assert!(!handle.is_cancelled());
    }

    /// 스코프 가드는 drop 시 이전 설치 상태로 되돌린다(중첩/누수 방지).
    #[test]
    fn cancel_scope_restores_previous() {
        assert!(current_cancel().is_none());
        let outer = SshCancel::new();
        {
            let _g = outer.scope();
            assert!(current_cancel().is_some());
            let inner = SshCancel::new();
            {
                let _g2 = inner.scope();
                inner.cancel();
                assert!(cancel_requested());
            }
            assert!(!cancel_requested(), "가드 drop 후 outer 로 복귀");
        }
        assert!(current_cancel().is_none());
    }
}
