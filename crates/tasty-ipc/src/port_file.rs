//! Port file (`~/.tasty/tasty.port`) 위치 + read/write 헬퍼.
//!
//! Server 인스턴스 없는 컨텍스트 (CLI 클라이언트 등) 가 사용 — free fn.
//! 서버 측은 `TcpIpcServer::start_with_port_file` 안에서 `write_port_file_to`
//! 를 직접 호출 + Drop 시 `effective_port_file_path()` 로 삭제.

use std::path::{Path, PathBuf};

use anyhow::Result;

use tasty_utils::path::tasty_home;

/// Port file 경로 (`tasty_home()/tasty.port`).
///
/// debug/release 격리는 루트(`tasty_home()`)가 담당한다 — debug 빌드는
/// `~/.tasty-debug/tasty.port`, release 는 `~/.tasty/tasty.port`. 루트가 갈리므로
/// 파일명 접미사(`-debug`)는 두지 않는다.
pub fn port_file_path() -> Option<PathBuf> {
    tasty_home().map(|dir| dir.join("tasty.port"))
}

/// 기본 port file 에서 포트 읽기 — CLI 클라이언트가 사용.
pub fn read_port_file() -> Result<u16> {
    read_port_file_from(None)
}

/// 지정 경로 (또는 기본) 의 port file 에서 포트 읽기.
pub fn read_port_file_from(port_file: Option<&str>) -> Result<u16> {
    let path = match port_file {
        Some(p) => PathBuf::from(p),
        None => port_file_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?,
    };
    let contents = std::fs::read_to_string(&path).map_err(|_| {
        anyhow::anyhow!(
            "No running tasty instance found (port file not found at {})",
            path.display()
        )
    })?;
    contents
        .trim()
        .parse::<u16>()
        .map_err(|_| anyhow::anyhow!("Invalid port file contents"))
}

/// 포트를 파일에 쓴다. `custom_path` 지정 시 그 경로, 아니면 기본
/// (`port_file_path()`). 디렉터리는 부재 시 생성.
pub fn write_port_file_to(port: u16, custom_path: Option<&Path>) -> Result<()> {
    let path = match custom_path {
        Some(p) => p.to_path_buf(),
        None => match port_file_path() {
            Some(p) => p,
            None => return Ok(()),
        },
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, port.to_string())?;
    tracing::info!("Wrote port file: {}", path.display());
    Ok(())
}
