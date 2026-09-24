//! 원격 포트 발견의 첫 단계인 tasty port를 처리한다. IPC 연결 없이 포트 파일을 읽는다.
//! Windows GUI release의 비-PTY PowerShell/cmd에서는 stdout을 받지 못할 수 있어
//! auto 발견은 file-unix/file-windows fallback도 사용한다(crate::ssh::discover_remote_port).

use anyhow::Result;

use crate::out::outln;

/// tasty.port의 포트를 출력한다. 기본 루트는 release ~/.tasty/, debug ~/.tasty-debug/다.
/// --port-file로 별도 파일을 지정할 수 있다.
pub fn run_port(port_file: Option<&str>) -> Result<()> {
    let port = crate::port_file::read_port(port_file)?;
    outln!("{port}")?;
    Ok(())
}
