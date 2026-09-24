//! 로컬 loopback attach는 사용자 mirror 조작을 재현하므로 debug 빌드에서만 제공한다.
//! local::attach의 공용 세션 처리에 로컬 포트를 넘긴다. 원격은 tasty remote attach를 쓴다.

#![cfg(debug_assertions)]

use anyhow::Result;

use crate::local::attach::{run_attach_on_port, run_attach_workspace_on_port};

/// `tasty debug attach <surface>` (로컬 loopback) 진입점. force-detach 는 별도(JSON-RPC).
pub fn run_attach(
    surface: u32,
    dump_after: Option<u64>,
    send: Option<&str>,
    raw: bool,
    port_file: Option<&str>,
) -> Result<()> {
    let port = crate::port_file::read_port(port_file)?;
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
    let port = crate::port_file::read_port(port_file)?;
    run_attach_workspace_on_port(port, workspace, dump_after, send, send_to)?;
    Ok(())
}
