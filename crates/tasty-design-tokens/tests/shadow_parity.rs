//! 직접 옮겨 적은 ShadowToken 상수를 DTCG 그림자 값과 비교한다.
//! theme.rs의 알파 반올림·음수 spread 검사와 별도로 원본과의 값 일치를 확인한다.
//! 어떤 화면에 어느 그림자를 쓰는지는 이 시험에서 검사하지 않는다.
//! 선택 규칙: docs/design/systems/theme.md#떠-있는-표면의-그림자.

use tasty_design_tokens::DTCG_JSON;
use tasty_design_tokens::dtcg::{self, ThemeMode, Tier};
use tasty_type_appearance::theme::{SHADOW_MODAL, SHADOW_POPOVER, ShadowToken};

/// 별칭이 아닌 원본 그림자 값과 Rust 상수의 대응표.
/// None은 구현하지 않은 항목이며 그 이유를 함께 기록한다.
const RAW_SHADOW_TOKENS: &[(&str, Option<ShadowToken>, &str)] = &[
    ("semantic.shadow-popover", Some(SHADOW_POPOVER), ""),
    ("semantic.shadow-modal", Some(SHADOW_MODAL), ""),
    (
        "component.titlebar-csd-shadow",
        None,
        "음수 spread(-8px) — egui `epaint::Shadow::spread` 가 u8 이라 담지 못한다. \
         근사하면 falloff 가 8px 리사이즈 엣지 밴드를 넘어 번지므로 미구현으로 둔다 \
         (docs/design/systems/theme.md#떠-있는-표면의-그림자). 그리려면 egui 밖 렌더 경로가 먼저 필요하다",
    ),
];

/// `0 6px 18px rgba(0, 0, 0, 0.4)` · `0 18px 50px -8px rgba(0,0,0,.5)` 를 뜯는다.
/// 길이는 2~4 개(offset-x, offset-y, [blur], [spread]), 마지막이 `rgba(...)`.
fn parse_css_shadow(raw: &str) -> ParsedShadow {
    let (lengths, rgba) = raw
        .split_once("rgba(")
        .unwrap_or_else(|| panic!("rgba(...)가 없는 그림자 값: {raw}"));
    let px = |s: &str| -> f32 {
        s.trim_end_matches("px")
            .parse::<f32>()
            .unwrap_or_else(|_| panic!("길이가 아닌 값: {s} (원문 {raw})"))
    };
    let parts: Vec<f32> = lengths.split_whitespace().map(px).collect();
    assert!(
        (2..=4).contains(&parts.len()),
        "그림자 길이는 2~4 개여야 한다 (원문 {raw})"
    );
    let comps: Vec<&str> = rgba
        .trim_end()
        .trim_end_matches(')')
        .split(',')
        .map(str::trim)
        .collect();
    assert_eq!(comps.len(), 4, "rgba 는 네 성분이어야 한다 (원문 {raw})");
    assert!(
        comps[..3].iter().all(|c| *c == "0"),
        "그림자 색은 순수 검정이어야 한다 — `ShadowToken` 은 알파만 담는다 (원문 {raw})"
    );
    let alpha: f32 = comps[3]
        .parse()
        .unwrap_or_else(|_| panic!("알파가 소수가 아니다: {} (원문 {raw})", comps[3]));
    ParsedShadow {
        offset_x: parts[0],
        offset_y: parts[1],
        blur: parts.get(2).copied().unwrap_or(0.0),
        spread: parts.get(3).copied().unwrap_or(0.0),
        alpha,
    }
}

struct ParsedShadow {
    offset_x: f32,
    offset_y: f32,
    blur: f32,
    spread: f32,
    /// 디자인 원문의 **소수** 알파(0.0~1.0). u8 변환은 대조할 때 한다.
    alpha: f32,
}

/// 정본 raw 값이 Rust 상수와 필드 단위로 같은가.
#[test]
fn shadow_constants_transcribe_the_design_values() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    for (path, rust, reason) in RAW_SHADOW_TOKENS {
        let raw = set
            .resolve(path, ThemeMode::Mocha)
            .unwrap_or_else(|e| panic!("{path} 해석 실패: {e:?}"));
        let want = parse_css_shadow(&raw);
        let Some(token) = rust else {
            assert!(
                !reason.is_empty(),
                "{path} 를 미구현으로 두려면 사유를 적어라"
            );
            continue;
        };
        let got = *token;
        assert_eq!(
            (got.offset_x, got.offset_y, got.blur, got.spread),
            (want.offset_x, want.offset_y, want.blur, want.spread),
            "{path} 의 기하가 정본과 다르다 (정본 {raw})"
        );
        assert_eq!(
            got.alpha,
            (want.alpha * 255.0).round() as u8,
            "{path} 의 alpha 가 정본 소수 알파({})의 반올림과 다르다 (정본 {raw})",
            want.alpha
        );
    }
}

/// 새 그림자가 비교에서 빠지지 않도록 원본 토큰 전체와 대응표를 비교한다.
#[test]
fn raw_shadow_token_roster_is_complete() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    let mut found: Vec<String> = set
        .iter()
        .filter(|t| t.ty == "shadow" && dtcg::alias_target(&t.value).is_none())
        .map(|t| t.path())
        .collect();
    found.sort();
    assert!(
        !found.is_empty(),
        "vendor json 에서 raw 그림자 토큰을 하나도 못 찾았다 — 스캔이 비면 이 대조는 \
         아무 값도 검사하지 않는다($type 이름을 먼저 확인해라)"
    );
    let mut listed: Vec<String> = RAW_SHADOW_TOKENS
        .iter()
        .map(|(p, _, _)| (*p).to_string())
        .collect();
    listed.sort();
    assert_eq!(
        found, listed,
        "vendor json 의 raw 그림자 토큰과 RAW_SHADOW_TOKENS 가 어긋났다 — 명부 밖 값은 \
         값 비교에서 빠진다"
    );
}

/// 그림자 별칭이 대응표에 있는 원본을 가리키는지 확인한다.
#[test]
fn alias_shadow_tokens_land_on_a_listed_raw_value() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    let listed: Vec<&str> = RAW_SHADOW_TOKENS.iter().map(|(p, _, _)| *p).collect();
    let aliases: Vec<&dtcg::Token> = set
        .iter()
        .filter(|t| t.ty == "shadow" && dtcg::alias_target(&t.value).is_some())
        .collect();
    assert!(
        !aliases.is_empty(),
        "alias 그림자 토큰을 하나도 못 찾았다 — 비교할 별칭이 없다"
    );
    for token in aliases {
        let target = dtcg::alias_target(&token.value).expect("filtered");
        assert!(
            listed.contains(&target),
            "{} 가 명부 밖 raw 값 `{target}` 을 가리킨다 — 그림자 값은 둘뿐이고, \
             세 번째가 필요하면 그림자 선택 규칙(docs/design/systems/theme.md#떠-있는-표면의-그림자) 재검토가 먼저다",
            token.path()
        );
    }
}

/// `*-shadow`는 그림자 타입이어야 한다. 키캡 두께인 `*-shadow-depth`와 구분한다.
#[test]
fn shadow_named_dimension_tokens_are_not_floating_shadows() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    for token in set.iter().filter(|t| t.tier == Tier::Component) {
        if token.ty == "shadow" {
            continue;
        }
        assert!(
            !token.name.ends_with("-shadow"),
            "{} 가 `-shadow` 로 끝나는데 $type 이 `{}` 다 — 떠 있는 그림자면 $type 을 \
             맞추고, 아니면 이름을 바꿔라(`*-shadow-depth` 처럼 용도를 드러낸다)",
            token.path(),
            token.ty
        );
    }
}
