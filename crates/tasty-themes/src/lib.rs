#![forbid(unsafe_code)]

//! Theme file loading, disk persistence, partial-merge, 전역 인스턴스.
//!
//! `tasty-type-appearance::theme` 는 schema (Theme/ThemeColors/PartialColors/
//! ThemeSizing/SurfaceTheme) 와 인스턴스 메서드만 갖는다. 이 crate 는 그 위에
//! 도메인/IO 책임을 얹는다.
//!
//! 책임:
//! - 빌트인 Catppuccin Mocha fallback (`mocha_fallback_colors()`, `mocha_fallback()`)
//! - 전역 `Theme` 인스턴스 (`theme()` / `set_theme()` / `mutate_theme()`)
//! - `~/.tasty/themes/` 의 TOML 파일 스캔/로드
//! - mocha 누락/파싱 실패 시 자동 복구
//! - first-run 시 latte 풀어두기
//! - 테마 변경 흐름 (`apply_theme`): 사용자 overrides 클리어 + base 누적
//! - 두 레이어 합쳐 `Theme` 인스턴스 빌드 (`resolve`)
//! - `ThemeApplyContext` trait — settings 가 구현해서 어댑터 역할

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

// 공개 표면 — 외부 사용자(본 바이너리, settings) 는 여기 재수출만 본다.

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

/// type-appearance 의 theme schema 를 themes 경로로도 재수출.
/// 본 바이너리의 `crate::theme::*` 호출처가 schema 와 IO 양쪽을 한 모듈에서 보는
/// 기존 사용 패턴을 그대로 유지한다.
pub use tasty_type_appearance::theme::{
    PartialColors, PartialSurfaceTheme, SIZING, SurfaceTheme, Theme, ThemeColors, ThemeSizing,
};

/// Embedded built-in `mocha.toml` text. Written to disk on first run / after
/// detecting a missing or corrupt mocha file.
pub const MOCHA_TOML_TEXT: &str = include_str!("../themes/mocha.toml");

/// Embedded built-in `latte.toml` text. Seeded on first run (empty themes dir)
/// and re-synced by `sync_builtin_themes()` when the file is present. Not
/// recreated if the user deleted it (deletion is respected).
pub const LATTE_TOML_TEXT: &str = include_str!("../themes/latte.toml");
