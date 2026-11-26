//! 생성기가 건너뛰는 color-mix 토큰의 수기 접근자가 DTCG 식과 같은 색을 내는지 본다.
//! DTCG 값을 별칭과 `var(--tasty-…)` 참조까지 따라가 srgb color-mix로 계산하고, Mocha·Latte 내장 테마의
//! 접근자 값과 비교한다. 단계마다 8비트로 반올림하는 것은 앱의 `mix_srgb`·`with_alpha`와 같다.

use tasty_design_tokens::DTCG_JSON;
use tasty_design_tokens::dtcg::{self, ThemeMode, TokenSet};
use tasty_themes::{LATTE_TOML_TEXT, MOCHA_TOML_TEXT, ThemeFile, mocha_fallback_colors};
use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;

fn theme(toml: &str, light: bool) -> Theme {
    let file = ThemeFile::parse(toml).expect("embedded theme must parse");
    let (partial, _) = file.to_partial();
    let mut colors = mocha_fallback_colors();
    colors.apply_partial(&partial);
    Theme::with_colors(colors, light)
}

/// `var(--tasty-<name>)`가 가리키는 토큰 경로. 이름은 계층 접두 없이 쓰므로 세 계층에서 찾는다.
fn var_path(set: &TokenSet, name: &str) -> String {
    ["semantic", "component", "primitive"]
        .iter()
        .map(|tier| format!("{tier}.{name}"))
        .find(|path| set.get(path).is_some())
        .unwrap_or_else(|| panic!("var(--tasty-{name}) has no token"))
}

fn resolve(set: &TokenSet, path: &str, mode: ThemeMode) -> String {
    set.resolve(path, mode)
        .unwrap_or_else(|e| panic!("{path} ({mode:?}): {e}"))
}

/// 괄호 밖의 쉼표로 나눈다.
fn split_top_level(s: &str) -> Vec<&str> {
    let (mut depth, mut start, mut parts) = (0usize, 0usize, Vec::new());
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim());
    parts
}

/// `55%` 또는 `calc(var(--tasty-<number token>) * 100%)`를 정수 퍼센트로 읽는다. 정본 비율은 모두 정수
/// 퍼센트라, 계산을 정수로 해 정확한 .5를 그대로 다룬다. 정수 퍼센트가 아니면 멈춘다.
fn ratio(set: &TokenSet, s: &str, mode: ThemeMode) -> u64 {
    let value: f64 = if let Some(pct) = s.strip_suffix('%').filter(|p| !p.contains('(')) {
        pct.trim().parse::<f64>().expect("percentage")
    } else {
        let name = s
            .strip_prefix("calc(var(--tasty-")
            .and_then(|rest| rest.strip_suffix(") * 100%)"))
            .unwrap_or_else(|| panic!("unsupported ratio {s:?}"));
        let n: f64 = resolve(set, &var_path(set, name), mode)
            .parse()
            .unwrap_or_else(|_| panic!("{name} is not a number"));
        n * 100.0
    };
    let pct = value.round();
    assert!(
        (value - pct).abs() < 1e-9 && (0.0..=100.0).contains(&pct),
        "ratio {s:?} is not a whole percentage"
    );
    pct as u64
}

/// color-mix 한 항: 색과 (있으면) 비율. 색은 `var(--tasty-…)`, `transparent`, 또는 hex다.
#[allow(clippy::disallowed_methods)] // reason: DTCG 식의 transparent 항을 계산값으로 조립하며 제품 색을 정의하지 않는다.
fn term(set: &TokenSet, s: &str, mode: ThemeMode) -> (HexColor, Option<u64>) {
    if let Some(rest) = s.strip_prefix("transparent") {
        let r = rest.trim();
        return (
            HexColor::from_rgba(0, 0, 0, 0),
            (!r.is_empty()).then(|| ratio(set, r, mode)),
        );
    }
    if s.starts_with('#') {
        let (hex, r) = s.split_once(' ').unwrap_or((s, ""));
        let r = r.trim();
        return (
            color(set, hex, mode),
            (!r.is_empty()).then(|| ratio(set, r, mode)),
        );
    }
    let rest = s
        .strip_prefix("var(--tasty-")
        .unwrap_or_else(|| panic!("unsupported color term {s:?}"));
    let close = rest.find(')').expect("var closes");
    let color = color(
        set,
        &resolve(set, &var_path(set, &rest[..close]), mode),
        mode,
    );
    let r = rest[close + 1..].trim();
    (color, (!r.is_empty()).then(|| ratio(set, r, mode)))
}

/// DTCG 색 값 하나를 계산한다: hex, 또는 srgb color-mix(앞 항 비율 p, 뒤 항 1-p, 미리 곱한 alpha). 정수
/// 퍼센트와 정수 산술로 정확한 유리수 값을 구한 뒤 반올림한다(.5는 올림).
#[allow(clippy::disallowed_methods)] // reason: DTCG color-mix 계산 결과를 조립하며 제품 색을 정의하지 않는다.
fn color(set: &TokenSet, raw: &str, mode: ThemeMode) -> HexColor {
    if raw.starts_with('#') {
        return HexColor::from_hex(raw).unwrap_or_else(|| panic!("bad hex {raw}"));
    }
    let inner = raw
        .strip_prefix("color-mix(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or_else(|| panic!("unsupported color {raw:?}"));
    let parts = split_top_level(inner);
    assert_eq!(parts.len(), 3, "{raw}");
    assert_eq!(parts[0], "in srgb", "{raw}");
    let (a, pa) = term(set, parts[1], mode);
    let (b, pb) = term(set, parts[2], mode);
    // 두 항 모두 비율이 있으면 CSS는 두 비율을 합이 1이 되게 정규화한다. 이 계산기는 한 항 비율만
    // 읽으므로 그런 식은 거짓 일치를 내지 않게 거부한다.
    let p = match (pa, pb) {
        (Some(p), None) => p,
        (None, Some(q)) => 100 - q,
        (None, None) => panic!("one term must carry the ratio: {raw}"),
        (Some(_), Some(_)) => {
            panic!("both terms carry a ratio, which this reader does not normalize: {raw}")
        }
    };
    // 가중치 단위는 1 / (255 × 100). 결과 알파 = 합 / 100, 채널 = 가중 합 / 합.
    let wa = u64::from(a.a()) * p;
    let wb = u64::from(b.a()) * (100 - p);
    let sum = wa + wb;
    let round_div = |n: u64, d: u64| u8::try_from((2 * n + d) / (2 * d)).expect("8-bit result");
    let ch = |x: u8, y: u8| {
        if sum == 0 {
            0
        } else {
            round_div(u64::from(x) * wa + u64::from(y) * wb, sum)
        }
    };
    HexColor::from_rgba(
        ch(a.r(), b.r()),
        ch(a.g(), b.g()),
        ch(a.b(), b.b()),
        round_div(sum, 100),
    )
}

type Accessor = fn(&Theme) -> HexColor;

fn assert_accessors_match(cases: &[(&str, Accessor)]) {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    for (mode, th) in [
        (ThemeMode::Mocha, theme(MOCHA_TOML_TEXT, false)),
        (ThemeMode::Latte, theme(LATTE_TOML_TEXT, true)),
    ] {
        for (path, accessor) in cases {
            let raw = resolve(&set, path, mode);
            assert_eq!(
                accessor(&th),
                color(&set, &raw, mode),
                "{path} {mode:?}: {raw}"
            );
        }
    }
}

/// semantic `tint-*-border`와 그 별칭인 component `tag-*-border`.
#[test]
fn tint_border_accessors_match_the_design_mix() {
    assert_accessors_match(&[
        ("semantic.tint-primary-border", Theme::tint_primary_border),
        ("semantic.tint-info-border", Theme::tint_info_border),
        ("semantic.tint-success-border", Theme::tint_success_border),
        ("semantic.tint-warning-border", Theme::tint_warning_border),
        (
            "semantic.tint-attention-border",
            Theme::tint_attention_border,
        ),
        ("semantic.tint-danger-border", Theme::tint_danger_border),
        ("semantic.tint-attached-border", Theme::tint_attached_border),
        ("component.tag-info-border", Theme::tag_info_border),
        ("component.tag-success-border", Theme::tag_success_border),
        ("component.tag-warning-border", Theme::tag_warning_border),
        (
            "component.tag-attention-border",
            Theme::tag_attention_border,
        ),
        ("component.tag-danger-border", Theme::tag_danger_border),
        ("component.tag-attached-border", Theme::tag_attached_border),
    ]);
}

/// DAG block 티어 채움 `dag-*-block-bg`(상태색 55% + surface-raised).
#[test]
fn dag_block_fill_accessors_match_the_design_mix() {
    assert_accessors_match(&[
        (
            "component.dag-status-waiting-block-bg",
            Theme::dag_status_waiting_block_bg,
        ),
        (
            "component.dag-status-ready-block-bg",
            Theme::dag_status_ready_block_bg,
        ),
        (
            "component.dag-status-running-block-bg",
            Theme::dag_status_running_block_bg,
        ),
        (
            "component.dag-status-succeeded-block-bg",
            Theme::dag_status_succeeded_block_bg,
        ),
        (
            "component.dag-status-failed-block-bg",
            Theme::dag_status_failed_block_bg,
        ),
        (
            "component.dag-status-cancelled-block-bg",
            Theme::dag_status_cancelled_block_bg,
        ),
        (
            "component.dag-status-skipped-block-bg",
            Theme::dag_status_skipped_block_bg,
        ),
        (
            "component.dag-status-unknown-block-bg",
            Theme::dag_status_unknown_block_bg,
        ),
        (
            "component.dag-status-partially-failed-block-bg",
            Theme::dag_status_partially_failed_block_bg,
        ),
        (
            "component.dag-phase-awaiting-block-bg",
            Theme::dag_phase_awaiting_block_bg,
        ),
    ]);
}

/// 시험 입력 `sky 36%, transparent`의 기대 알파(0.36 × 255 반올림). 제품 계수가 아니라 계산기 시험의 기대값이다.
const SKY_36_ALPHA: u8 = 92;

/// 계산기가 비율과 바탕을 실제로 읽는지 본다. 앞 항만 쓰거나 비율을 무시하면 두 값이 같아진다.
#[test]
fn the_mix_reader_weighs_both_terms() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    let mode = ThemeMode::Mocha;
    let a = color(
        &set,
        "color-mix(in srgb, var(--tasty-color-sky) 55%, var(--tasty-color-black))",
        mode,
    );
    let b = color(
        &set,
        "color-mix(in srgb, var(--tasty-color-sky) 45%, var(--tasty-color-black))",
        mode,
    );
    let sky = color(&set, &resolve(&set, "primitive.color-sky", mode), mode);
    assert_ne!(a, b);
    assert_ne!(a, sky);
    let clear = color(
        &set,
        "color-mix(in srgb, var(--tasty-color-sky) 36%, transparent)",
        mode,
    );
    assert_eq!(clear, sky.with_alpha(SKY_36_ALPHA));
}

/// 두 항 모두에 비율이 있는 식은 계산하지 않고 멈춘다. 뒤 비율을 버리면 정본이 50:50으로 바뀌어도
/// 55:45로 계산해 거짓 일치가 난다.
#[test]
#[should_panic(expected = "both terms carry a ratio")]
fn the_mix_reader_rejects_a_ratio_on_both_terms() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    color(
        &set,
        "color-mix(in srgb, var(--tasty-color-sky) 55%, var(--tasty-color-black) 55%)",
        ThemeMode::Mocha,
    );
}

/// 사용자 테마의 완전히 투명한 강조색 `[accent] yellow`(알파 0).
const CLEAR_YELLOW: &str = "[accent]\nyellow = \"#f9e2af00\"\n";

/// 강조색이 완전히 투명하면 Tag 테두리는 정본 식처럼 투명 검정이다(강조색 RGB를 남기지 않는다).
#[test]
fn tint_border_of_a_clear_accent_is_transparent_black() {
    let th = user_theme(MOCHA_TOML_TEXT, false, CLEAR_YELLOW);
    let accent = th.accent_warning();
    assert_eq!(accent.a(), 0, "the user accent must stay clear");
    let expected = design_mix_with(
        "semantic.tint-warning-border",
        ThemeMode::Mocha,
        &[("accent-warning", accent)],
    );
    assert_eq!(expected.to_hex(), "#00000000");
    assert_eq!(th.tint_warning_border(), expected, "tint-warning-border");
    assert_eq!(th.tag_warning_border(), expected, "tag-warning-border");
}

/// 내장 테마 TOML 위에 사용자 테마 조각(`[accent]` 반투명 색 등)을 얹은 테마.
fn user_theme(base: &str, light: bool, overlay: &str) -> Theme {
    let mut colors = mocha_fallback_colors();
    for toml in [base, overlay] {
        let file = ThemeFile::parse(toml).expect("theme toml must parse");
        colors.apply_partial(&file.to_partial().0);
    }
    Theme::with_colors(colors, light)
}

/// 정본 식 `path`의 `var(--tasty-<name>)` 항을 사용자 테마 색으로 바꿔 계산한다.
fn design_mix_with(path: &str, mode: ThemeMode, vars: &[(&str, HexColor)]) -> HexColor {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    let mut raw = resolve(&set, path, mode);
    for (name, value) in vars {
        let var = format!("var(--tasty-{name})");
        assert!(raw.contains(&var), "{path}: {raw} has no {var}");
        raw = raw.replace(&var, &value.to_hex());
    }
    color(&set, &raw, mode)
}

/// 사용자 테마의 반투명 강조색 `[accent] yellow`(알파 128).
const TRANSLUCENT_YELLOW: &str = "[accent]\nyellow = \"#f9e2af80\"\n";
/// 사용자 테마의 반투명 강조색 `[accent] sky`(알파 128).
const TRANSLUCENT_SKY: &str = "[accent]\nsky = \"#89dceb80\"\n";
/// 알파 128 강조색을 `color-mix(… 36%, transparent)`로 낮춘 기대 알파(128 × 0.36 반올림).
const HALF_ALPHA_AT_TINT_BORDER: u8 = 46;
/// 알파 128 강조색 55%를 불투명 바탕에 섞은 기대 알파(128/255 × 0.55 + 0.45, × 255 반올림).
const HALF_ALPHA_AT_BLOCK_FILL: u8 = 185;

/// Tag 테두리는 반투명 강조색의 알파에 tint-border 비율을 곱한다(덮어쓰지 않는다).
#[test]
fn tint_border_multiplies_a_translucent_accent_alpha() {
    let th = user_theme(MOCHA_TOML_TEXT, false, TRANSLUCENT_YELLOW);
    let accent = th.accent_warning();
    assert_eq!(accent.a(), 128, "the user accent must stay translucent");
    let expected = design_mix_with(
        "semantic.tint-warning-border",
        ThemeMode::Mocha,
        &[("accent-warning", accent)],
    );
    assert_eq!(expected, accent.with_alpha(HALF_ALPHA_AT_TINT_BORDER));
    assert_eq!(th.tint_warning_border(), expected, "tint-warning-border");
    assert_eq!(th.tag_warning_border(), expected, "tag-warning-border");
}

/// DAG block 채움은 반투명 강조색의 알파를 color-mix 규칙대로 섞는다(바탕 알파를 그대로 쓰지 않는다).
#[test]
fn dag_block_fill_mixes_a_translucent_accent_alpha() {
    let th = user_theme(MOCHA_TOML_TEXT, false, TRANSLUCENT_SKY);
    let accent = th.accent_info_raised();
    let surface = th.surface_raised();
    assert_eq!(accent.a(), 128, "the user accent must stay translucent");
    assert_eq!(surface.a(), u8::MAX, "the block ground is opaque");
    let expected = design_mix_with(
        "component.dag-status-ready-block-bg",
        ThemeMode::Mocha,
        &[("accent-info-raised", accent), ("surface-raised", surface)],
    );
    assert_eq!(expected.a(), HALF_ALPHA_AT_BLOCK_FILL);
    assert_eq!(th.dag_status_ready_block_bg(), expected);
}

/// 정본 식 계산이 정확히 .5에 떨어지는 사용자 테마. 앞은 sky(알파 221)로 Ready block R이 120.5, 뒤는
/// sky(알파 1)와 surface0(알파 31)으로 알파가 14.5다. 정본 반올림은 둘 다 올린다.
const HALF_STEP_SKY: &str = "[accent]\nsky = \"#bcdcebdd\"\n";
const HALF_STEP_ALPHA: &str =
    "[accent]\nsky = \"#89dceb01\"\n[palette]\nsurface0 = \"#3132441f\"\n";
/// 위 두 입력의 정본 값(120.5 → 121, 14.5 → 15).
const HALF_STEP_RED: u8 = 121;
const HALF_STEP_ALPHA_BYTE: u8 = 15;
/// 불투명 두 색에서 R이 정확히 23.5인 사용자 테마(sky R 1, surface0 R 51). f32 보간은 23으로 내린다.
const HALF_STEP_OPAQUE: &str = "[accent]\nsky = \"#01dceb\"\n[palette]\nsurface0 = \"#333244\"\n";
/// 투명 sky와 알파 10 surface0으로 알파가 정확히 4.5인 사용자 테마. f64 비율 0.55로 계산하면 4로 내린다.
const HALF_STEP_FAINT_ALPHA: &str =
    "[accent]\nsky = \"#89dceb00\"\n[palette]\nsurface0 = \"#3132440a\"\n";
/// 위 두 입력의 정본 값(23.5 → 24, 4.5 → 5).
const HALF_STEP_OPAQUE_RED: u8 = 24;
const HALF_STEP_FAINT_ALPHA_BYTE: u8 = 5;
/// 강조색과 바탕이 모두 완전히 투명한 사용자 테마.
const CLEAR_SKY_AND_GROUND: &str =
    "[accent]\nsky = \"#89dceb00\"\n[palette]\nsurface0 = \"#31324400\"\n";

/// 사용자 테마의 Ready block 채움과 그 정본 식 계산값.
fn ready_block_against_design(overlay: &str) -> (HexColor, HexColor) {
    let th = user_theme(MOCHA_TOML_TEXT, false, overlay);
    let expected = design_mix_with(
        "component.dag-status-ready-block-bg",
        ThemeMode::Mocha,
        &[
            ("accent-info-raised", th.accent_info_raised()),
            ("surface-raised", th.surface_raised()),
        ],
    );
    (th.dag_status_ready_block_bg(), expected)
}

/// 정본 계산이 .5에 떨어지는 채널과 알파도 정본처럼 올린다.
#[test]
fn dag_block_fill_rounds_half_steps_like_the_design_mix() {
    let (got, expected) = ready_block_against_design(HALF_STEP_SKY);
    assert_eq!(expected.r(), HALF_STEP_RED, "{}", expected.to_hex());
    assert_eq!(got, expected, "red half step");
    let (got, expected) = ready_block_against_design(HALF_STEP_ALPHA);
    assert_eq!(expected.a(), HALF_STEP_ALPHA_BYTE, "{}", expected.to_hex());
    assert_eq!(got, expected, "alpha half step");
}

/// 두 색이 불투명해도 정확한 .5 채널을 정본처럼 올린다(f32 보간은 23으로 내린다).
#[test]
fn dag_block_fill_rounds_an_opaque_half_step_like_the_design_mix() {
    let (got, expected) = ready_block_against_design(HALF_STEP_OPAQUE);
    assert_eq!(expected.r(), HALF_STEP_OPAQUE_RED, "{}", expected.to_hex());
    assert_eq!(got, expected, "opaque red half step");
}

/// 정확히 4.5인 옅은 알파도 정본처럼 5로 올린다(f64 비율 0.55 계산은 4로 내린다).
#[test]
fn dag_block_fill_rounds_a_faint_alpha_half_step_like_the_design_mix() {
    let (got, expected) = ready_block_against_design(HALF_STEP_FAINT_ALPHA);
    assert_eq!(
        expected.a(),
        HALF_STEP_FAINT_ALPHA_BYTE,
        "{}",
        expected.to_hex()
    );
    assert_eq!(got, expected, "faint alpha half step");
}

/// 강조색과 바탕이 모두 완전히 투명하면 block 채움은 정본 식처럼 투명 검정이다.
#[test]
fn dag_block_fill_of_clear_inputs_is_transparent_black() {
    let (got, expected) = ready_block_against_design(CLEAR_SKY_AND_GROUND);
    assert_eq!(expected.to_hex(), "#00000000");
    assert_eq!(got, expected);
}
