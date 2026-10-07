//! 데이터 루트의 webhooks.toml에 저장한 명시 지정 포트. 파일 규칙(형식 표시, 이전 자동 기록 포트 정리,
//! 다른 키 보존)은 인스턴스가 없을 때의 CLI와 함께 쓰도록 `tasty_settings::webhook_port_file`에 있다.

use std::path::PathBuf;

use tasty_settings::webhook_port_file as file;

pub use file::DEFAULT_PORT;

/// 데이터 루트가 없으면 임시 디렉터리의 공유 경로를 사용한다.
pub fn config_path() -> PathBuf {
    file::path()
}

/// 사용자가 명시로 정한 포트(`tasty webhook port <N>`). 없거나 범위 밖이면 None이다.
/// 다음 실행에 쓸 값이라 실행 중인 리스너와 다를 수 있다.
pub fn read_port() -> Option<u16> {
    file::read_port(&config_path())
}

/// port를 저장한다. 리스너에는 다음 실행부터 적용된다.
pub fn set_port(port: u16) -> std::io::Result<()> {
    file::set_port(&config_path(), port)
}

/// 저장한 port를 지운다.
pub fn clear_port() -> std::io::Result<()> {
    file::clear_port(&config_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 공유 락과 TestHome으로 실제 설정 경로를 시험용 홈에 격리한다.
    use crate::test_support::TastyHomeGuard as HomeGuard;

    /// 파일 규칙 자체는 `tasty_settings::webhook_port_file`의 시험이 본다. 여기서는 데이터 루트 경로로 이어지는지만 본다.
    #[test]
    fn the_wrappers_use_the_data_root_file() {
        let _home = HomeGuard::new();
        assert_eq!(read_port(), None);
        set_port(40000).unwrap();
        assert!(config_path().exists());
        assert_eq!(read_port(), Some(40000));
        clear_port().unwrap();
        assert_eq!(read_port(), None);
    }
}
