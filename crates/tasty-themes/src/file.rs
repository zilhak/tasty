//! `ThemeFile` — TOML 표면. 모든 색상 필드는 `Option`.
//!
//! 누락된 필드는 `apply_partial` 로 base 에 반영될 때 그냥 무시되므로,
//! 사용자가 일부 색상만 정의한 partial 테마도 자연스럽게 적용된다.

use serde::Deserialize;
use tasty_core::color::HexColor;
use tasty_core::theme::PartialColors;
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TerminalSection {
    pub fg: Option<HexColor>,
    pub bg: Option<HexColor>,
    pub selection_bg: Option<HexColor>,
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

    /// 모든 색상 필드를 `PartialColors` 의 평평한 형태로 풀어낸다.
    /// is_light 는 별도 반환 — 호출자가 settings 에 반영할지 결정.
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
            terminal_fg: self.terminal.fg,
            terminal_bg: self.terminal.bg,
            selection_bg: self.terminal.selection_bg,
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
        };
        (p, self.is_light)
    }
}

#[cfg(test)]
// 테스트 더미 색 생성 — 정상 운영 경로 아님.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use tasty_core::theme::MOCHA_FALLBACK_COLORS;

    /// 빌트인 mocha.toml 텍스트가 `MOCHA_FALLBACK_COLORS` const 와 완전히 일치하는지 확인.
    /// 어긋나면 런타임에 사용자가 보는 색상과 fallback 이 달라진다.
    #[test]
    fn builtin_mocha_toml_matches_fallback_const() {
        let text = crate::MOCHA_TOML_TEXT;
        let file = ThemeFile::parse(text).expect("mocha.toml must parse");
        assert_eq!(file.is_light, Some(false));
        let (partial, _) = file.to_partial();

        // 빈 base 에 partial 을 적용하면 풀 세트로 채워져야 한다 (mocha 는 풀 세트).
        let mut base = MOCHA_FALLBACK_COLORS;
        // base 를 일부러 다르게 만든 뒤 partial 적용 결과가 MOCHA_FALLBACK_COLORS 와 같은지 확인.
        base.crust = HexColor::from_rgb(0, 0, 0);
        base.text = HexColor::from_rgb(0, 0, 0);
        base.apply_partial(&partial);

        assert_eq!(base, MOCHA_FALLBACK_COLORS);
    }

    #[test]
    fn builtin_latte_toml_parses() {
        let text = crate::LATTE_TOML_TEXT;
        let file = ThemeFile::parse(text).expect("latte.toml must parse");
        assert_eq!(file.is_light, Some(true));
        let (partial, _) = file.to_partial();
        // 라뜨도 풀 세트로 정의 — 주요 필드 일부만 spot-check.
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
