//! `tasty port` — 현재 인스턴스의 IPC 포트를 stdout 으로 출력(attach/detach 단계 5).
//!
//! 원격 포트 발견 auto 체인의 **첫 단계**용 서브커맨드: `ssh <dest> tasty port`
//! 는 exe + 단순 인자라 git bash·unix 원격에서 OS·셸 분기 없이 동작한다
//! (`type`/`cat`/`Get-Content` 회피). 단, Windows GUI release 바이너리는 비-PTY
//! 세션의 PowerShell/cmd 에서 stdout 이 SSH 채널로 결선되지 않아 빈 출력으로 조용히
//! 실패한다 — 그래서 셸 비의존성은 이 단계 단독이 아니라 file 모드 fallback 까지
//! 포함한 auto 체인 전체(subcommand→file-unix→file-windows)가 달성한다
//! ([`crate::ssh::discover_remote_port`]). IPC 연결 없이 포트 파일만 읽으므로
//! `run.rs` 에서 IPC 연결 전에 로컬 분기로 처리한다(`plugin logs` 선례와 동일).

use anyhow::Result;
use tasty_ipc::port_file as pf;

use crate::out::outln;

/// 포트 파일(`~/.tasty/tasty.port`, debug 는 `tasty-debug.port`)의 포트를 stdout 출력.
/// `--port-file` 로 경로를 격리할 수 있어 검증 시 격리 데몬 포트도 동일 경로로 조회된다.
pub fn run_port(port_file: Option<&str>) -> Result<()> {
    let port = pf::read_port_file_from(port_file)?;
    outln!("{port}")?;
    Ok(())
}
