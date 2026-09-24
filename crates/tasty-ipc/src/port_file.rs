//! 데이터 루트의 tasty.port 조회와 저장. 기본 루트는 debug/release에 따라 구분한다.

use std::path::{Path, PathBuf};

use anyhow::Result;

use tasty_utils::path::tasty_home;

/// 포트 조회 실패 조건. 소비자가 번역하며 Display의 기본 영어는 CLI 카탈로그와 맞춘다.
#[derive(Debug, thiserror::Error)]
pub enum PortFileError {
    /// tasty home 을 확정하지 못해 포트 파일 경로 자체를 만들 수 없다.
    #[error("Could not determine config directory")]
    HomeUnresolved,
    /// 지정한 포트 파일이 없다. 인스턴스 미실행이나 다른 데이터 루트를 확인한다.
    #[error("No running tasty instance found (port file not found at {})", .path.display())]
    NotFound {
        /// 실제로 찾아본 경로. 홈이 갈릴 때(`TASTY_HOME`) 사용자가 대조하는 값이라
        /// 문구를 만드는 쪽에 그대로 넘긴다.
        path: PathBuf,
    },
    /// 파일은 있으나 포트 번호로 읽히지 않는다.
    #[error("Invalid port file contents")]
    Invalid,
}

/// Port file 경로 (`tasty_home()/tasty.port`).
///
/// debug/release 격리는 루트(`tasty_home()`)가 담당한다 — debug 빌드는
/// `~/.tasty-debug/tasty.port`, release 는 `~/.tasty/tasty.port`. 루트가 갈리므로
/// 파일명 접미사(`-debug`)는 두지 않는다.
pub fn port_file_path() -> Option<PathBuf> {
    tasty_home().map(|dir| dir.join("tasty.port"))
}

/// 기본 port file 에서 포트 읽기 — CLI 클라이언트가 사용.
pub fn read_port_file() -> Result<u16, PortFileError> {
    read_port_file_from(None)
}

/// 지정 경로 (또는 기본) 의 port file 에서 포트 읽기.
pub fn read_port_file_from(port_file: Option<&str>) -> Result<u16, PortFileError> {
    let path = match port_file {
        Some(p) => PathBuf::from(p),
        None => port_file_path().ok_or(PortFileError::HomeUnresolved)?,
    };
    let contents = std::fs::read_to_string(&path)
        .map_err(|_| PortFileError::NotFound { path: path.clone() })?;
    contents
        .trim()
        .parse::<u16>()
        .map_err(|_| PortFileError::Invalid)
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
