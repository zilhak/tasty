//! host가 제공하는 훅 실행과 native WebView 인터페이스.

pub mod hooks;
#[cfg(feature = "gui")]
pub mod webview;
