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

/// 사용자 조작으로 시작한 OS 열기의 실패 안내 문구. 성공하면 None이라 토스트를 더하지 않는다.
/// `key`는 `{}` 하나를 가진 번역 키이고, 대상은 상한에 맞게 가운데를 줄여 넣는다.
pub(crate) fn open_failure_message(opened: bool, key: &str, target: &str) -> Option<String> {
    (!opened).then(|| crate::i18n::t_fmt_fit(key, target))
}

#[cfg(test)]
mod open_failure_tests {
    use super::open_failure_message;
    use crate::adapters::ui::{ToastKind, ToastManager, ToastScope};

    fn toasts_after(opened: bool) -> usize {
        let mut toasts = ToastManager::new();
        if let Some(message) =
            open_failure_message(opened, "toast.open_link_failed", "https://example.com")
        {
            toasts.push(message, ToastKind::Error, ToastScope::Window);
        }
        toasts.len()
    }

    #[test]
    fn a_failed_open_adds_one_toast_and_a_successful_open_adds_none() {
        assert_eq!(toasts_after(false), 1);
        assert_eq!(toasts_after(true), 0);
    }

    #[test]
    fn the_failure_message_names_the_target() {
        let message = open_failure_message(false, "toast.open_link_failed", "https://example.com")
            .expect("실패는 문구가 있다");
        assert!(message.contains("https://example.com"), "{message}");
        assert_ne!(message, "toast.open_link_failed", "번역 키가 그대로 나왔다");
    }
}
