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
use tasty_themes::{LATTE_TOML_TEXT, MOCHA_TOML_TEXT, ThemeFile, mocha_fallback_colors};
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

/// `[surfaces.<kind>]` 의 전경 두 필드는 팔레트 토큰을 그대로 따라간다 —
/// `focused_fg == text`, `unfocused_fg == subtext0`. shipped 테마 TOML 에 그렇게
/// **주석으로만** 적혀 있어(`unfocused_fg = "#63667c"   # = subtext0`) 둘 중 하나만
/// 바뀌면 조용히 어긋난다. latte 의 `subtext0` 을 대비 때문에 내렸을 때 실제로
/// 세 곳을 손으로 맞춰야 했던 자리라 가드를 건다.
///
/// 배경 두 필드는 가드하지 않는다 — 관계가 kind 마다 다르다(terminal 의
/// `focused_bg` 는 팔레트 밖 `#000000`, `unfocused_bg` 는 terminal=base /
/// markdown=mantle). 균일한 관계가 아니면 가드가 아니라 족쇄가 된다.
#[test]
fn surface_foregrounds_track_palette_in_shipped_themes() {
    for (name, toml) in [("mocha", MOCHA_TOML_TEXT), ("latte", LATTE_TOML_TEXT)] {
        let file = ThemeFile::parse(toml).unwrap_or_else(|e| panic!("{name}.toml: {e}"));
        let text = file
            .palette
            .text
            .unwrap_or_else(|| panic!("{name}.toml: palette.text 없음"));
        let subtext0 = file
            .palette
            .subtext0
            .unwrap_or_else(|| panic!("{name}.toml: palette.subtext0 없음"));

        assert!(
            !file.surfaces.is_empty(),
            "{name}.toml 에 [surfaces.*] 가 하나도 없다 — 가드가 헛돈다"
        );
        for (kind, surface) in &file.surfaces {
            assert_eq!(
                surface.focused_fg,
                Some(text),
                "{name}.toml [surfaces.{kind}].focused_fg != palette.text ({text:?})"
            );
            assert_eq!(
                surface.unfocused_fg,
                Some(subtext0),
                "{name}.toml [surfaces.{kind}].unfocused_fg != palette.subtext0 ({subtext0:?})"
            );
        }
    }
}
