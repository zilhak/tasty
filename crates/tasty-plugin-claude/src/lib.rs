#![forbid(unsafe_code)]

//! Claude 플러그인의 공용 상수. 실행 진입점은 main.rs다.

/// Plugin manifest id — `tasty-plugin.toml` 의 `id` 와 일치해야 함.
/// host 측 코드가 `claude.spawn` 등을 호출할 때 plugin 식별자로 사용한다.
pub const PLUGIN_ID: &str = "com.tasty.claude";
