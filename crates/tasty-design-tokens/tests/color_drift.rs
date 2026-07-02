//! DTCG primitive 색 ↔ `tasty-themes` 임베드 값 드리프트 가드.
//!
//! mocha 기준값 = `mocha_fallback_colors()`, latte 기준값 = `LATTE_TOML_TEXT` 파싱
//! (`builtin_mocha_toml_matches_fallback_const` 선례). 색의 SSoT 는 런타임 테마
//! 시스템이므로 여기서는 **값 일치만** 고정한다 — const 생성은 하지 않는다.
//!
//! 비교 범위는 hex 색 primitive 만 (시리즈 01 결정):
//! - `alpha-*` (rgba 문자열) — `ThemeColors` 에 대응 필드 없음 (`is_light` 도출) → 스킵
//! - `color-black`/`color-white` (절대색), `color-melon-*` (브랜드), `color-os-*`
//!   (OS 리터럴 const) — `ThemeColors` 테마 필드가 아님 → 스킵
//!
//! neutral ramp 12단 넘버링은 elevation role 기준 (TOKENS.md) — 대응표는
//! `docs/design/systems/token-crosswalk.md` 에도 기록되어 있다.

use tasty_design_tokens::DTCG_JSON;
use tasty_design_tokens::dtcg::{self, ThemeMode};
use tasty_themes::{LATTE_TOML_TEXT, ThemeFile, mocha_fallback_colors};
use tasty_type_appearance::color::HexColor;

#[test]
fn primitive_colors_match_embedded_themes() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    let mocha = mocha_fallback_colors();
    let latte_file = ThemeFile::parse(LATTE_TOML_TEXT).expect("latte.toml must parse");
    let (latte, is_light) = latte_file.to_partial();
    assert_eq!(is_light, Some(true), "latte.toml must declare is_light");

    // (DTCG primitive 이름, mocha 기준값, latte 기준값)
    let pairs: &[(&str, HexColor, Option<HexColor>)] = &[
        // ── neutral ramp 12단 (neutral-0 = 최심 배경 … neutral-1100 = 최강 전경) ──
        ("color-neutral-0", mocha.crust, latte.crust),
        ("color-neutral-100", mocha.mantle, latte.mantle),
        ("color-neutral-200", mocha.base, latte.base),
        ("color-neutral-300", mocha.surface0, latte.surface0),
        ("color-neutral-400", mocha.surface1, latte.surface1),
        ("color-neutral-500", mocha.surface2, latte.surface2),
        ("color-neutral-600", mocha.overlay0, latte.overlay0),
        ("color-neutral-700", mocha.overlay1, latte.overlay1),
        ("color-neutral-800", mocha.overlay2, latte.overlay2),
        ("color-neutral-900", mocha.subtext0, latte.subtext0),
        ("color-neutral-1000", mocha.subtext1, latte.subtext1),
        ("color-neutral-1100", mocha.text, latte.text),
        // ── accent hue 13종 (catppuccin — 테마당 hue 별 1값, ramp 없음) ──
        ("color-blue", mocha.blue, latte.blue),
        ("color-green", mocha.green, latte.green),
        ("color-red", mocha.red, latte.red),
        ("color-yellow", mocha.yellow, latte.yellow),
        ("color-peach", mocha.peach, latte.peach),
        ("color-mauve", mocha.mauve, latte.mauve),
        ("color-teal", mocha.teal, latte.teal),
        ("color-sky", mocha.sky, latte.sky),
        ("color-lavender", mocha.lavender, latte.lavender),
        ("color-flamingo", mocha.flamingo, latte.flamingo),
        ("color-pink", mocha.pink, latte.pink),
        ("color-maroon", mocha.maroon, latte.maroon),
        ("color-rosewater", mocha.rosewater, latte.rosewater),
    ];

    for (name, mocha_expected, latte_expected) in pairs {
        let path = format!("primitive.{name}");

        let mocha_raw = set
            .resolve(&path, ThemeMode::Mocha)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        let mocha_actual = HexColor::from_hex(&mocha_raw)
            .unwrap_or_else(|| panic!("{path}: mocha 값이 hex 가 아님: {mocha_raw}"));
        assert_eq!(
            mocha_actual, *mocha_expected,
            "{path} mocha drift ({mocha_raw})"
        );

        let latte_raw = set
            .resolve(&path, ThemeMode::Latte)
            .unwrap_or_else(|e| panic!("{path}: {e}"));
        let latte_actual = HexColor::from_hex(&latte_raw)
            .unwrap_or_else(|| panic!("{path}: latte 값이 hex 가 아님: {latte_raw}"));
        assert_eq!(
            Some(latte_actual),
            *latte_expected,
            "{path} latte drift ({latte_raw})"
        );
    }
}

/// `placeholder` 필드는 DTCG primitive 미대응(ramp 밖)이지만, shipped 테마에서 값이
/// `overlay0`(=neutral-600)와 동일하다. design-tokens-05b 가 overlay0 직접읽기 3곳을
/// `text_placeholder()`(=placeholder field)로 값-보존 이식하면서 이 결합에 의존한다
/// (convert/port_scanner/remote_tool disabled-role). shipped 테마에서 그 결합이 깨지면
/// 그 3화면만 색이 어긋나므로 여기서 가드한다 (mocha·latte 양쪽).
#[test]
fn placeholder_matches_overlay0_in_shipped_themes() {
    let mocha = mocha_fallback_colors();
    assert_eq!(
        mocha.placeholder, mocha.overlay0,
        "mocha placeholder != overlay0 — 05b overlay0→text_placeholder 값-보존 결합 깨짐"
    );
    let latte_file = ThemeFile::parse(LATTE_TOML_TEXT).expect("latte.toml must parse");
    let (latte, _) = latte_file.to_partial();
    assert_eq!(
        latte.placeholder, latte.overlay0,
        "latte placeholder != overlay0 — 05b overlay0→text_placeholder 값-보존 결합 깨짐"
    );
}
