// 빌트인 mocha 색 정의. theme 색 디자인의 정당한 출처 중 하나이므로
// `HexColor::from_rgb` / `from_rgba` 사용이 정당. lint 예외.
#![allow(clippy::disallowed_methods)]

//! 빌트인 Catppuccin Mocha fallback.
//!
//! `~/.tasty/themes/mocha.toml` 로드에 실패해도 이 함수의 결과가 마지막 보루로 적용된다.
//! `MOCHA_TOML_TEXT` (lib.rs) 와 시각적으로 동일해야 하며, `tests` 에서 강제한다.
//!
//! `BTreeMap` 을 들고 있는 ThemeColors 가 더 이상 const fn 으로 빌드 불가능해서
//! const 값 대신 함수로 노출한다. 호출 비용은 µs 단위 — 부팅 시 1회 + 테마 fallback 케이스.

use std::collections::BTreeMap;

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::{SurfaceTheme, Theme, ThemeColors};

/// 최후의 fallback 색상 세트. `tasty-themes` 가 mocha.toml 로드에 실패하면 이걸 쓴다.
pub fn mocha_fallback_colors() -> ThemeColors {
    let mut surface_themes = BTreeMap::new();
    surface_themes.insert("terminal".to_string(), terminal_surface());
    surface_themes.insert("markdown".to_string(), markdown_surface());

    ThemeColors {
        // Surfaces
        crust: HexColor::from_rgb(0x11, 0x11, 0x1b),
        mantle: HexColor::from_rgb(0x18, 0x18, 0x25),
        base: HexColor::from_rgb(0x1e, 0x1e, 0x2e),
        surface0: HexColor::from_rgb(0x31, 0x32, 0x44),
        surface1: HexColor::from_rgb(0x45, 0x47, 0x5a),
        surface2: HexColor::from_rgb(0x58, 0x5b, 0x70),
        // Overlays
        overlay0: HexColor::from_rgb(0x6c, 0x70, 0x86),
        overlay1: HexColor::from_rgb(0x7f, 0x84, 0x9c),
        overlay2: HexColor::from_rgb(0x93, 0x99, 0xb2),
        // Text
        text: HexColor::from_rgb(0xcd, 0xd6, 0xf4),
        subtext1: HexColor::from_rgb(0xba, 0xc2, 0xde),
        subtext0: HexColor::from_rgb(0xa6, 0xad, 0xc8),
        placeholder: HexColor::from_rgb(0x6c, 0x70, 0x86), // = overlay0
        // Accent
        blue: HexColor::from_rgb(0x89, 0xb4, 0xfa),
        green: HexColor::from_rgb(0xa6, 0xe3, 0xa1),
        red: HexColor::from_rgb(0xf3, 0x8b, 0xa8),
        yellow: HexColor::from_rgb(0xf9, 0xe2, 0xaf),
        peach: HexColor::from_rgb(0xfa, 0xb3, 0x87),
        mauve: HexColor::from_rgb(0xcb, 0xa6, 0xf7),
        teal: HexColor::from_rgb(0x94, 0xe2, 0xd5),
        sky: HexColor::from_rgb(0x89, 0xdc, 0xeb),
        lavender: HexColor::from_rgb(0xb4, 0xbe, 0xfe),
        flamingo: HexColor::from_rgb(0xf2, 0xcd, 0xcd),
        pink: HexColor::from_rgb(0xf5, 0xc2, 0xe7),
        maroon: HexColor::from_rgb(0xeb, 0xa0, 0xac),
        rosewater: HexColor::from_rgb(0xf5, 0xe0, 0xdc),
        // Terminal-specific
        selection_bg: HexColor::from_rgb(0x58, 0x5b, 0x70), // = surface2
        search_match_bg: HexColor::from_rgba(0xf9, 0xe2, 0xaf, 0x4d), // yellow @ ~30%
        search_match_active_bg: HexColor::from_rgba(0xf9, 0xe2, 0xaf, 0xb3), // yellow @ ~70%
        // ANSI 16
        ansi_black: HexColor::from_rgb(0x45, 0x47, 0x5a), // surface1
        ansi_red: HexColor::from_rgb(0xf3, 0x8b, 0xa8),
        ansi_green: HexColor::from_rgb(0xa6, 0xe3, 0xa1),
        ansi_yellow: HexColor::from_rgb(0xf9, 0xe2, 0xaf),
        ansi_blue: HexColor::from_rgb(0x89, 0xb4, 0xfa),
        ansi_magenta: HexColor::from_rgb(0xcb, 0xa6, 0xf7),
        ansi_cyan: HexColor::from_rgb(0x94, 0xe2, 0xd5),
        ansi_white: HexColor::from_rgb(0xba, 0xc2, 0xde), // subtext1
        ansi_bright_black: HexColor::from_rgb(0x6c, 0x70, 0x86), // overlay0
        ansi_bright_red: HexColor::from_rgb(0xf3, 0x8b, 0xa8),
        ansi_bright_green: HexColor::from_rgb(0xa6, 0xe3, 0xa1),
        ansi_bright_yellow: HexColor::from_rgb(0xf9, 0xe2, 0xaf),
        ansi_bright_blue: HexColor::from_rgb(0x89, 0xb4, 0xfa),
        ansi_bright_magenta: HexColor::from_rgb(0xcb, 0xa6, 0xf7),
        ansi_bright_cyan: HexColor::from_rgb(0x89, 0xdc, 0xeb), // sky
        ansi_bright_white: HexColor::from_rgb(0xcd, 0xd6, 0xf4), // text
        surface_themes,
    }
}

/// 최후의 fallback `Theme` 인스턴스. 전역 RwLock 초기값을 LazyLock 으로 빌드할 때 사용.
pub fn mocha_fallback() -> Theme {
    Theme::with_colors(mocha_fallback_colors(), false)
}

/// 빌트인 terminal SurfaceTheme. 검은 배경 + Mocha text/subtext.
fn terminal_surface() -> SurfaceTheme {
    SurfaceTheme {
        focused_bg: HexColor::from_rgb(0, 0, 0),            // #000000
        focused_fg: HexColor::from_rgb(0xcd, 0xd6, 0xf4),   // text
        unfocused_bg: HexColor::from_rgb(0x1e, 0x1e, 0x2e), // base
        unfocused_fg: HexColor::from_rgb(0xa6, 0xad, 0xc8), // subtext0
    }
}

/// 빌트인 markdown SurfaceTheme. 검은 배경 + Mocha text/subtext.
/// unfocused 가 mantle 인 게 terminal 과 다름 — markdown 은 한 단계 더 어두운 톤.
fn markdown_surface() -> SurfaceTheme {
    SurfaceTheme {
        focused_bg: HexColor::from_rgb(0, 0, 0),            // #000000
        focused_fg: HexColor::from_rgb(0xcd, 0xd6, 0xf4),   // text
        unfocused_bg: HexColor::from_rgb(0x18, 0x18, 0x25), // mantle
        unfocused_fg: HexColor::from_rgb(0xa6, 0xad, 0xc8), // subtext0
    }
}
