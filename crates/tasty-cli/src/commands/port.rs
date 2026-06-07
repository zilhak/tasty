//! `tasty port` — 현재 인스턴스의 IPC 포트를 stdout 으로 출력(attach/detach 단계 5).
//!
//! 원격 포트 발견을 **셸 비의존**으로 만들기 위한 캡슐화: `ssh <dest> tasty port`
//! 는 exe + 단순 인자라 원격 셸(cmd/PowerShell/bash) 종류와 무관하게 동작한다
//! (`type`/`cat`/`Get-Content` OS·셸 분기 회피). IPC 연결 없이 포트 파일만 읽으므로
//! `run.rs` 에서 IPC 연결 전에 로컬 분기로 처리한다(`plugin logs` 선례와 동일).

use anyhow::Result;
use tasty_ipc::port_file as pf;

/// 포트 파일(`~/.tasty/tasty.port`, debug 는 `tasty-debug.port`)의 포트를 stdout 출력.
/// `--port-file` 로 경로를 격리할 수 있어 검증 시 격리 데몬 포트도 동일 경로로 조회된다.
pub fn run_port(port_file: Option<&str>) -> Result<()> {
    let port = pf::read_port_file_from(port_file)?;
    println!("{port}");
    Ok(())
}
