//! 시스템 ssh 위임 — SSH 1회성 터널 전송 (attach/detach 단계 5).
//!
//! tasty 는 자체 원격 프로토콜/암호화를 만들지 않고 **시스템 ssh 에 위임**한다.
//! 이 모듈은 CLI(client) 안에만 존재한다 — 호스트(`src/`)·IPC 서버는 SSH 를 전혀
//! 모르고 loopback(`127.0.0.1`) 만 안다. "원격성" 은 전부 client 가 흡수한다.
//!
//! 흐름(단계 4 attach 를 SSH 너머로):
//! 1. [`resolve_ssh_path`] — 시스템 ssh 경로(Windows 는 System32 OpenSSH 풀경로).
//! 2. [`discover_remote_port`] — 원격 tasty 데몬의 IPC 포트 발견(auto fallback 체인).
//! 3. [`SshTunnel::establish`] — `ssh -L 127.0.0.1:local:127.0.0.1:remote -N` 백그라운드.
//! 4. client 가 `127.0.0.1:local` 로 단계 4 attach (commands::attach).
//!
//! Windows 는 반드시 시스템 OpenSSH 풀경로를 쓴다 — git 번들 ssh 는 윈도우
//! ssh-agent(named pipe `\\.\pipe\openssh-ssh-agent`) 를 못 봐 무암호 인증이 실패한다.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
}

/// attach 소비자용 헬퍼 — 프로필을 `(SshTarget, remote_tasty, port_mode)` 로 resolve.
/// ssh kind 검증 + 비활성(detect 실패) 게이트 + passkey resolve 를 한곳에서 처리한다
/// (auto_attach / `remote attach` / `remote check` 3곳의 중복 제거).
pub fn resolve_attach_target(
    p: &RemoteProfile,
    passkeys: &Passkeys,
) -> Result<(SshTarget, String, String)> {
    let v = p.as_ssh().ok_or_else(|| {
        anyhow::anyhow!("attach 는 ssh kind 프로필만 지원합니다 (kind='{}')", p.kind)
    })?;
    if v.is_disabled() {
        bail!("프로필 '{}' 은 비활성(셸 감지 실패) — 재감지가 필요합니다", p.name);
    }
    let remote_tasty = v.remote_tasty().to_string();
    let port_mode = v.port_mode().to_string();
    let target = SshTarget::from_remote_profile(p, passkeys)?;
    Ok((target, remote_tasty, port_mode))
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
    /// Unix 원격: `cat ~/.tasty/<port-file>`.
    FileUnix,
    /// Windows 원격: `type %USERPROFILE%\.tasty\<port-file>`.
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

/// ssh 공통 `-o` 옵션을 인자 벡터에 추가한다.
///
/// - `BatchMode=no`: 첫 연결의 host key/passphrase 프롬프트 허용(무암호면 무영향).
/// - `ServerAliveInterval`/`CountMax`: 네트워크 단절 감지(~45s 내 ssh 자가 종료).
/// - `verify` 시 `StrictHostKeyChecking=accept-new`: 자동 검증 한정(평상시 기본 strict 유지).
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
}

/// 원격에서 한 줄 명령을 실행하고 stdout 을 캡처한다(포트 발견용).
fn run_ssh_capture(
    ssh: &Path,
    target: &SshTarget,
    verify: bool,
    remote_argv: &[&str],
) -> Result<String> {
    let mut args: Vec<String> = Vec::new();
    push_common_opts(&mut args, target, verify);
    args.push(target.destination.clone());
    for a in remote_argv {
        args.push((*a).to_string());
    }
    let output = Command::new(ssh)
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| anyhow::anyhow!("ssh spawn 실패({}): {e}", ssh.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "원격 명령 실패(code={:?}): {}",
            output.status.code(),
            stderr.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// stdout 텍스트에서 포트 숫자를 파싱한다(trailing newline/CR 허용).
fn parse_port(stdout: &str) -> Result<u16> {
    stdout
        .trim()
        .lines()
        .next_back()
        .unwrap_or("")
        .trim()
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("원격 포트 파싱 실패: {:?}", stdout.trim()))
}

/// debug/release 빌드에 맞는 포트 파일명.
fn port_file_name(debug: bool) -> &'static str {
    if debug {
        "tasty-debug.port"
    } else {
        "tasty.port"
    }
}

/// auto 체인 첫 단계: `ssh <dest> <remote-tasty> port` → stdout 의 포트 숫자.
fn discover_via_subcommand(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    verify: bool,
) -> Result<u16> {
    let out = run_ssh_capture(ssh, target, verify, &[remote_tasty, "port"])?;
    parse_port(&out)
}

/// OS 분기 file 모드(decisions 9 fallback): Unix `cat` / Windows `type`.
fn discover_via_file(
    ssh: &Path,
    target: &SshTarget,
    windows: bool,
    verify: bool,
    debug: bool,
) -> Result<u16> {
    let fname = port_file_name(debug);
    let remote_cmd = if windows {
        format!("type %USERPROFILE%\\.tasty\\{fname}")
    } else {
        format!("cat ~/.tasty/{fname}")
    };
    let out = run_ssh_capture(ssh, target, verify, &[&remote_cmd])?;
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

/// 원격 tasty 데몬의 IPC 포트를 발견한다(plan §4.4).
///
/// `Auto` 는 [`AUTO_FALLBACK_CHAIN`] 순서로 단일 모드를 차례로 시도하고, 한 모드라도
/// 포트를 내면 즉시 반환한다. subcommand 는 Windows release 에서 "빈 출력 + exit 0"
/// 으로 조용히 실패할 수 있는데, 이 경우 [`parse_port`] 가 에러를 내며 다음 단계로
/// 넘어간다(exit code 만으로는 감지 불가).
pub fn discover_remote_port(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    mode: PortMode,
    verify: bool,
    debug: bool,
) -> Result<u16> {
    if mode != PortMode::Auto {
        return discover_single_mode(ssh, target, remote_tasty, mode, verify, debug);
    }

    let mut last_err = None;
    for m in AUTO_FALLBACK_CHAIN {
        match discover_single_mode(ssh, target, remote_tasty, m, verify, debug) {
            Ok(port) => return Ok(port),
            Err(e) => {
                tracing::debug!("{m:?} 포트 발견 실패({e}) — 다음 모드로 fallback");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.expect("AUTO_FALLBACK_CHAIN 은 비어있지 않음"))
}

/// 단일(비-Auto) 모드 1회 시도. `Auto` 는 호출 전 체인으로 분해되므로 unreachable.
fn discover_single_mode(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    mode: PortMode,
    verify: bool,
    debug: bool,
) -> Result<u16> {
    match mode {
        PortMode::Subcommand => discover_via_subcommand(ssh, target, remote_tasty, verify),
        PortMode::FileUnix => discover_via_file(ssh, target, false, verify, debug),
        PortMode::FileWindows => discover_via_file(ssh, target, true, verify, debug),
        PortMode::Auto => unreachable!("Auto 는 AUTO_FALLBACK_CHAIN 으로 분해됨"),
    }
}

/// 자동감지: 프로브 체인([`AUTO_FALLBACK_CHAIN`])을 순서대로 시도해 **첫 성공 모드**를
/// 돌려준다(셸 종류를 묻는 단일 명령이 없으므로 프로브 성패가 곧 감지 결과 — TODO 04).
/// 전 프로브 실패 시 마지막 에러를 반환한다.
///
/// `try_mode` 는 단일 모드를 시도해 포트를 내는 클로저 — 실제 SSH 실행([`detect_port_mode`])
/// 또는 테스트용 mock 을 주입할 수 있다.
fn detect_first_success<F>(mut try_mode: F) -> Result<PortMode>
where
    F: FnMut(PortMode) -> Result<u16>,
{
    let mut last_err = None;
    for m in AUTO_FALLBACK_CHAIN {
        match try_mode(m) {
            Ok(_) => return Ok(m),
            Err(e) => {
                tracing::debug!("{m:?} 감지 프로브 실패({e}) — 다음 모드");
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("프로브 체인이 비어있음")))
}

/// 원격 셸 자동감지 — 프로브 체인을 실제 SSH 로 1회씩 돌려 첫 성공 모드를 반환한다.
/// 네트워크 I/O(1~3 왕복)로 수 초 블록될 수 있다 — 호출자가 적절한 스레드에서 실행.
pub fn detect_port_mode(
    ssh: &Path,
    target: &SshTarget,
    remote_tasty: &str,
    verify: bool,
    debug: bool,
) -> Result<PortMode> {
    detect_first_success(|m| discover_single_mode(ssh, target, remote_tasty, m, verify, debug))
}

/// 한 프로필의 `shell` 값에 맞춰 `port_mode` / `detect_failed` 를 갱신한다.
///
/// - 명시 셸(powershell/cmd/bash/zsh) → [`shell_to_port_mode`] 로 `port_mode` 즉시 도출
///   (네트워크 I/O 없음), `detect_failed=false`. 반환 `None`.
/// - `auto` → 실제 SSH 프로브 체인 1회 실행(블록). 성공 시 `port_mode`=감지 모드 +
///   `detect_failed=false`, 실패 시 `detect_failed=true`. 반환 `Some(결과)`.
///
/// `auto` 분기는 네트워크 I/O 로 수 초 블록될 수 있다 — GUI/host 는 워커 스레드에서 호출.
pub fn apply_shell_to_profile(
    profile: &mut RemoteProfile,
    passkeys: &Passkeys,
) -> Option<Result<PortMode>> {
    let shell = profile.as_ssh().map(|v| v.shell().to_string()).unwrap_or_else(|| "auto".into());
    if let Some(mode) = shell_to_port_mode(&shell) {
        profile.set_field("port_mode", mode);
        profile.remove_field("detect_failed");
        return None;
    }
    // shell == auto (또는 알 수 없는 값) → 감지.
    let outcome = detect_for_profile(profile, passkeys);
    match &outcome {
        Ok(mode) => {
            profile.set_field("port_mode", mode.as_str());
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
    let remote_tasty = profile
        .as_ssh()
        .map(|v| v.remote_tasty().to_string())
        .unwrap_or_else(|| "tasty".into());
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);
    detect_port_mode(&ssh, &target, &remote_tasty, verify, debug)
}

/// 이름으로 프로필을 로드→재감지→저장한다(GUI 새로고침/IPC detect 워커 진입점).
///
/// 셸 무관하게 프로브 체인을 돌려, 성공 시 `port_mode`=감지 모드 + 활성화,
/// 실패 시 `detect_failed=true`(비활성)로 toml 을 **항상 갱신**한다. 네트워크 I/O 블록.
pub fn detect_and_persist(name: &str) -> Result<PortMode> {
    let mut profiles = RemoteProfiles::load();
    let passkeys = Passkeys::load();
    let Some(mut p) = profiles.get(name).cloned() else {
        bail!("원격 프로필 '{name}' 을 찾을 수 없습니다");
    };
    let result = detect_for_profile(&p, &passkeys);
    match &result {
        Ok(mode) => {
            p.set_field("port_mode", mode.as_str());
            p.remove_field("detect_failed");
        }
        Err(_) => p.set_field("detect_failed", "true"),
    }
    profiles.upsert(p);
    profiles.save()?;
    result
}

/// `127.0.0.1:0` 바인드로 비어있는 로컬 포트를 확보한 뒤 즉시 해제한다(ssh 가 다시
/// bind). 짧은 TOCTOU 레이스가 있으나 표준 관행(충돌 시 ready-probe 타임아웃 → 재시도).
fn pick_local_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
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
        let local_port = pick_local_port()?;
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

        let child = Command::new(ssh)
            .args(&args)
            .stdin(Stdio::null())
            // stderr 는 인증/에러 노출용으로 상속(첫 연결 host key/passphrase).
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

impl Drop for SshTunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();  // best-effort 자식 종료 — 이미 종료됐을 수 있음, 무시
        let _ = self.child.wait();  // 좀비 방지 reaping — 실패 무시
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
    fn resolve_attach_target_rejects_non_ssh_and_disabled() {
        let pk = Passkeys::default();
        // 비-ssh kind 거부.
        let http = RemoteProfile::new("site", "http").with_field("url", "https://x");
        assert!(resolve_attach_target(&http, &pk).is_err());
        // 비활성(detect 실패) 거부.
        let disabled = RemoteProfile::new("x", "ssh")
            .with_field("host", "h")
            .with_field("detect_failed", "true");
        assert!(resolve_attach_target(&disabled, &pk).is_err());
        // 정상 — (target, remote_tasty, port_mode).
        let ok = RemoteProfile::new("y", "ssh")
            .with_field("host", "h")
            .with_field("remote_tasty", "/usr/bin/tasty")
            .with_field("port_mode", "file-unix");
        let (t, rt, pm) = resolve_attach_target(&ok, &pk).unwrap();
        assert_eq!(t.destination, "h");
        assert_eq!(rt, "/usr/bin/tasty");
        assert_eq!(pm, "file-unix");
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
                Err(anyhow::anyhow!("probe {m:?} failed"))
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
        let r = detect_first_success(|m| Err(anyhow::anyhow!("{m:?} down")));
        assert!(r.is_err());
    }

    #[test]
    fn apply_shell_explicit_sets_mode_without_io() {
        // 명시 셸 → 매핑으로 port_mode 즉시 도출(감지 미실행, None 반환).
        let pk = Passkeys::default();
        let mut p = RemoteProfile::new("x", "ssh").with_field("host", "h").with_field("shell", "cmd");
        let ran = apply_shell_to_profile(&mut p, &pk);
        assert!(ran.is_none()); // 감지 미실행.
        assert_eq!(p.as_ssh().unwrap().port_mode(), "file-windows");
        assert!(!p.as_ssh().unwrap().is_disabled());

        p.set_field("shell", "powershell");
        apply_shell_to_profile(&mut p, &pk);
        assert_eq!(p.as_ssh().unwrap().port_mode(), "file-unix");

        p.set_field("shell", "bash");
        apply_shell_to_profile(&mut p, &pk);
        assert_eq!(p.as_ssh().unwrap().port_mode(), "subcommand");
    }

    #[test]
    fn port_file_name_branches() {
        assert_eq!(port_file_name(true), "tasty-debug.port");
        assert_eq!(port_file_name(false), "tasty.port");
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
        assert_eq!(b.cur, Duration::from_millis(500));
        // sleep 은 시간을 쓰므로 증가 로직만 직접 검증.
        b.cur = (b.cur * 2).min(b.max);
        assert_eq!(b.cur, Duration::from_secs(1));
        b.cur = b.max; // 상한 확인
        b.cur = (b.cur * 2).min(b.max);
        assert_eq!(b.cur, Duration::from_secs(30));
        b.reset();
        assert_eq!(b.cur, Duration::from_millis(500));
    }

    #[test]
    fn pick_local_port_returns_usable() {
        let p = pick_local_port().unwrap();
        assert!(p > 0);
        // 해제됐으니 다시 bind 가능해야 함.
        let l = std::net::TcpListener::bind(("127.0.0.1", p));
        assert!(l.is_ok());
    }
}
