// SurfaceColors 의 기본값(`terminal_default` 등) 이 mocha 색을 직접 정의한다 —
// 이는 theme 색 정의의 일부이므로 정당한 사용. 외부 호출자에게 차단되는
// `HexColor::from_rgb` 가 여기서는 의도된 사용.
#![allow(clippy::disallowed_methods)]

//! Color types shared between theme and settings.
//!
//! `HexColor` 와 GPU 색 newtype (`GpuRgba`, `GpuRgb`) 은 별 leaf crate
//! (`tasty-type-appearance`) 에서 정의되며 여기서 재수출된다. 호출자는
//! `tasty_core::color::HexColor` 또는 `tasty_type_appearance::color::HexColor`
//! 어느 쪽으로도 import 가능.
//!
//! `SurfaceColors` 는 surface 종류별 focused/unfocused 배경·전경 색상 묶음 —
//! 도메인 의미가 있어 tasty-core 에 남는다.

use serde::{Deserialize, Serialize};

// Appearance primitive 재수출. 호출자가 tasty_core::color 한 경로로 모든 색 타입에 접근.
pub use tasty_type_appearance::color::{GpuRgb, GpuRgba, HexColor};

/// Per-surface-type color settings for focused / unfocused states.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceColors {
    pub focused_bg: HexColor,
    pub focused_fg: HexColor,
    pub unfocused_bg: HexColor,
    pub unfocused_fg: HexColor,
}

impl SurfaceColors {
    /// Terminal defaults: Catppuccin Mocha base/text.
    pub fn terminal_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),         // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(30, 30, 46),    // #1e1e2e
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }

    /// Markdown defaults.
    pub fn markdown_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),         // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(24, 24, 37),    // #181825
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }

    /// Explorer defaults.
    pub fn explorer_default() -> Self {
        Self {
            focused_bg: HexColor::from_rgb(0, 0, 0),         // #000000
            focused_fg: HexColor::from_rgb(205, 214, 244),   // #cdd6f4
            unfocused_bg: HexColor::from_rgb(24, 24, 37),    // #181825
            unfocused_fg: HexColor::from_rgb(166, 173, 200), // #a6adc8
        }
    }
}

impl Default for SurfaceColors {
    fn default() -> Self {
        Self::terminal_default()
    }
}
