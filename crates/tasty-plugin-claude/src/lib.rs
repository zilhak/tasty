#![forbid(unsafe_code)]

//! Cross-crate constants for the Claude plugin. binary entry point 은 `main.rs`.

/// Plugin manifest id — `tasty-plugin.toml` 의 `id` 와 일치해야 함.
/// host 측 코드가 `claude.spawn` / `claude.wait` 을 동기 호출할 때 plugin
/// 식별자로 사용한다.
pub const PLUGIN_ID: &str = "com.tasty.claude";
