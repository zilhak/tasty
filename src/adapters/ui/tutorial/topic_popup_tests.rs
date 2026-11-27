//! 튜토리얼 목록 팝업의 키 입력(↓·Enter 는 수식키 없는 누름만 받는다)과 선택 행의 그린 색.

use super::*;
use crate::adapters::ui::popup::window_input_test_support::{
    Held, OwnerBody, draw_owner_body, held_press,
};

/// 키 주인 목록을 `held` 를 붙인 `key` 한 프레임으로 그린다. 고른 줄과 그리기 결과를 돌려준다.
fn topics_frame(key: egui::Key, held: Option<Held>) -> (usize, Option<usize>, PopupAction) {
    crate::i18n::init("en");
    let (mut state, session) = crate::state::tests::test_state();
    let engine = session.read();
    // 진행 기록 파일을 읽지 않도록 목록을 이미 불러온 것으로 둔다.
    state.tutorial.catalog_loaded = true;
    let ctx = egui::Context::default();
    let mut payload = crate::adapters::ui::popup::PopupPayload::None;
    let press = held_press(&ctx, key, held);
    let action = draw_owner_body(
        &ctx,
        &mut state,
        &engine,
        OwnerBody {
            id: TUTORIAL_TOPICS_POPUP_ID,
            draw: draw_tutorial_topics_popup,
            payload: &mut payload,
            keyboard: crate::adapters::ui::popup::PopupKeyboard::Keys,
        },
        vec![press],
    );
    (
        state.tutorial.popup_selected,
        state.tutorial.pending_start,
        action,
    )
}

/// T-9a: Given 키 주인 튜토리얼 목록, When 수식키 없는 ↓ 를 누르면, Then 다음 줄을 고른다.
#[test]
fn bare_down_moves_the_topic_selection() {
    assert_eq!(topics_frame(egui::Key::ArrowDown, None).0, 1);
}

/// T-9a: Given 키 주인 튜토리얼 목록, When Shift·Ctrl·Alt·Cmd(비-macOS Super)+↓ 를 누르면, Then 줄이 그대로다.
#[test]
fn modified_down_is_ignored_by_the_topics() {
    for held in Held::ALL {
        assert_eq!(
            topics_frame(egui::Key::ArrowDown, Some(held)).0,
            0,
            "{held:?}"
        );
    }
}

/// T-9a: Given 키 주인 튜토리얼 목록, When 수식키 없는 Enter 를 누르면, Then 고른 주제를 시작하고 닫는다.
#[test]
fn bare_enter_starts_the_topic() {
    let (_, start, action) = topics_frame(egui::Key::Enter, None);
    assert_eq!(start, Some(0));
    assert!(matches!(action, PopupAction::Close));
}

/// T-9a: Given 키 주인 튜토리얼 목록, When Shift·Ctrl·Alt·Cmd(비-macOS Super)+Enter 를 누르면, Then 시작하지 않는다.
#[test]
fn modified_enter_is_ignored_by_the_topics() {
    for held in Held::ALL {
        let (_, start, action) = topics_frame(egui::Key::Enter, Some(held));
        assert_eq!(start, None, "{held:?}");
        assert!(matches!(action, PopupAction::None), "{held:?}");
    }
}

/// 선택한 주제 행(surface-active 채움)의 테두리가 정본 토큰 색인지 그린 도형으로 본다. 반투명 accent
/// 테마도 그려, 불투명에서는 같은 값이 되는 고정 알파와 갈리게 한다.
#[test]
fn selected_topic_row_border_paints_the_design_token() {
    fn flatten(shape: &egui::Shape, out: &mut Vec<egui::epaint::RectShape>) {
        match shape {
            egui::Shape::Rect(r) => out.push(r.clone()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            _ => {}
        }
    }
    // 반투명 accent 의 알파. 정본 식은 accent 알파에 비율을 곱한다.
    const TRANSLUCENT_ACCENT_ALPHA: u8 = 128;
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.blue = colors.blue.with_alpha(TRANSLUCENT_ACCENT_ALPHA);
    let translucent = tasty_type_appearance::theme::Theme::with_colors_and_zoom(colors, false, 1.0);
    assert_eq!(translucent.accent_primary().a(), TRANSLUCENT_ACCENT_ALPHA);
    for (name, th) in [
        ("mocha", tasty_themes::mocha_fallback()),
        ("translucent accent", translucent),
    ] {
        let ctx = egui::Context::default();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                topic_row(ui, &th, 1, "T", "D", true);
            });
        });
        let mut rects = Vec::new();
        out.shapes
            .iter()
            .for_each(|c| flatten(&c.shape, &mut rects));
        let active = th.surface_active().to_egui();
        let borders: Vec<egui::Color32> = rects
            .iter()
            .filter(|r| r.fill == active && r.stroke.width > 0.0)
            .map(|r| r.stroke.color)
            .collect();
        assert_eq!(
            borders,
            vec![th.tutorial_topic_selected_border().to_egui()],
            "{name}: 선택 행 테두리"
        );
    }
}
