//! `tasty remote check` — SSH 너머 원격 tasty 인스턴스의 생존 확인(introspection).
//!
//! 포트 발견(`tasty port`/포트 파일)만으로는 **stale 포트 파일**(이미 죽은 인스턴스가
//! 남긴 파일)을 살아있다고 오판할 수 있다. 따라서 진짜 생존 확인은 세 단계를 모두 거친다:
//!   ① [`ssh::discover_remote_port`] 로 포트 후보 발견,
//!   ② [`SshTunnel::establish`] 로 `ssh -L` 터널 수립,
//!   ③ 터널 localport 로 가벼운 IPC(`system.info`) 1 회 → 응답이 와야만 alive.
//! 연결 거부/EOF/타임아웃 = dead(stale 포트). 출력은 stdout(alive)/stderr(dead, exit≠0).
//!
//! SSH 부품(포트 발견/터널)은 `attach` 와 완전히 공유한다 — 이 모듈은 그 위에
//! "IPC 1 회 + 판정" 만 얹는다. 서버측·로컬 list 핸들러는 건드리지 않는다.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tasty_i18n::{t, t_args};
use tasty_ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

use crate::out::outln;

use crate::ssh::{self, PortMode, SshTarget, SshTunnel};

/// 터널 너머 IPC 1 회 프로브의 읽기/쓰기 타임아웃. 살아있는 서버는 `system.info` 에
/// 즉시 응답하므로 짧게 잡는다 — stale 포트(EOF 전 행이 걸리는 경우)도 이 안에 끊긴다.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// `tasty remote check --ssh user@host` / `--profile <name>` 진입점.
///
/// ① 포트 발견 → ② 터널 → ③ `system.info` IPC 1 회. alive 면 포트(+버전)를 stdout 에
/// 출력하고 `Ok(())`(exit 0). 어느 단계든 실패하면 시도한 발견 모드·실패 단계를 담은
/// 에러를 반환한다(상위에서 stderr + exit≠0). `attach` 와 달리 1 회성이라 백오프 재연결은
/// 하지 않는다 — 생존 확인은 "지금 살아있나" 의 단발 판정이다.
pub fn run_remote_check(
    target: SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
) -> Result<()> {
    let ssh = ssh::resolve_ssh_path();
    let dest = target.destination.clone();
    let mode = PortMode::parse(port_mode)?;
    // 자동 검증(Claude Bash) 한정 host key accept-new. 평상시는 기본 strict 유지(보안).
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);

    // ① 원격 포트 발견. 실패 = 미발견(포트 파일 없음/원격 명령 실패) → dead.
    let remote_port =
        ssh::discover_remote_port(&ssh, &target, remote_tasty, mode, verify, debug, port_file)
            .with_context(|| t_args("cli.remote_check.not_found", &[dest.as_str(), port_mode]))?;

    // ② ssh -L 터널 (Drop 시 자식 ssh 자동 kill). 터널 수립 실패 = 인증/네트워크/포워드
    //    문제 → dead 로 판정(포트는 발견됐으나 접근 불가).
    let tunnel = SshTunnel::establish(&ssh, &target, remote_port, verify).with_context(|| {
        t_args(
            "cli.remote_check.tunnel_failed",
            &[&remote_port.to_string(), dest.as_str()],
        )
    })?;

    // ③ 터널 localport 로 가벼운 IPC 1 회. 응답 OK = alive, 연결거부/EOF/타임아웃 = dead.
    //    포트 발견만으로 alive 단정 금지 — 이 단계가 stale 포트를 가려낸다.
    let info = probe_system_info(tunnel.local_port).with_context(|| {
        t_args(
            "cli.remote_check.no_ipc_response",
            &[&remote_port.to_string(), dest.as_str()],
        )
    })?;

    // alive — 포트(+가능하면 버전/워크스페이스 수)를 stdout 출력, exit 0.
    let version = info.get("version").and_then(|v| v.as_str());
    let ws_count = info.get("workspace_count").and_then(|v| v.as_u64());
    let port_str = remote_port.to_string();
    match (version, ws_count) {
        (Some(v), Some(n)) => outln!(
            "{}",
            t_args(
                "cli.remote_check.alive_full",
                &[dest.as_str(), &port_str, v, &n.to_string()],
            )
        )?,
        (Some(v), None) => outln!(
            "{}",
            t_args(
                "cli.remote_check.alive_version",
                &[dest.as_str(), &port_str, v],
            )
        )?,
        _ => outln!(
            "{}",
            t_args("cli.remote_check.alive_basic", &[dest.as_str(), &port_str])
        )?,
    }
    Ok(())
}

/// 터널 localport 로 `system.info` IPC 1 회를 보내 응답(result)을 돌려준다.
///
/// **EOF/타임아웃을 dead 로 확정**하기 위해 [`tasty_ipc::client::IpcConnection`] 을
/// 재사용하지 않는다 — 그 구현은 빈 줄(EOF)에서 `continue` 로 무한 루프에 빠지므로
/// stale 포트(서버가 채널을 닫아 EOF) 케이스에서 행이 걸린다. 여기서는 read 타임아웃을
/// 걸고 EOF/빈 응답을 명시적 에러로 변환한다.
fn probe_system_info(port: u16) -> Result<serde_json::Value> {
    let stream = TcpStream::connect(("127.0.0.1", port))
        .with_context(|| t_args("cli.remote_check.connect_failed", &[&port.to_string()]))?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT))?;

    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "system.info".to_string(),
        params: serde_json::json!({}),
        id: Some(serde_json::json!(1)),
        session_token: std::env::var("TASTY_SESSION_TOKEN")
            .ok()
            .filter(|s| !s.is_empty()),
    };

    let mut writer = stream.try_clone()?;
    let json = serde_json::to_string(&request)?;
    writeln!(writer, "{json}").context(t("cli.remote_check.send_failed"))?;
    writer.flush().context(t("cli.remote_check.flush_failed"))?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .context(t("cli.remote_check.read_failed"))?;
    if n == 0 || line.trim().is_empty() {
        bail!("{}", t("cli.remote_check.eof"));
    }

    let response: JsonRpcResponse =
        serde_json::from_str(line.trim()).context(t("cli.remote_check.parse_failed"))?;
    if let Some(err) = response.error {
        bail!(
            "{}",
            t_args(
                "cli.remote_check.error_response",
                &[&err.code.to_string(), err.message.as_str()],
            )
        );
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// 살아있는 서버 흉내: 한 줄(요청)을 읽고 유효한 system.info 응답을 돌려준다.
    /// probe 가 result(version/workspace_count)를 그대로 파싱하면 alive.
    #[test]
    fn probe_alive_parses_system_info() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            // 요청을 개행까지 완전 소비해야 한다. 부분 read 후 close 하면 미소비
            // 데이터 때문에 FIN 대신 RST 가 나가 클라이언트 read 가 간헐 실패한다.
            let mut request = String::new();
            BufReader::new(sock.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let resp =
                br#"{"jsonrpc":"2.0","result":{"version":"9.9.9","workspace_count":3},"id":1}"#;
            sock.write_all(resp).unwrap();
            sock.write_all(b"\n").unwrap();
        });
        let info = probe_system_info(port).expect("alive");
        assert_eq!(info.get("version").and_then(|v| v.as_str()), Some("9.9.9"));
        assert_eq!(
            info.get("workspace_count").and_then(|v| v.as_u64()),
            Some(3)
        );
        h.join().unwrap();
    }

    /// stale 포트 흉내: 연결을 수락한 즉시 닫는다(EOF). probe 는 무한 루프(IpcConnection
    /// 버그)에 빠지지 않고 즉시 에러로 dead 판정해야 한다. 이 테스트가 끝나면(행 없이
    /// 반환되면) 그 자체가 no-hang 회귀 고정이다.
    #[test]
    fn probe_stale_port_eof_is_error_not_hang() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            drop(sock); // 즉시 닫음 → client 는 EOF.
        });
        let r = probe_system_info(port);
        assert!(r.is_err(), "EOF 는 dead(에러)여야 한다: {r:?}");
        h.join().unwrap();
    }

    /// 서버 없는 포트: 연결 거부 → 에러(dead). 포트 발견만으로 alive 단정 금지의 핵심.
    ///
    /// **놓고 쓰는 형태라 재시도로 감싼다.** "닫힌 포트" 를 만드는 방법이 bind 했다 놓는
    /// 것뿐인데, 놓은 순간부터 probe 까지 사이에 같은 머신의 다른 완주가 그 포트를 집어갈
    /// 수 있다. 집어간 것이 하필 tasty 면 probe 가 성공해 이 단언이 뒤집힌다 — 확률적
    /// red 다(`docs/adr/0129-flaky-test-classes-and-standard-fixes.md` 형태 B 역방향:
    /// 자원을 쥐는 처방이 안 맞으므로 재시도 + 약한 단언이 정본이다).
    ///
    /// 한 번 성공하면 우연일 수 있으니 **다른 포트로 다시 잰다.** 세 포트가 전부 성공하면
    /// 그때는 우연이 아니라 판정 대상의 결함이다.
    #[test]
    fn probe_connection_refused_is_error() {
        let mut answered = Vec::new();
        for _ in 0..3 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            match probe_system_info(port) {
                Err(_) => return,
                Ok(v) => answered.push((port, v)),
            }
        }
        panic!("연결 거부는 dead(에러)여야 한다 — 세 포트가 전부 응답했다: {answered:?}");
    }
}
