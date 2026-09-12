//! 그림자 토큰 전사 대조 — vendor json ↔ `tasty-type-appearance` 의 `ShadowToken` 상수.
//!
//! 그림자는 `$type: shadow` 라 생성기가 건너뛴다(`dtcg.rs` 의 `Skip::Type`). 즉 이
//! 축의 값은 **사람이 손으로 옮겨 적는다.** 그래서 옮겨 적은 값이 디자인 정본과 같은지
//! 묻는 자동 검사가 없으면 발산이 조용히 남는다 — 실제로 남아 있었다(popover 가
//! `0 8px 24px /α90` 으로, 정본 `0 6px 18px /.4` 와 offset·blur·alpha 셋 다 달랐다).
//! 상수 옆 주석은 값을 지키지 못한다.
//!
//! `theme.rs` 의 유닛 시험(`shadow_token_alphas_match_their_design_fractions` ·
//! `no_shipped_shadow_token_uses_negative_spread`)과 축이 다르다 — 그쪽은 **소수 알파를
//! u8 로 옮기는 산술**과 **음수 spread 금지**를 잠그고, 값이 정본과 같은지는 안 본다
//! (그 crate 는 vendor json 을 못 읽는다). 여기서 그 남은 축을 닫는다.
//!
//! 어느 표면이 어느 값을 쓰는가는 이 시험의 물음이 아니다 — 그것은 SCOPE RULE
//! (`docs/adr/0254-floating-surface-shadow-scope-rule.md`)이고 소스에서 읽을 판정기가
//! 없다(ADR Consequences).

use tasty_design_tokens::DTCG_JSON;
use tasty_design_tokens::dtcg::{self, ThemeMode, Tier};
use tasty_type_appearance::theme::{SHADOW_MODAL, SHADOW_POPOVER, ShadowToken};

/// 정본 raw 그림자 값 ↔ Rust 상수 대응. **alias 가 아닌 값을 직접 든 토큰만** 여기 온다
/// (alias 토큰은 가리키는 쪽이 검사되면 함께 지켜진다 — `resolve` 가 그 체인을 탄다).
///
/// `Rust` 가 `None` 인 항목은 **의도적 미구현**이다. 값을 근사해 넣지 않는 것이 디자인
/// 지시이고, 근거는 ADR-0254 — 사유를 함께 적어 둔다.
const RAW_SHADOW_TOKENS: &[(&str, Option<ShadowToken>, &str)] = &[
    ("semantic.shadow-popover", Some(SHADOW_POPOVER), ""),
    ("semantic.shadow-modal", Some(SHADOW_MODAL), ""),
    (
        "component.titlebar-csd-shadow",
        None,
        "음수 spread(-8px) — egui `epaint::Shadow::spread` 가 u8 이라 담지 못한다. \
         근사하면 falloff 가 8px 리사이즈 엣지 밴드를 넘어 번지므로 미구현으로 둔다 \
         (ADR-0254). 그리려면 egui 밖 렌더 경로가 먼저 필요하다",
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

/// vendor json 의 `$type: shadow` 토큰 중 **alias 가 아닌 것**이 전부 명부에 있는가.
///
/// 명부를 안 늘린 채 새 raw 그림자가 들어오면 위 대조가 그것을 **안 보고** 초록이
/// 된다. 세 번째 값이 끼어드는 자리가 바로 거기라, 들어오는 순간 여기서 멈춰야 한다 —
/// 새 값은 SCOPE RULE 의 재검토 트리거다(ADR-0254).
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
         거짓 초록이 된다($type 이름을 먼저 확인해라)"
    );
    let mut listed: Vec<String> = RAW_SHADOW_TOKENS
        .iter()
        .map(|(p, _, _)| (*p).to_string())
        .collect();
    listed.sort();
    assert_eq!(
        found, listed,
        "vendor json 의 raw 그림자 토큰과 RAW_SHADOW_TOKENS 가 어긋났다 — 명부 밖 값은 \
         전사 대조를 통과하지 않고 **안 보인다**"
    );
}

/// alias 그림자 토큰은 전부 명부에 있는 raw 값으로 귀착하는가.
///
/// 컴포넌트 토큰(`banner-shadow`·`modhint-shadow` …)은 alias 라 값을 직접 안 든다.
/// 그래서 위 두 시험이 그것을 안 보는데, alias 가 **명부 밖 raw 값**을 새로 가리키면
/// 그것이 곧 세 번째 그림자다. 체인의 끝을 여기서 잠근다.
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
        "alias 그림자 토큰을 하나도 못 찾았다 — 스캔이 비면 거짓 초록이 된다"
    );
    for token in aliases {
        let target = dtcg::alias_target(&token.value).expect("filtered");
        assert!(
            listed.contains(&target),
            "{} 가 명부 밖 raw 값 `{target}` 을 가리킨다 — 그림자 값은 둘뿐이고, \
             세 번째가 필요하면 SCOPE RULE(ADR-0254) 재검토가 먼저다",
            token.path()
        );
    }
}

/// component tier 의 `*-shadow` 이름이 전부 `$type: shadow` 인가 — 이름만 shadow 인
/// 치수 토큰(`kbd-shadow-depth` 같은 3D edge)과 섞이면 위 세 시험의 모수가 흔들린다.
/// 그 둘은 축이 다르다(떠 있는 그림자 vs 키캡 하단 edge).
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
             맞추고, 아니면 이름을 바꿔라(`*-shadow-depth` 처럼 축을 드러낸다)",
            token.path(),
            token.ty
        );
    }
}
