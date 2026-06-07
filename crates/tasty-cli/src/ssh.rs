//! 시스템 ssh 위임 — SSH 1회성 터널 전송 (attach/detach 단계 5).
//!
//! tasty 는 자체 원격 프로토콜/암호화를 만들지 않고 **시스템 ssh 에 위임**한다.
//! 이 모듈은 CLI(client) 안에만 존재한다 — 호스트(`src/`)·IPC 서버는 SSH 를 전혀
//! 모르고 loopback(`127.0.0.1`) 만 안다. "원격성" 은 전부 client 가 흡수한다.
//!
//! 흐름(단계 4 attach 를 SSH 너머로):
//! 1. [`resolve_ssh_path`] — 시스템 ssh 경로(Windows 는 System32 OpenSSH 풀경로).
//! 2. [`discover_remote_port`] — 원격 tasty 데몬의 IPC 포트 발견(셸 비의존 우선).
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
#[derive(Clone, Debug)]
pub struct SshTarget {
    /// ssh 에 그대로 넘길 destination (`user@host` | `host` | config alias).
    pub destination: String,
    /// 사용자가 명시한 ssh 포트(없으면 ssh config / 22 위임).
    pub ssh_port: Option<u16>,
}

impl SshTarget {
    /// `user@host` / `host` / config alias 를 그대로 보관한다. ssh 포트는
    /// `~/.ssh/config` 에 위임하므로 여기서는 destination 만 받는다.
    pub fn parse(dest: &str) -> Self {
        Self {
            destination: dest.to_string(),
            ssh_port: None,
        }
    }
}

/// 원격 포트 발견 모드(plan §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortMode {
    /// 셸 비의존: `ssh <dest> <remote-tasty> port` (권장).
    Subcommand,
    /// Unix 원격: `cat ~/.tasty/<port-file>`.
    FileUnix,
    /// Windows 원격: `type %USERPROFILE%\.tasty\<port-file>`.
    FileWindows,
    /// subcommand 먼저, 실패 시 unix file fallback(기본).
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

/// 셸 비의존: `ssh <dest> <remote-tasty> port` → stdout 의 포트 숫자.
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

/// 원격 tasty 데몬의 IPC 포트를 발견한다(plan §4.4).
///
/// `Auto` 는 셸 비의존 subcommand 를 먼저 시도하고, 실패하면 Unix file 로 fallback
/// 한다(Windows 원격은 `--remote-port-mode subcommand` 권장 — file 모드의 type/셸
/// 분기 위험 회피).
pub fn discover_remote_port(
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
        PortMode::Auto => discover_via_subcommand(ssh, target, remote_tasty, verify).or_else(|e| {
            tracing::debug!("subcommand 포트 발견 실패({e}) — unix file fallback");
            discover_via_file(ssh, target, false, verify, debug)
        }),
    }
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
        let _ = self.child.kill();
        let _ = self.child.wait();
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
