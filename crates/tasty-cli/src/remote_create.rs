//! 원격 워크스페이스 **생성** 능력 — attach 프로필/ssh 대상에 붙어 원격 tasty
//! 인스턴스에 워크스페이스를 하나 만든다(`workspace.create` 1회).
//!
//! [`crate::remote_browse`] 와 형제 모듈이다. 그쪽이 "client 측 순수 조회" 인 반면
//! 이 모듈은 **원격 상태를 바꾸는 유일한 client 능력**이라, 조회/변경을 모듈 경계로
//! 갈라 둔다(browse 라는 이름 아래 mutate 를 숨기지 않는다).
//!
//! CLI(`tasty remote new-workspace`)와 로컬 IPC(`remote.attach` 의 생성 옵션)가
//! **같은 함수를 공유**한다(원칙 2 — 에이전트가 CLI 없이 소켓만으로도 생성 가능).
//!
//! 원칙 1 은 양쪽 끝에서 모두 유지된다:
//! - 원격측: `workspace.create` 는 IPC = Agent origin 이라 원격의 active workspace 를
//!   바꾸지 않는다(`src/adapters/ipc/handler/workspace.rs` 의 cascade 분기).
//! - 로컬측: 이 모듈은 로컬 상태에 아예 닿지 않는다(원격으로 나가는 client 로직).
//!
//! 블로킹 I/O(SSH 터널 수립·소켓 read)를 하므로 **이벤트루프에서 직접 호출하면 안 된다**
//! — 호스트 IPC 경로는 워커 스레드에서 호출한다(`src/app/ipc/app_methods.rs`).

use anyhow::{Context, Result};

use crate::remote_browse::{probe_method, resolve_endpoint};
use crate::ssh::SshTarget;

/// 원격에 갓 만들어진 워크스페이스 — `workspace.create` 응답에서 이번 스코프에
/// 의미 있는 필드만 추린다(그 id 를 attach 대상으로 그대로 넘길 수 있다).
#[derive(Debug, Clone, serde::Serialize)]
pub struct CreatedRemoteWorkspace {
    pub id: u32,
    pub name: String,
    pub index: u32,
    /// 생성과 함께 만들어진 첫 surface id (원격 기준).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<u32>,
}

/// 접속된 포트로 `workspace.create` 1회. 터널 수명은 호출자가 관리한다 — attach 를
/// 이어 붙이는 호스트 경로는 같은 터널을 mirror 세션에 그대로 실어 살려야 한다.
///
/// `name`/`cwd` 는 미지정 가능(원격 기본값). `cwd` 는 **원격에서** `is_dir()` 검증되며,
/// 없으면 원격이 `invalid_params("cwd does not exist: …")` 로 거절한다 — 그 메시지가
/// 그대로 호출자에게 전파된다(로컬에서 미리 판정하지 않는다. 원격 파일시스템이다).
pub fn create_via_port(
    port: u16,
    name: Option<&str>,
    cwd: Option<&str>,
) -> Result<CreatedRemoteWorkspace> {
    let mut params = serde_json::Map::new();
    if let Some(n) = name {
        params.insert("name".to_string(), serde_json::Value::from(n));
    }
    if let Some(c) = cwd {
        params.insert("cwd".to_string(), serde_json::Value::from(c));
    }
    let resp = probe_method(port, "workspace.create", serde_json::Value::Object(params))
        .context("원격 workspace.create 실패")?;
    let id = resp
        .get("id")
        .and_then(|v| v.as_u64())
        .context("원격 workspace.create 응답에 id 가 없습니다")? as u32;
    Ok(CreatedRemoteWorkspace {
        id,
        name: resp
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        index: resp.get("index").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        surface_id: resp
            .get("surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
    })
}

/// 전체 생성 경로: 엔드포인트(터널/loopback) 해석 → `workspace.create`. **블로킹**(SSH).
/// 터널은 이 함수 반환 시 Drop 된다(단발 생성) — attach 를 이어 붙이려면 호출자가
/// [`resolve_endpoint`] 를 직접 잡고 [`create_via_port`] 를 쓴다.
pub fn create(
    target: &SshTarget,
    remote_tasty: &str,
    port_mode: &str,
    port_file: Option<&str>,
    name: Option<&str>,
    cwd: Option<&str>,
) -> Result<CreatedRemoteWorkspace> {
    let (_tunnel, port) = resolve_endpoint(target, remote_tasty, port_mode, port_file)?;
    create_via_port(port, name, cwd)
}
