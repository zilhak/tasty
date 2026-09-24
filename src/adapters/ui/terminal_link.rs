//! 터미널 링크 열기. 검출은 tasty-terminal-link에서 수행하며
//! OS 브라우저 의존이 필요한 열기 동작은 gui 기능 뒤에 둔다.

pub use tasty_terminal_link::*;

/// URI를 기본 브라우저/연결 프로그램으로 연다. 크로스 플랫폼.
/// 성공하면 true, 실패하면 `tracing::warn!`을 남기고 false.
pub fn open_uri(uri: &str) -> bool {
    #[cfg(debug_assertions)]
    if crate::platform::debug_os_open::intercepted("open_uri", uri) {
        return true;
    }
    match webbrowser::open(uri) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("failed to open link {:?}: {}", uri, e);
            false
        }
    }
}
