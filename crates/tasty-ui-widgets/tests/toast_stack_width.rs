//! 스택의 카드가 공유 폭 없이 자기 내용 폭을 쓰고, 넓은 스코프에서도 `toast_max_width`를
//! 넘지 않으며, 오른쪽 끝이 앵커에 맞는지 검사한다. 좌측 강조 막대가 `toast_accent_width`
//! 두께인지도 검사한다. hint 키캡이 카드 오른쪽 끝 첫 줄에 놓이고 줄지 않으며 본문이 먼저
//! 줄바꿈되는지도 검사한다.

use egui::{Pos2, RawInput, Rect, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;
use tasty_ui_widgets::tokens::{TOAST_HINT_GAP, TOAST_PADDING_X};
use tasty_ui_widgets::{
    KbdKey, ToastEntryView, ToastScopeView, ToastViewProps, draw_toast_scopes, kbd_parts_width,
};

const LONG: &str = "This is a long notice that keeps going well past the width a single toast \
                    card may take, so it has to wrap onto several lines inside the card.";
const SHORT: &str = "Path copied";

/// 1280 폭 스코프에 긴 카드와 짧은 카드를 그리고 채우기 색이 `fill`인 사각형을 위에서부터
/// 돌려준다.
fn filled_rects(theme: &Theme, fill: impl Fn(&Theme) -> egui::Color32) -> Vec<Rect> {
    let fill = fill(theme);
    let ctx = egui::Context::default();
    let scope_rect = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0));
    let scopes = [ToastScopeView {
        scope_rect,
        entries: vec![
            ToastEntryView {
                kind: ToastKind::Warning,
                message: LONG.into(),
                hint: Vec::new(),
                alpha: 1.0,
            },
            ToastEntryView {
                kind: ToastKind::Success,
                message: SHORT.into(),
                hint: Vec::new(),
                alpha: 1.0,
            },
        ],
    }];
    let mut shapes = Vec::new();
    // 첫 프레임은 폰트 준비 전이라 두 번 돌린다.
    for _ in 0..2 {
        let out = ctx.run(
            RawInput {
                screen_rect: Some(scope_rect),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("toast_stack_width"),
                ));
                draw_toast_scopes(
                    &painter,
                    &ToastViewProps {
                        theme,
                        scopes: &scopes,
                    },
                );
            },
        );
        shapes = out.shapes;
    }
    let mut rects: Vec<Rect> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
            _ => None,
        })
        .collect();
    rects.sort_by(|a, b| a.top().total_cmp(&b.top()));
    rects
}

/// 카드 배경 사각형과 폭 상한.
fn card_rects() -> (Vec<Rect>, f32) {
    let theme = tasty_themes::mocha_fallback();
    let rects = filled_rects(&theme, |t| {
        tasty_ui_widgets::toast_card_colors(t, ToastKind::Info, 1.0).bg
    });
    (rects, theme.toast_max_width.value())
}

#[test]
fn stacked_cards_use_their_own_width_capped_at_toast_max_width() {
    let (rects, cap) = card_rects();
    assert_eq!(rects.len(), 2, "two card backgrounds: {rects:?}");
    let (long, short) = (rects[0], rects[1]);
    assert!(
        long.width() <= cap + 0.5,
        "long card {} exceeds toast-max-width {cap}",
        long.width()
    );
    // 상한에 걸려 줄바꿈했으므로 긴 카드가 더 높다.
    assert!(
        long.height() > short.height(),
        "long card did not wrap: {} vs {}",
        long.height(),
        short.height()
    );
    assert!(
        short.width() < long.width(),
        "short card {} is not narrower than the long card {}",
        short.width(),
        long.width()
    );
    assert!(
        (long.right() - short.right()).abs() <= 0.5,
        "right edges differ: {} vs {}",
        long.right(),
        short.right()
    );
}

#[test]
fn the_accent_rail_is_toast_accent_width_thick() {
    let theme = tasty_themes::mocha_fallback();
    let rails = filled_rects(&theme, |t| t.accent_warning().into());
    let cards = card_rects().0;
    assert_eq!(rails.len(), 1, "one warning rail: {rails:?}");
    let rail = rails[0];
    assert_eq!(rail.width(), theme.toast_accent_width.value());
    // 시안 `toast-accent-width` = size-3.
    assert_eq!(rail.width(), 3.0);
    assert_eq!(rail.left(), cards[0].left());
    assert_eq!(rail.height(), cards[0].height());
}

const LONG_HINT: &str = "Copied 3 lines from the selection to the clipboard, and the notice keeps \
                    going so that it has to wrap inside the card.";

fn hint() -> Vec<String> {
    vec!["Ctrl".into(), "Shift".into(), "C".into()]
}

/// 한 장을 그려 카드 배경과 키캡 상자(kbd-bg 채움)를 돌려준다.
fn draw(theme: &Theme, message: &str, hint: Vec<String>) -> (Rect, Vec<Rect>) {
    let ctx = egui::Context::default();
    let scope_rect = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0));
    let scopes = [ToastScopeView {
        scope_rect,
        entries: vec![ToastEntryView {
            kind: ToastKind::Success,
            message: message.into(),
            hint,
            alpha: 1.0,
        }],
    }];
    let mut shapes = Vec::new();
    // 첫 프레임은 폰트 준비 전이라 두 번 돌린다.
    for _ in 0..2 {
        let out = ctx.run(
            RawInput {
                screen_rect: Some(scope_rect),
                ..Default::default()
            },
            |ctx| {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("toast_hint"),
                ));
                draw_toast_scopes(
                    &painter,
                    &ToastViewProps {
                        theme,
                        scopes: &scopes,
                    },
                );
            },
        );
        shapes = out.shapes;
    }
    let rects_of = |fill: egui::Color32| -> Vec<Rect> {
        let mut v: Vec<Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
                _ => None,
            })
            .collect();
        v.sort_by(|a, b| a.left().total_cmp(&b.left()));
        v
    };
    // 키캡 바탕(kbd-bg)과 카드 바탕은 같은 surface-raised라 크기로 가른다.
    let card_bg = tasty_ui_widgets::toast_card_colors(theme, ToastKind::Success, 1.0).bg;
    assert_eq!(card_bg, egui::Color32::from(theme.kbd_bg()));
    let filled = rects_of(card_bg);
    let kbd_h = theme.kbd_size().value();
    let (caps, cards): (Vec<Rect>, Vec<Rect>) = filled
        .into_iter()
        .partition(|r| (r.height() - kbd_h).abs() <= 0.01);
    assert_eq!(cards.len(), 1, "one card: {cards:?}");
    (cards[0], caps)
}

fn hint_width(theme: &Theme) -> f32 {
    let ctx = egui::Context::default();
    let mut w = 0.0;
    for _ in 0..2 {
        drop(ctx.run(RawInput::default(), |ctx| {
            let keys: Vec<KbdKey<'_>> = ["Ctrl", "Shift", "C"]
                .into_iter()
                .map(KbdKey::Text)
                .collect();
            w = kbd_parts_width(ctx, theme, &keys).value();
        }));
    }
    w
}

#[test]
fn the_hint_sits_at_the_right_end_of_the_first_line() {
    let theme = tasty_themes::mocha_fallback();
    let (card, caps) = draw(&theme, "Path copied", hint());
    assert_eq!(caps.len(), 3, "three keycaps: {caps:?}");
    let last = caps[caps.len() - 1];
    assert!(
        (last.right() - (card.right() - TOAST_PADDING_X)).abs() <= 0.5,
        "hint right {} vs card right - padding {}",
        last.right(),
        card.right() - TOAST_PADDING_X
    );
    let (plain, no_caps) = draw(&theme, "Path copied", Vec::new());
    assert!(no_caps.is_empty(), "no hint draws no keycap");
    let reserve = hint_width(&theme) + TOAST_HINT_GAP;
    assert!(
        (card.width() - plain.width() - reserve).abs() <= 0.5,
        "hint widens the card by its keycaps and the gap: {} - {} vs {reserve}",
        card.width(),
        plain.width()
    );
}

#[test]
fn a_long_body_wraps_before_the_hint_shrinks() {
    let theme = tasty_themes::mocha_fallback();
    let (card, caps) = draw(&theme, LONG_HINT, hint());
    let cap = theme.toast_max_width.value();
    assert!(
        card.width() <= cap + 0.5,
        "card {} > cap {cap}",
        card.width()
    );
    let (plain, _) = draw(&theme, "Path copied", hint());
    assert!(card.height() > plain.height(), "long body did not wrap");
    let width: f32 = caps.iter().map(Rect::width).sum::<f32>();
    let (_, short_caps) = draw(&theme, "Path copied", hint());
    let short_width: f32 = short_caps.iter().map(Rect::width).sum::<f32>();
    assert_eq!(width, short_width, "keycaps keep their width");
    // 키캡은 첫 줄 높이에 있다 — 카드 위쪽 절반 안.
    assert!(
        caps[0].center().y < card.center().y,
        "hint {} is not on the first line of card {:?}",
        caps[0].center().y,
        card
    );
}
