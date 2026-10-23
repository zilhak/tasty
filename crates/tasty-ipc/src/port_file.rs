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

/// Completed lifecycle publication. This is not a queued save receipt.
#[derive(Debug)]
pub struct PortFilePublication {
    path: PathBuf,
    target: PathBuf,
    fingerprint: tasty_utils::file_replace::Fingerprint,
}

impl PortFilePublication {
    /// Best-effort teardown of this publication only. Never waits for a participant lock.
    /// External writers must serialize startup; a nonparticipant can race the final check/unlink.
    pub fn cleanup_if_owned(&self) -> std::io::Result<bool> {
        use tasty_utils::file_replace::{self, Fingerprint};
        if file_replace::resolve_link(&self.path)? != self.target {
            return Ok(false);
        }
        match std::fs::metadata(&self.path) {
            Ok(meta) if !meta.is_file() => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
            Ok(_) => {}
        }
        if Fingerprint::read(&self.path)? != self.fingerprint {
            return Ok(false);
        }
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Publish only after a complete atomic replacement; unreadable originals are preserved.
/// New Unix files are 0600. Existing metadata and live symlinks use the common replacement policy.
pub fn publish_port_file_to(port: u16, custom_path: Option<&Path>) -> Result<PortFilePublication> {
    use tasty_utils::file_replace::{self, Fingerprint};
    let path = custom_path
        .map(Path::to_path_buf)
        .or_else(port_file_path)
        .ok_or(PortFileError::HomeUnresolved)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let target = file_replace::resolve_link(&path)?;
    match std::fs::metadata(&target) {
        Ok(meta) if !meta.is_file() => anyhow::bail!("port file is not a regular file"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let observed = Fingerprint::read(&target)?;
    publish_observed(port, path, target, observed)
}

fn publish_observed(
    port: u16,
    path: PathBuf,
    target: PathBuf,
    observed: tasty_utils::file_replace::Fingerprint,
) -> Result<PortFilePublication> {
    use tasty_utils::file_replace::{self, Fingerprint, Options};
    let _lock = file_replace::lock(&path)?;
    if file_replace::resolve_link(&path)? != target || Fingerprint::read(&target)? != observed {
        anyhow::bail!("port file changed while preparing publication");
    }
    let bytes = port.to_string();
    file_replace::replace_with(
        &path,
        bytes.as_bytes(),
        Options {
            new_mode: Some(0o600),
            sync: false,
        },
    )?;
    tracing::info!(path = %path.display(), "published port file");
    Ok(PortFilePublication {
        path,
        target,
        fingerprint: Fingerprint::of_bytes(bytes.as_bytes()),
    })
}

/// Blocking startup barrier. Returning Ok means the complete port is observable by discovery.
pub fn write_port_file_to(port: u16, custom_path: Option<&Path>) -> Result<()> {
    publish_port_file_to(port, custom_path).map(|_| ())
}

#[cfg(test)]
#[path = "port_file/tests.rs"]
mod tests;
