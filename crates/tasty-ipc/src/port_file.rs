//! Port file (`~/.tasty/tasty.port`) 위치 + read/write 헬퍼.
//!
//! Server 인스턴스 없는 컨텍스트 (CLI 클라이언트 등) 가 사용 — free fn.
//! 서버 측은 `TcpIpcServer::start_with_port_file` 안에서 `write_port_file_to`
//! 를 직접 호출 + Drop 시 `effective_port_file_path()` 로 삭제.

use std::path::{Path, PathBuf};

use anyhow::Result;

use tasty_utils::path::tasty_home;

/// 포트 파일 조회 실패 — **조건만** 알리고 표시 문구는 소비자가 고른다.
///
/// 이 크레이트는 wire framing/전송 계층이라 `tasty-i18n` 을 의존하지 않는다.
/// 사용자에게 보일 문구는 그 실패를 화면에 내보내는 소비자(CLI)가 자기 로케일로
/// 만든다 — leaf 크레이트가 문구를 소유하지 않는다는 원칙의 적용이다
/// (`docs/adr/0106-non-widget-user-strings-go-through-i18n.md` 결정 4,
/// `docs/dev-guide/i18n.md` "호출자 주입").
///
/// 아래 `Display` 는 그 경로를 타지 않는 소비자를 위한 **영어 기본 렌더링**이며
/// `lang/en.toml` 의 대응 키와 문자 단위로 같아야 한다. 정합은 문구를 소유하는
/// 쪽(`tasty_cli::port_file`)의 테스트가 강제한다.
#[derive(Debug, thiserror::Error)]
pub enum PortFileError {
    /// tasty home 을 확정하지 못해 포트 파일 경로 자체를 만들 수 없다.
    #[error("Could not determine config directory")]
    HomeUnresolved,
    /// 포트 파일이 없다 — 실행 중인 인스턴스가 없다는 뜻이다(가장 흔한 원인).
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
