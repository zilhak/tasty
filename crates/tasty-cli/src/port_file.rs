//! 포트 파일 조회의 **사용자 문구 소유 지점**.
//!
//! `tasty-ipc` 는 전송 계층이라 문구를 갖지 않고 실패 조건만 타입
//! ([`PortFileError`])으로 돌려준다 — 그 조건을 현재 로케일의 문장으로 바꾸는 곳이
//! 여기 한 곳이다([`docs/dev-guide/i18n.md`] "호출자 주입", ADR-0106 결정 4).
//! CLI 의 모든 포트 파일 조회는 이 함수를 거친다: 조회 지점마다 문구를 만들면
//! 같은 실패가 명령에 따라 다른 문장으로 나온다.
//!
//! 인스턴스 미실행은 사용자가 가장 먼저 마주치는 실패라, 여기서 만든 문장이
//! `tasty` 를 처음 쓰는 사람이 보는 첫 안내가 된다.
//!
//! [`docs/dev-guide/i18n.md`]: ../../../../docs/dev-guide/i18n.md

use anyhow::Result;
use tasty_i18n::{t, t_fmt};
use tasty_ipc::port_file::{self as pf, PortFileError};

/// 포트 파일에서 IPC 포트를 읽는다. 실패 문구는 `general.language` 를 따른다.
///
/// 반환은 단일 메시지 `anyhow::Error` 다 — `context` 로 원인을 체인하면 최상위
/// 출력이 `Caused by:` 블록까지 붙어 기존 한 줄 출력과 달라진다.
pub fn read_port(port_file: Option<&str>) -> Result<u16> {
    read_port_diagnosed(port_file).map_err(|e| anyhow::anyhow!("{}", localize(&e)))
}

/// [`read_port`] 와 같은 조회지만 **번역 전의 에러 값**을 그대로 돌려준다.
///
/// `hook-failures.log` 에 실릴 문구는 로케일 무관 영어여야 하는데(`hook_failure` 모듈
/// 참고), `read_port` 는 번역문만 돌려주므로 그 자리에서는 원본에 닿을 수 없었다.
/// `PortFileError` 의 `Display` 가 곧 영어 렌더링이고, 아래 테스트가 그것이
/// `lang/en.toml` 값과 문자 단위로 같음을 강제한다 — 두 문구가 갈리지 않는다.
pub fn read_port_diagnosed(port_file: Option<&str>) -> std::result::Result<u16, PortFileError> {
    pf::read_port_file_from(port_file)
}

/// 실패 조건 → 현재 로케일 문장. en 값은 [`PortFileError`] 의 기본 렌더링과
/// 문자 단위로 같아야 한다(아래 테스트가 강제).
pub(crate) fn localize(err: &PortFileError) -> String {
    match err {
        PortFileError::HomeUnresolved => t("cli.port_file.home_unresolved").to_string(),
        PortFileError::NotFound { path } => {
            t_fmt("cli.port_file.no_instance", &path.display().to_string())
        }
        PortFileError::Invalid => t("cli.port_file.invalid_contents").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// en 로케일에서는 번역을 거친 문구가 크레이트의 영어 기본 렌더링과 **같아야**
    /// 한다 — 두 값이 갈라지면 같은 실패가 경로에 따라 다른 영어 문장으로 나온다.
    /// lang 값을 손대면 여기서 먼저 걸린다.
    #[test]
    fn english_lang_values_match_the_crate_default_rendering() {
        tasty_i18n::init("en");

        let path = std::path::PathBuf::from("/home/u/.tasty/tasty.port");
        assert_eq!(
            localize(&PortFileError::NotFound { path: path.clone() }),
            PortFileError::NotFound { path }.to_string()
        );
        assert_eq!(
            localize(&PortFileError::HomeUnresolved),
            PortFileError::HomeUnresolved.to_string()
        );
        assert_eq!(
            localize(&PortFileError::Invalid),
            PortFileError::Invalid.to_string()
        );
    }

    /// 경로는 문구 안에 그대로 들어간다 — `TASTY_HOME` 으로 홈이 갈렸을 때
    /// 사용자가 대조하는 값이라 치환 자리가 비면 안내가 무의미해진다.
    #[test]
    fn the_searched_path_is_substituted_into_the_message() {
        tasty_i18n::init("en");

        let msg = localize(&PortFileError::NotFound {
            path: std::path::PathBuf::from("/isolated/home/tasty.port"),
        });
        assert!(msg.contains("/isolated/home/tasty.port"), "{msg}");
        assert!(!msg.contains("{}"), "치환 자리가 남았다: {msg}");
    }
}
