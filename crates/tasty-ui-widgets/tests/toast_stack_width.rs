//! 스택의 카드가 공유 폭 없이 자기 내용 폭을 쓰고, 넓은 스코프에서도 `toast_max_width`를
//! 넘지 않으며, 오른쪽 끝이 앵커에 맞는지 검사한다. 좌측 강조 막대가 `toast_accent_width`
//! 두께인지도 검사한다.

use egui::{Pos2, RawInput, Rect, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;
use tasty_ui_widgets::{ToastEntryView, ToastScopeView, ToastViewProps, draw_toast_scopes};

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
                alpha: 1.0,
            },
            ToastEntryView {
                kind: ToastKind::Success,
                message: SHORT.into(),
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
