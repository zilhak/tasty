//! TOML 테마 파일. 일부 필드만 지정하면 나머지는 기존 색상을 유지한다.

use std::collections::BTreeMap;

use serde::Deserialize;
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::{PartialColors, PartialSurfaceTheme};
use thiserror::Error;

/// TOML 파일 표현. 모든 sub-table 과 필드 optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThemeFile {
    /// 사용자에게 보여줄 이름. 없으면 파일명 stem 으로 fallback.
    pub label: Option<String>,
    /// 라이트/다크 플래그. None 이면 이전 상태의 is_light 를 유지한다.
    pub is_light: Option<bool>,

    #[serde(default)]
    pub palette: PaletteSection,
    #[serde(default)]
    pub accent: AccentSection,
    #[serde(default)]
    pub terminal: TerminalSection,
    #[serde(default)]
    pub ansi: AnsiSection,
    /// 서피스 종류별 색상. 내장 종류 외의 ID도 읽어 플러그인 색상으로 사용할 수 있다.
    #[serde(default)]
    pub surfaces: BTreeMap<String, PartialSurfaceTheme>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PaletteSection {
    pub crust: Option<HexColor>,
    pub mantle: Option<HexColor>,
    pub base: Option<HexColor>,
    pub surface0: Option<HexColor>,
    pub surface1: Option<HexColor>,
    pub surface2: Option<HexColor>,
    pub overlay0: Option<HexColor>,
    pub overlay1: Option<HexColor>,
    pub overlay2: Option<HexColor>,
    pub text: Option<HexColor>,
    pub subtext1: Option<HexColor>,
    pub subtext0: Option<HexColor>,
    pub placeholder: Option<HexColor>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AccentSection {
    pub blue: Option<HexColor>,
    pub green: Option<HexColor>,
    pub red: Option<HexColor>,
    pub yellow: Option<HexColor>,
    pub peach: Option<HexColor>,
    pub mauve: Option<HexColor>,
    pub teal: Option<HexColor>,
    pub sky: Option<HexColor>,
    pub lavender: Option<HexColor>,
    pub flamingo: Option<HexColor>,
    pub pink: Option<HexColor>,
    pub maroon: Option<HexColor>,
    pub rosewater: Option<HexColor>,
}

/// 터미널 전용 색상. 기본 글자·배경색은 surfaces.terminal에 있다.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TerminalSection {
    pub selection_bg: Option<HexColor>,
    pub vi_cursor_bg: Option<HexColor>,
    pub search_match_bg: Option<HexColor>,
    pub search_match_active_bg: Option<HexColor>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AnsiSection {
    pub black: Option<HexColor>,
    pub red: Option<HexColor>,
    pub green: Option<HexColor>,
    pub yellow: Option<HexColor>,
    pub blue: Option<HexColor>,
    pub magenta: Option<HexColor>,
    pub cyan: Option<HexColor>,
    pub white: Option<HexColor>,
    pub bright_black: Option<HexColor>,
    pub bright_red: Option<HexColor>,
    pub bright_green: Option<HexColor>,
    pub bright_yellow: Option<HexColor>,
    pub bright_blue: Option<HexColor>,
    pub bright_magenta: Option<HexColor>,
    pub bright_cyan: Option<HexColor>,
    pub bright_white: Option<HexColor>,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

impl ThemeFile {
    /// TOML 텍스트를 파싱.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let file: ThemeFile = toml::from_str(text)?;
        Ok(file)
    }

    /// 색상 필드를 PartialColors로 모으고 밝기 모드는 별도로 반환한다.
    pub fn to_partial(&self) -> (PartialColors, Option<bool>) {
        let p = PartialColors {
            crust: self.palette.crust,
            mantle: self.palette.mantle,
            base: self.palette.base,
            surface0: self.palette.surface0,
            surface1: self.palette.surface1,
            surface2: self.palette.surface2,
            overlay0: self.palette.overlay0,
            overlay1: self.palette.overlay1,
            overlay2: self.palette.overlay2,
            text: self.palette.text,
            subtext1: self.palette.subtext1,
            subtext0: self.palette.subtext0,
            placeholder: self.palette.placeholder,
            blue: self.accent.blue,
            green: self.accent.green,
            red: self.accent.red,
            yellow: self.accent.yellow,
            peach: self.accent.peach,
            mauve: self.accent.mauve,
            teal: self.accent.teal,
            sky: self.accent.sky,
            lavender: self.accent.lavender,
            flamingo: self.accent.flamingo,
            pink: self.accent.pink,
            maroon: self.accent.maroon,
            rosewater: self.accent.rosewater,
            selection_bg: self.terminal.selection_bg,
            vi_cursor_bg: self.terminal.vi_cursor_bg,
            search_match_bg: self.terminal.search_match_bg,
            search_match_active_bg: self.terminal.search_match_active_bg,
            ansi_black: self.ansi.black,
            ansi_red: self.ansi.red,
            ansi_green: self.ansi.green,
            ansi_yellow: self.ansi.yellow,
            ansi_blue: self.ansi.blue,
            ansi_magenta: self.ansi.magenta,
            ansi_cyan: self.ansi.cyan,
            ansi_white: self.ansi.white,
            ansi_bright_black: self.ansi.bright_black,
            ansi_bright_red: self.ansi.bright_red,
            ansi_bright_green: self.ansi.bright_green,
            ansi_bright_yellow: self.ansi.bright_yellow,
            ansi_bright_blue: self.ansi.bright_blue,
            ansi_bright_magenta: self.ansi.bright_magenta,
            ansi_bright_cyan: self.ansi.bright_cyan,
            ansi_bright_white: self.ansi.bright_white,
            surface_themes: self.surfaces.clone(),
        };
        (p, self.is_light)
    }
}

#[cfg(test)]
// 색 변환 검사를 위한 합성 색상 사용을 허용한다.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::mocha_fallback_colors;

    /// TOML 전체 색상과 내장 fallback을 비교한다.
    #[test]
    fn builtin_mocha_toml_matches_fallback_const() {
        let text = crate::MOCHA_TOML_TEXT;
        let file = ThemeFile::parse(text).expect("mocha.toml must parse");
        assert_eq!(file.is_light, Some(false));
        let (partial, _) = file.to_partial();

        let mut base = mocha_fallback_colors();
        // 다른 기본값에 적용해도 내장 색상 전체가 복원되는지 확인한다.
        base.crust = HexColor::from_rgb(0, 0, 0);
        base.text = HexColor::from_rgb(0, 0, 0);
        base.apply_partial(&partial);

        assert_eq!(base, mocha_fallback_colors());
    }

    #[test]
    fn builtin_latte_toml_parses() {
        let text = crate::LATTE_TOML_TEXT;
        let file = ThemeFile::parse(text).expect("latte.toml must parse");
        assert_eq!(file.is_light, Some(true));
        let (partial, _) = file.to_partial();
        // Latte의 대표 필드를 확인한다.
        assert_eq!(partial.text, Some(HexColor::from_rgb(0x4c, 0x4f, 0x69)));
        assert_eq!(partial.blue, Some(HexColor::from_rgb(0x1e, 0x66, 0xf5)));
        assert_eq!(
            partial.ansi_bright_white,
            Some(HexColor::from_rgb(0x4c, 0x4f, 0x69))
        );
    }

    #[test]
    fn empty_file_is_all_none() {
        let file = ThemeFile::parse("").unwrap();
        let (partial, is_light) = file.to_partial();
        assert!(is_light.is_none());
        assert!(partial.crust.is_none());
        assert!(partial.text.is_none());
        assert!(partial.ansi_bright_white.is_none());
    }

    #[test]
    fn partial_file_keeps_unspecified_fields_none() {
        let text = r##"
            label = "Custom"
            [accent]
            blue = "#00ff00"
        "##;
        let file = ThemeFile::parse(text).unwrap();
        assert_eq!(file.label.as_deref(), Some("Custom"));
        let (partial, _) = file.to_partial();
        assert_eq!(partial.blue, Some(HexColor::from_rgb(0, 0xff, 0)));
        assert!(partial.red.is_none());
        assert!(partial.crust.is_none());
    }

    #[test]
    fn invalid_hex_rejects_file() {
        let text = r#"
            [palette]
            crust = "not-a-color"
        "#;
        assert!(ThemeFile::parse(text).is_err());
    }
}
