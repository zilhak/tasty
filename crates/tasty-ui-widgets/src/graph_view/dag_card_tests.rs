use super::*;

fn latte() -> Theme {
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT).expect("latte");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    Theme::with_colors(colors, is_light.unwrap_or(true))
}

/// 노드 상태 아홉 가지(개별 여덟 + 집계 전용 PartiallyFailed).
fn every_status() -> Vec<DagStatus> {
    let mut all = DagStatus::ALL.to_vec();
    all.push(DagStatus::PartiallyFailed);
    all
}

/// 토큰으로 옮기기 전 block 채움 비율. 시험이 값이 그대로인지 재는 기준이다.
const BEFORE_BLOCK_FILL_MIX: f32 = 0.55;

/// block 채움 토큰은 옮기기 전 식(상태색 55%를 surface-raised에 섞음)과 같은 색이다.
#[test]
fn block_fill_tokens_keep_the_previous_mix() {
    for theme in [tasty_themes::mocha_fallback(), latte()] {
        for status in every_status() {
            for awaiting in [false, true] {
                let accent = dag_card_colors(&theme, status, awaiting).0;
                let before = theme
                    .surface_raised()
                    .to_egui()
                    .lerp_to_gamma(accent.to_egui(), BEFORE_BLOCK_FILL_MIX);
                assert_eq!(
                    dag_block_fill(&theme, status, awaiting).to_egui(),
                    before,
                    "{status:?} awaiting={awaiting}"
                );
            }
        }
    }
}

/// 사용자 테마의 반투명 강조색 `[accent] sky`(알파 128). Ready block이 읽는 accent-info-raised가 된다.
const TRANSLUCENT_SKY: &str = "[accent]\nsky = \"#89dceb80\"\n";

/// 토큰은 알파를 곱하지 않은 8비트, 옛 식은 곱한 8비트로 반올림해, 반투명 입력이면 그릴 때 채널이 갈린다.
/// 아래 fixture(`#89dceb80`, 불투명 바탕)에서는 한 단계다. 이 fixture에 한한 값이다.
const PREMULTIPLIED_ROUNDING_SLACK: u8 = 1;

/// 반투명 강조색을 불투명 바탕에 섞는 이 fixture에서는 block 채움이 옮기기 전 식(premultiplied 보간)과 같은
/// 알파이고, 채널은 반올림 한 단계 안이다. 옛 방식과의 일치는 일반 규칙이 아니다: 반투명 입력이면(바탕이
/// 불투명이어도) 옛 방식과 채널마다 최대 두 단계 다를 수 있고 세 단계 이상은 없다(Gate4 r3 전수 측정). 그때는
/// 정본 color-mix 값이 맞다(정본 대조는 tasty-design-tokens `color_mix_accessors` 시험이 한다).
#[test]
fn block_fill_tokens_keep_the_previous_mix_for_a_translucent_accent_over_an_opaque_ground() {
    let mut colors = tasty_themes::mocha_fallback_colors();
    for toml in [tasty_themes::MOCHA_TOML_TEXT, TRANSLUCENT_SKY] {
        let file = tasty_themes::ThemeFile::parse(toml).expect("theme toml");
        colors.apply_partial(&file.to_partial().0);
    }
    let theme = Theme::with_colors(colors, false);
    let accent = dag_card_colors(&theme, DagStatus::Ready, false).0;
    assert!(
        accent.a() < u8::MAX,
        "the user accent must stay translucent"
    );
    assert_eq!(
        theme.surface_raised().a(),
        u8::MAX,
        "this fixture keeps the block ground opaque"
    );
    let before = theme
        .surface_raised()
        .to_egui()
        .lerp_to_gamma(accent.to_egui(), BEFORE_BLOCK_FILL_MIX);
    let after = dag_block_fill(&theme, DagStatus::Ready, false).to_egui();
    assert_eq!(after.a(), before.a(), "alpha {after:?} vs {before:?}");
    for (a, b) in after.to_array().into_iter().zip(before.to_array()) {
        assert!(
            a.abs_diff(b) <= PREMULTIPLIED_ROUNDING_SLACK,
            "{after:?} vs {before:?}"
        );
    }
}

/// block 배율로 그린 카드 한 장의 채움색.
fn painted_block_fill(theme: &Theme, status: DagStatus, awaiting: bool) -> egui::Color32 {
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(160.0, 48.0));
    let out = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let card = DagCardView {
                name: "build",
                status,
                awaiting,
                command_kind: "shell",
                glyph: "",
                status_label: "",
                duration: None,
                in_cycle: false,
            };
            let vis = NodeVisual {
                lod: Lod::Block,
                zoom: 1.0,
                selected: false,
                hovered: false,
                dimmed: false,
            };
            paint_dag_card(ui, ui.painter(), theme, rect, &card, &vis);
        });
    });
    let fills: Vec<egui::Color32> = out
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(r) if r.rect == rect && r.fill != egui::Color32::TRANSPARENT => {
                Some(r.fill)
            }
            _ => None,
        })
        .collect();
    assert_eq!(fills.len(), 1, "{status:?}: one block fill");
    fills[0]
}

/// block 배율 카드는 상태별 `dag-*-block-bg`로 채우고, 입력 대기는 대기 채움이다.
#[test]
fn block_cards_fill_with_the_block_tokens() {
    for theme in [tasty_themes::mocha_fallback(), latte()] {
        for status in every_status() {
            assert_eq!(
                painted_block_fill(&theme, status, false),
                dag_block_fill(&theme, status, false).to_egui(),
                "{status:?}"
            );
        }
        assert_eq!(
            painted_block_fill(&theme, DagStatus::Running, true),
            theme.dag_phase_awaiting_block_bg().to_egui(),
        );
    }
}
