//! 배너 셸과 inset 배치 기하. 본체 BannerManager와 갤러리 예제가 같은 함수를 호출한다.
//! 큐·TTL·레이어 순서는 본체 정책이라 여기 두지 않는다. 규칙은 docs/design/systems/banner.md를 따른다.

use tasty_type_appearance::theme::Theme;

/// Theme의 배너 배경·테두리·모서리·그림자로 카드를 그린다.
/// opacity로 어둡게 표시하며 실제 카드 영역을 반환해 입력 판정에 쓴다.
/// 내용은 배너 문맥([`crate::banner_surface`])에서 그려 배너 위 Secondary 버튼이 한 단계 위 상자를 쓴다.
pub fn banner_shell(
    ui: &mut egui::Ui,
    theme: &Theme,
    opacity: f32,
    content: impl FnOnce(&mut egui::Ui),
) -> egui::Rect {
    let dim = |c: egui::Color32| c.gamma_multiply(opacity);
    let mut shadow = theme.shadow_popover().to_egui();
    shadow.color = shadow.color.gamma_multiply(opacity);
    egui::Frame::new()
        .fill(dim(theme.banner_bg().to_egui()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            dim(theme.banner_border().to_egui()),
        ))
        .corner_radius(theme.corner_radius_lg.value())
        .shadow(shadow)
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            crate::banner_surface(ui, content);
        })
        .response
        .rect
}

/// 배너가 놓인 스코프 폭이 `banner_narrow_below`보다 좁으면 액션을 본문 아래 줄로 내린다.
/// 스코프 크기가 바뀔 때마다 다시 판정하며 내용 길이로 임계값을 바꾸지 않는다.
pub fn banner_is_narrow(scope_width: f32, theme: &Theme) -> bool {
    scope_width < theme.banner_narrow_below().value()
}

/// [글리프 | 글 열 | 액션 묶음] 행에서 글 열이 쓸 폭. 좁으면 액션이 다음 줄이라 폭을 나누지 않는다.
pub(crate) fn action_row_text_width(
    row_w: f32,
    glyph: f32,
    gap: f32,
    actions_w: f32,
    narrow: bool,
) -> f32 {
    let reserved = if narrow { 0.0 } else { gap + actions_w };
    (row_w - glyph - gap - reserved).max(0.0)
}

/// 같은 행의 높이와 액션 묶음 위치(행 왼쪽 위 기준). `first_h`는 글리프·글 열 줄의 높이다.
/// 좁으면 flex-wrap처럼 줄 사이에 같은 gap을 두고 묶음을 본문 왼쪽 가장자리에서 시작한다.
pub(crate) fn action_row_place(
    row_w: f32,
    first_h: f32,
    glyph: f32,
    gap: f32,
    actions: egui::Vec2,
    narrow: bool,
) -> (f32, egui::Vec2) {
    if narrow {
        (
            first_h + gap + actions.y,
            egui::vec2(glyph + gap, first_h + gap),
        )
    } else {
        (first_h.max(actions.y), egui::vec2(row_w - actions.x, 0.0))
    }
}

/// inset 배치에서 배너를 둘 영역. 스코프 콘텐츠 rect의 위·좌·우를 `banner_margin`만큼 줄인다.
/// 높이는 배너가 정하므로 아래 경계는 스코프 바닥까지 열어 둔다.
pub fn inset_banner_zone(scope: egui::Rect, theme: &Theme) -> egui::Rect {
    let margin = theme.banner_margin().value();
    egui::Rect::from_min_max(
        egui::pos2(scope.left() + margin, scope.top() + margin),
        egui::pos2(scope.right() - margin, scope.bottom()),
    )
}

/// inset 배치에서 네이티브 콘텐츠가 차지할 rect. 배너 카드 아래 `banner_inset_gap` 뒤에서 시작한다.
/// 배너가 스코프보다 커도 높이가 음수가 되지 않도록 바닥에 맞춘다.
pub fn inset_content_rect(scope: egui::Rect, banner: egui::Rect, theme: &Theme) -> egui::Rect {
    let top = (banner.bottom() + theme.banner_inset_gap().value()).min(scope.bottom());
    egui::Rect::from_min_max(egui::pos2(scope.left(), top), scope.max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
    }

    #[test]
    fn narrow_starts_just_below_the_token_width() {
        let t = theme();
        let below = t.banner_narrow_below().value();
        assert!(banner_is_narrow(below - 1.0, &t));
        assert!(!banner_is_narrow(below, &t));
    }

    #[test]
    fn a_narrow_action_row_moves_the_group_under_the_text_column() {
        let actions = egui::vec2(120.0, 24.0);
        let (h, at) = action_row_place(300.0, 40.0, 16.0, 8.0, actions, true);
        assert_eq!(at, egui::vec2(24.0, 48.0));
        assert_eq!(h, 72.0);
        assert_eq!(
            action_row_text_width(300.0, 16.0, 8.0, actions.x, true),
            276.0
        );
        let (h, at) = action_row_place(300.0, 16.0, 16.0, 8.0, actions, false);
        assert_eq!(at, egui::vec2(180.0, 0.0));
        assert_eq!(h, 24.0);
        assert_eq!(
            action_row_text_width(300.0, 16.0, 8.0, actions.x, false),
            148.0
        );
    }

    #[test]
    fn inset_leaves_margin_on_all_four_sides() {
        let t = theme();
        let scope = egui::Rect::from_min_size(egui::pos2(0.0, 24.0), egui::vec2(400.0, 300.0));
        let zone = inset_banner_zone(scope, &t);
        let m = t.banner_margin().value();
        assert_eq!(zone.left(), scope.left() + m);
        assert_eq!(zone.right(), scope.right() - m);
        assert_eq!(zone.top(), scope.top() + m);

        let card = egui::Rect::from_min_size(zone.min, egui::vec2(zone.width(), 60.0));
        let content = inset_content_rect(scope, card, &t);
        assert_eq!(content.top(), card.bottom() + t.banner_inset_gap().value());
        assert_eq!(content.left(), scope.left());
        assert_eq!(content.right(), scope.right());
        assert_eq!(content.bottom(), scope.bottom());
    }

    #[test]
    fn inset_content_never_inverts() {
        let t = theme();
        let scope = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 40.0));
        let card = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(184.0, 80.0));
        let content = inset_content_rect(scope, card, &t);
        assert_eq!(content.height(), 0.0);
    }
}
