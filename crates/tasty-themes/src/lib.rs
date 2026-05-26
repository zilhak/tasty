//! Theme file loading, disk persistence, and partial-merge tooling.
//!
//! `tasty-core::theme` 는 **현재 적용된 값** 만 가진다. 이 crate 가 그 인스턴스를
//! 어떤 이벤트(부팅, 테마 변경, partial apply)에서 어떻게 mutate 할지 다룬다.
//!
//! 책임:
//! - `~/.tasty/themes/` 의 TOML 파일 스캔/로드
//! - 빌트인 mocha (const fallback + 임베드 텍스트), latte (임베드 텍스트만)
//! - mocha 누락/파싱 실패 시 자동 복구
//! - first-run 시 latte 풀어두기
//! - 테마 변경 흐름: 사용자 overrides 클리어 + base 누적
//! - 두 레이어(`theme_base ▷ theme_overrides`) 합쳐 `Theme` 인스턴스 빌드

mod file;
mod scan;
mod state;
mod store;

pub use file::{ParseError, ThemeFile};
pub use scan::{ThemeEntry, rescan, scan_themes};
pub use state::{apply_theme, install_global, resolve};
pub use store::{
    BUILTIN_LATTE_ID, BUILTIN_MOCHA_ID, ThemeStoreError, ensure_mocha_exists, first_run_init,
    rewrite_mocha_fallback, themes_dir,
};

/// Embedded built-in `mocha.toml` text. Written to disk on first run / after
/// detecting a missing or corrupt mocha file.
pub const MOCHA_TOML_TEXT: &str = include_str!("../themes/mocha.toml");

/// Embedded built-in `latte.toml` text. Written to disk only when the themes
/// directory is empty (first run). No automatic fallback otherwise.
pub const LATTE_TOML_TEXT: &str = include_str!("../themes/latte.toml");
