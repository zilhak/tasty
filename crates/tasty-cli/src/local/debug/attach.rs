//! `tasty debug attach <surface>` — 로컬 loopback attach client (debug 빌드 전용).
//!
//! 원칙 1 ②: 로컬 self(loopback) attach 는 *사용자가 직접 하는 mirror 조작* 을
//! 자동 재현하는 성격이라 release IPC/CLI 표면에 노출하지 않는다. 원격 attach 는
//! `tasty remote attach`(release). 이 모듈을 통째 삭제하고 router 분기 한 줄만
//! 정리하면 로컬 attach 가 깨끗이 사라진다(debug 격리 기준선).
//!
//! 실제 attach 세션 머신(`run_attach_on_port` / `run_attach_workspace_on_port` 와
//! 그 하위 mirror/raw/dump)은 `local/attach.rs` 에 로컬·원격 공용으로 보존된다 —
//! 이 모듈은 포트 파일을 읽어 그 공유 머신을 로컬 loopback 으로 호출하는 진입점일 뿐.

#![cfg(debug_assertions)]

use anyhow::Result;
use tasty_ipc::port_file as pf;

use crate::local::attach::{run_attach_on_port, run_attach_workspace_on_port};

/// `tasty debug attach <surface>` (로컬 loopback) 진입점. force-detach 는 별도(JSON-RPC).
pub fn run_attach(
    surface: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    raw: bool,
    port_file: Option<&str>,
) -> Result<()> {
    let port = pf::read_port_file_from(port_file)?;
    run_attach_on_port(port, surface, dump_after, send, raw)?;
    Ok(())
}

/// `tasty debug attach --workspace <id>` (로컬 loopback) 진입점.
pub fn run_attach_workspace(
    workspace: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    send_to: Option<u32>,
    port_file: Option<&str>,
) -> Result<()> {
    let port = pf::read_port_file_from(port_file)?;
    run_attach_workspace_on_port(port, workspace, dump_after, send, send_to)?;
    Ok(())
}
