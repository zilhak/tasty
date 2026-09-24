#![forbid(unsafe_code)]

//! 테마 파일 읽기·저장·병합과 전역 인스턴스를 관리한다.
//! 타입과 색 계산은 tasty-type-appearance에 두고 이 크레이트에서 파일 입출력을 처리한다.
//! 파일 위치는 Tasty 홈의 themes 디렉터리이며 내장 테마 복구 정책은 store 모듈을 따른다.

mod apply_context;
mod fallback;
mod file;
mod global;
mod plugin_defaults;
mod port;
mod scan;
mod state;
mod store;
mod store_instance;

pub mod testing;

pub use apply_context::ThemeApplyContext;
pub use fallback::{mocha_fallback, mocha_fallback_colors};
pub use file::{ParseError, ThemeFile};
pub use global::{mutate_theme, set_theme, theme};
pub use plugin_defaults::{
    add_plugin_surface_default, apply_plugin_defaults_to, record_user_defined_surface_kinds,
};
pub use port::ThemeStorage;
pub use scan::{ThemeEntry, rescan, scan_themes};
pub use state::{
    ThemeRuntime, apply_theme, install_global, install_global_with_runtime, resolve,
    resolve_with_runtime,
};
pub use store::{
    BUILTIN_LATTE_ID, BUILTIN_MOCHA_ID, ThemeStoreError, ensure_mocha_exists, first_run_init,
    rewrite_mocha_fallback, sync_builtin_themes, themes_dir,
};
pub use store_instance::ThemeStore;

/// 호출자가 테마 타입과 파일 입출력을 같은 경로에서 사용할 수 있도록 다시 공개한다.
pub use tasty_type_appearance::theme::{
    PartialColors, PartialSurfaceTheme, SIZING, SurfaceTheme, Theme, ThemeColors, ThemeSizing,
};

/// Embedded built-in `mocha.toml` text. Written to disk on first run / after
/// detecting a missing or corrupt mocha file.
pub const MOCHA_TOML_TEXT: &str = include_str!("../themes/mocha.toml");

/// Embedded built-in `latte.toml` text. Seeded when no TOML files exist
/// and re-synced by `sync_builtin_themes()` when the file is present. Not
/// recreated if the user deleted it (deletion is respected).
pub const LATTE_TOML_TEXT: &str = include_str!("../themes/latte.toml");
