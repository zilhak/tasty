//! 원격 워크스페이스 브라우징 능력 (N-RA01) — attach 프로필/ssh 대상에 붙어 원격
//! tasty 인스턴스의 워크스페이스 목록을 받아온다.
//!
//! 이 모듈은 **client 측 순수 조회 능력**이다 — 로컬 사용자 상태(focus/닫은항목
//! 히스토리/선택·스크롤·커서)에 전혀 닿지 않는다. "원격에 나가서 목록을 읽는" 행위라
//! 로컬 IPC 서버 핸들러가 아니라 CLI/호스트 client 로직이 주체다. CLI(`tasty remote
//! workspaces`)와 로컬 IPC method(`remote.workspaces`)가 **양쪽 모두** 이 함수를 공유해
//! 호출한다(원칙 2 — 에이전트가 CLI 없이 소켓만으로도 브라우징 가능).
//!
//! 흐름: `attach 프로필/ssh` → 접속 스펙 resolve → (SSH 터널 or loopback 직결) → 그
//! 포트로 `workspace.list` + `attach.list` JSON-RPC 각 1회 → 병합 → 목록 반환.
//! `attached`(타 client 점유) 여부는 `workspace.list` 응답에 없으므로 `attach.list` 의
//! workspace-level lock 과 병합해 채운다(2회 IPC).
//!
//! 블로킹 I/O(SSH 터널 수립·소켓 read)를 하므로 **이벤트루프에서 직접 호출하면 안 된다**
//! — 호스트 IPC 경로는 워커 스레드에서 호출한다(`src/app/ipc/app_methods.rs`).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tasty_ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use tasty_remote_profiles::{Passkeys, RemoteProfiles};

use tasty_ssh::{self as ssh, PortMode, SshTarget, SshTunnel};

/// 원격 프로브 1회의 읽기/쓰기 타임아웃. 살아있는 서버는 즉시 응답하므로 짧게 잡는다
/// — stale 포트(EOF 전 행이 걸리는 경우)도 이 안에 끊긴다(no-hang 보장).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// 원격 tasty 의 워크스페이스 1개(browse 결과). 필드는 `workspace.list` 응답에서
/// 뽑고, `attached`/`holder` 만 `attach.list` 와 병합해 채운다.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RemoteWorkspace {
    pub id: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub pane_count: u32,
    pub busy_count: u32,
    /// 타 client 가 이 워크스페이스를 점유(attach)중인지 — `attach.list` 의 workspace
    /// lock 존재 여부. RA02 팝업이 "이미 점유된 원격 ws" 를 구분/경고하는 데 쓴다.
    pub attached: bool,
    /// 점유중이면 lock holder(원격의 `AttachClientId` = u32; 없으면 None).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holder: Option<u32>,
}

/// `--profile` / `--ssh` 입력을 `(SshTarget, remote_tasty, port_mode, port_file)` 로
/// resolve 한다. CLI 선처리와 호스트 IPC 워커가 공유한다(중복 제거).
///
/// - `profile`: 저장된 tasty-attach 프로필 → [`ssh::resolve_attach_target`](비활성 게이트
///   포함). `remote_tasty`/`remote_port_mode` 는 프로필 값으로 대체된다.
/// - `ssh`: 1회성 대상(`user@host`/alias/`127.0.0.1:PORT`). CLI 인자의 `remote_tasty`/
///   `remote_port_mode` 를 그대로 쓴다.
///
/// 둘 다 지정/둘 다 없음은 호출자가 사전에 거부한다(여기서는 profile 우선).
pub fn resolve_connection_spec(
    profile: Option<&str>,
    ssh: Option<&str>,
    remote_tasty: &str,
    remote_port_mode: &str,
) -> Result<(SshTarget, String, String, Option<String>)> {
    match profile {
        Some(name) => {
            let profiles = RemoteProfiles::load();
            let passkeys = Passkeys::load();
            let p = profiles
                .get(name)
                .with_context(|| tasty_i18n::t_fmt("cli.remote_profile.not_found", name))?;
            ssh::resolve_attach_target(p, &profiles, &passkeys)
        }
        None => match ssh {
            Some(dest) => Ok((
                SshTarget::parse(dest),
                remote_tasty.to_string(),
                remote_port_mode.to_string(),
                None,
            )),
            None => bail!("원격 대상이 필요합니다 (--ssh 또는 --profile)."),
        },
    }
}

/// `127.0.0.1:PORT` / `localhost:PORT` / `[::1]:PORT` 면 PORT 를 돌려준다(loopback 직결).
/// 그 외(원격 호스트/alias)는 None → SSH 터널 경로.
pub(crate) fn parse_loopback_port(dest: &str) -> Option<u16> {
    let (host, port_str) = if let Some(rest) = dest.strip_prefix("[::1]:") {
        ("::1", rest)
    } else {
        let (h, p) = dest.rsplit_once(':')?;
        (h, p)
    };
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return None;
    }
    port_str.parse::<u16>().ok()
}

/// 접속 스펙 → 접속 엔드포인트 `(터널 핸들 or None, 접속 포트)`. **블로킹**(SSH 터널
/// 수립은 최대 수초) — 워커 스레드에서 호출한다.
///
/// loopback 대상이면 터널 없이(`None`) 그 포트로 직결한다(로컬 e2e — `--ssh
/// 127.0.0.1:<port>`). 원격 대상이면 포트 발견 → `ssh -L` 터널 → local_port.
///
/// 반환된 [`SshTunnel`] 은 Drop 시 자식 ssh 를 kill 한다. browse 는 조회 직후 Drop
/// (단발), 호스트 attach 는 mirror 세션에 실어 살려둔다(고아 터널 방지).
pub fn resolve_endpoint(
    target: &SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
) -> Result<(Option<SshTunnel>, u16)> {
    // ① loopback 직결(터널 없이).
    if let Some(port) = parse_loopback_port(&target.destination) {
        return Ok((None, port));
    }

    // ② SSH 터널: 원격 포트 발견 → ssh -L → local_port.
    let ssh = ssh::resolve_ssh_path();
    let mode = PortMode::parse(port_mode)?;
    // 자동 검증(Claude Bash) 한정 host key accept-new. 평상시 기본 strict 유지(보안).
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);
    let remote_port =
        ssh::discover_remote_port(&ssh, target, remote_tasty, mode, verify, debug, port_file)?;
    let tunnel = SshTunnel::establish(&ssh, target, remote_port, verify)?;
    let local_port = tunnel.local_port;
    Ok((Some(tunnel), local_port))
}

/// 터널/loopback localport 로 JSON-RPC 1회를 보내 응답 result 를 돌려준다(범용 프로브).
///
/// `remote_check` 의 `probe_system_info` 와 같은 이유로 [`tasty_ipc::client::IpcConnection`]
/// 을 쓰지 않는다 — 그 구현은 빈 줄(EOF)에서 무한 루프에 빠져 stale 포트에서 행이
/// 걸린다. 여기서는 read/write 타임아웃을 걸고 EOF/빈 응답을 명시적 에러로 변환한다.
///
/// 본체도 쓴다 — RA02 "+ 새 워크스페이스" 행(`src/adapters/ui/popup/remote_attach.rs`
/// `spawn_create`)이 `workspace.create` 응답에서 id 를 못 찾은 경우를 [`create::
/// create_via_port`] 의 범용 anyhow 에러가 아니라 로컬라이즈된 문구로 구분해 보여주려고
/// 이 원시 프로브를 직접 쓴다 — `create_via_port` 를 쓰면 그 구분이 사라진다.
pub fn probe_method(
    port: u16,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    let stream = TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("터널 localport 127.0.0.1:{port} 연결 실패"))?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT))?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT))?;

    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
        session_token: std::env::var("TASTY_SESSION_TOKEN")
            .ok()
            .filter(|s| !s.is_empty()),
    };

    let mut writer = stream.try_clone()?;
    let json = serde_json::to_string(&request)?;
    writeln!(writer, "{json}").context("IPC 요청 전송 실패")?;
    writer.flush().context("IPC 요청 flush 실패")?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let n = reader
        .read_line(&mut line)
        .context("IPC 응답 read 실패 (타임아웃/연결 종료)")?;
    if n == 0 || line.trim().is_empty() {
        bail!("IPC 응답 없음 (EOF) — 원격 서버가 응답하지 않습니다");
    }

    let response: JsonRpcResponse =
        serde_json::from_str(line.trim()).context("IPC 응답 JSON 파싱 실패")?;
    if let Some(err) = response.error {
        bail!("IPC 에러 응답 (code={}): {}", err.code, err.message);
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
}

/// 접속된 포트로 `workspace.list` + `attach.list` 를 조회해 병합한 목록을 만든다.
/// 터널 수명은 호출자가 관리한다(browse 는 조회 후 Drop, attach 는 별개).
pub fn browse_via_port(port: u16) -> Result<Vec<RemoteWorkspace>> {
    let ws_list = probe_method(port, "workspace.list", serde_json::json!({}))
        .context("원격 workspace.list 조회 실패")?;
    // attach.list 는 병합용 부가 정보 — 실패해도 목록 자체는 반환(점유 표시만 생략).
    let attach_list =
        probe_method(port, "attach.list", serde_json::json!({})).unwrap_or(serde_json::Value::Null);

    // attach.list.workspaces 에서 workspace_id → holder(AttachClientId=u32) 맵 구성.
    // 점유 판정은 **workspace 단위 lock** 기준 — workspace.list 응답이 멤버 surface id
    // 를 노출하지 않아 surface 단위 lock 을 특정 ws 에 귀속시킬 수 없기 때문이다(2회 IPC
    // 제약). 전체 워크스페이스 attach 는 workspace lock 을 잡으므로 "이 워크스페이스가
    // 이미 점유됐나" 는 이 신호로 판정된다.
    let mut ws_holders: std::collections::HashMap<u32, Option<u32>> =
        std::collections::HashMap::new();
    if let Some(arr) = attach_list.get("workspaces").and_then(|v| v.as_array()) {
        for w in arr {
            if let Some(wid) = w.get("workspace_id").and_then(|v| v.as_u64()) {
                let holder = w.get("holder").and_then(|v| v.as_u64()).map(|h| h as u32);
                ws_holders.insert(wid as u32, holder);
            }
        }
    }

    let arr = ws_list
        .as_array()
        .context("원격 workspace.list 응답이 배열이 아닙니다")?;
    let mut out = Vec::with_capacity(arr.len());
    for ws in arr {
        let id = ws.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let holder_entry = ws_holders.get(&id);
        out.push(RemoteWorkspace {
            id,
            name: ws
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            subtitle: ws
                .get("subtitle")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            description: ws
                .get("description")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            pane_count: ws.get("pane_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            busy_count: ws.get("busy_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            attached: holder_entry.is_some(),
            holder: holder_entry.and_then(|h| *h),
        });
    }
    Ok(out)
}

/// 전체 browse: 접속 스펙 resolve → 엔드포인트(터널/loopback) → 목록 조회. **블로킹**
/// (SSH) — 워커 스레드/CLI 프로세스에서 호출한다. 터널은 이 함수 반환 시 Drop(단발).
pub fn browse(
    target: &SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
) -> Result<Vec<RemoteWorkspace>> {
    let (_tunnel, port) = resolve_endpoint(target, remote_tasty, port_mode, port_file)?;
    // _tunnel 은 이 스코프 끝에서 Drop → 자식 ssh kill(단발 조회이므로 여기서 정리).
    browse_via_port(port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn loopback_ports_parsed() {
        assert_eq!(parse_loopback_port("127.0.0.1:45123"), Some(45123));
        assert_eq!(parse_loopback_port("localhost:8080"), Some(8080));
        assert_eq!(parse_loopback_port("[::1]:9000"), Some(9000));
    }

    #[test]
    fn non_loopback_is_none() {
        assert_eq!(parse_loopback_port("gx10"), None);
        assert_eq!(parse_loopback_port("user@host"), None);
        assert_eq!(parse_loopback_port("192.168.0.10:45123"), None);
        assert_eq!(parse_loopback_port("example.com:22"), None);
    }

    /// mock 서버가 한 줄(요청)을 읽고 유효한 workspace.list 배열을 돌려준다 →
    /// browse_via_port 가 목록을 파싱하는지. attach.list 는 ws 2 를 점유중으로 응답 →
    /// 병합으로 attached/holder 가 채워지는지 검증.
    #[test]
    fn browse_via_port_parses_and_merges_attach() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            // 첫 연결: workspace.list 응답.
            {
                let (mut sock, _) = listener.accept().unwrap();
                let mut req = String::new();
                BufReader::new(sock.try_clone().unwrap())
                    .read_line(&mut req)
                    .unwrap();
                let resp = br#"{"jsonrpc":"2.0","result":[{"id":1,"name":"alpha","pane_count":2,"busy_count":0},{"id":2,"name":"beta","pane_count":1,"busy_count":1}],"id":1}"#;
                sock.write_all(resp).unwrap();
                sock.write_all(b"\n").unwrap();
            }
            // 둘째 연결: attach.list 응답(ws 2 점유중).
            {
                let (mut sock, _) = listener.accept().unwrap();
                let mut req = String::new();
                BufReader::new(sock.try_clone().unwrap())
                    .read_line(&mut req)
                    .unwrap();
                let resp = br#"{"jsonrpc":"2.0","result":{"attached":[],"workspaces":[{"workspace_id":2,"holder":7,"granted_seq":5}]},"id":1}"#;
                sock.write_all(resp).unwrap();
                sock.write_all(b"\n").unwrap();
            }
        });
        let list = browse_via_port(port).expect("browse ok");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, 1);
        assert_eq!(list[0].name, "alpha");
        assert!(!list[0].attached);
        assert_eq!(list[0].holder, None);
        assert_eq!(list[1].id, 2);
        assert!(list[1].attached);
        assert_eq!(list[1].holder, Some(7));
        assert_eq!(list[1].busy_count, 1);
        h.join().unwrap();
    }

    /// stale 포트(즉시 close/EOF) → probe 가 무한 루프 없이 즉시 에러(no-hang 회귀 고정).
    #[test]
    fn probe_stale_port_eof_is_error_not_hang() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            drop(sock);
        });
        let r = probe_method(port, "workspace.list", serde_json::json!({}));
        assert!(r.is_err(), "EOF 는 에러여야 한다: {r:?}");
        h.join().unwrap();
    }

    /// 닫힌 포트 → 연결 거부 에러.
    ///
    /// **놓고 쓰는 형태라 재시도로 감싼다.** "닫힌 포트" 는 bind 했다 놓아야만 만들 수
    /// 있고, 놓은 순간부터 probe 까지 사이에 같은 머신의 다른 완주가 그 포트를 집어갈 수
    /// 있다. 집어간 것이 하필 tasty 면 probe 가 성공해 단언이 뒤집힌다
    /// (`docs/adr/0129-flaky-test-classes-and-standard-fixes.md` 형태 B 역방향 — 자원을
    /// 쥐는 처방이 안 맞아 재시도 + 약한 단언이 정본이다). 세 포트가 전부 응답하면 그때는
    /// 우연이 아니다.
    #[test]
    fn probe_connection_refused_is_error() {
        let mut answered = Vec::new();
        for _ in 0..3 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            match probe_method(port, "workspace.list", serde_json::json!({})) {
                Err(_) => return,
                Ok(v) => answered.push((port, v)),
            }
        }
        panic!("연결 거부는 에러여야 한다 — 세 포트가 전부 응답했다: {answered:?}");
    }
}
