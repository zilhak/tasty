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
