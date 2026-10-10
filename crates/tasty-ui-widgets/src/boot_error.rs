//! 부팅 실패 화면. 본체와 갤러리가 함께 호출한다.
//!
//! `bg-app`으로 채운 영역 가운데에 진단 카드 하나를 그린다. 카드는 `bg-sidebar` 채움, 1px
//! `border-default`, `corner-radius-lg`, 모달 그림자, `spacing-lg` 안쪽 여백이며 안에 제목·본문·안내·
//! 종료 버튼을 쌓는다. 본문과 안내의 백틱 구간(CLI 명령·옵션)은 code run으로 그린다.
//!
//! 이 화면의 디자인 시안은 아직 없다. 카드 폭과 버튼 크기는 시안 없이 쓰던 값을 그대로 옮겼다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::spacing::{margin_all, vspace};
use crate::tokens::{BOOT_CHROME_CORNER_RADIUS, STRUCT_GAP_2};
use crate::ui_code::ui_copy;

/// 카드 안 내용의 폭. 대응하는 디자인 값이 없다.
pub const BOOT_ERROR_CONTENT_WIDTH: LogicalPx = LogicalPx(460.0);
/// 종료 버튼의 최소 폭. 대응하는 디자인 값이 없다.
const QUIT_BUTTON_WIDTH: LogicalPx = LogicalPx(120.0);
/// 종료 버튼의 최소 높이. 대응하는 디자인 값이 없다.
const QUIT_BUTTON_HEIGHT: LogicalPx = LogicalPx(34.0);

/// 화면 문구. i18n은 호출부가 한다.
pub struct BootErrorView<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub hint: &'a str,
    pub quit: &'a str,
}

/// `ui`의 가용 영역 전체를 `bg-app`으로 채우고 진단 카드를 가운데에 그린다.
/// 종료 버튼이 눌렸으면 true다. 키 입력은 호출부가 판정한다.
pub fn boot_error_screen(ui: &mut egui::Ui, theme: &Theme, view: &BootErrorView<'_>) -> bool {
    let area = ui.available_rect_before_wrap();
    ui.painter()
        .rect_filled(area, 0.0, theme.bg_app().to_egui());

    // 문구 줄바꿈에 따라 높이가 바뀌므로 지난 패스에서 잰 카드 크기로 가운데를 잡는다.
    let size_id = ui.id().with("boot_error_card_size");
    let known = ui
        .data(|d| d.get_temp::<egui::Vec2>(size_id))
        .unwrap_or(egui::Vec2::ZERO);
    let min = area.center() - known / 2.0;
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(min.max(area.min), area.max))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let inner = card_frame(theme).show(&mut content, |ui| {
        // 카드 안 간격은 부모가 바꾼 값이 아니라 컨텍스트 스타일 기본값을 쓴다. 본체는 화면 전체가
        // 이 카드라 기본값 그대로이고, 갤러리 무대가 바꾼 간격이 예제에 새지 않는다.
        ui.spacing_mut().item_spacing = ui.ctx().style().spacing.item_spacing;
        ui.set_width(BOOT_ERROR_CONTENT_WIDTH.value());
        draw_card(ui, theme, view)
    });
    let measured = inner.response.rect.size();
    if (measured - known).length() > 0.5 {
        ui.data_mut(|d| d.insert_temp(size_id, measured));
        ui.ctx().request_discard("boot error card size changed");
    }
    ui.allocate_rect(area, egui::Sense::hover());
    inner.inner
}

fn card_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.bg_sidebar().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius_lg.value())
        .inner_margin(margin_all(theme.spacing_lg))
        // 화면 중앙의 진단 카드에는 모달 그림자를 사용한다.
        .shadow(theme.shadow_modal().to_egui())
}

fn draw_card(ui: &mut egui::Ui, theme: &Theme, view: &BootErrorView<'_>) -> bool {
    let danger = theme.accent_danger().to_egui();
    ui.label(
        egui::RichText::new(view.title)
            .size(theme.font_size_heading.value())
            .strong()
            .color(danger),
    );
    vspace(ui, STRUCT_GAP_2);
    ui_copy(
        ui,
        theme,
        view.body,
        theme.font_size_body,
        theme.text_primary().to_egui(),
    );
    vspace(ui, theme.spacing_md);
    ui_copy(
        ui,
        theme,
        view.hint,
        theme.font_size_caption,
        theme.text_muted().to_egui(),
    );
    vspace(ui, theme.spacing_lg);
    ui.vertical_centered(|ui| {
        ui.add(
            egui::Button::new(
                egui::RichText::new(view.quit)
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.bg_panel().to_egui()),
            )
            .min_size(egui::vec2(
                QUIT_BUTTON_WIDTH.value(),
                QUIT_BUTTON_HEIGHT.value(),
            ))
            .fill(danger)
            // 채움과 같은 색의 stroke가 버튼 면적을 넓힌다.
            // border_width가 바뀌면 테두리뿐 아니라 이 버튼 크기도 함께 확인해야 한다.
            .stroke(egui::Stroke::new(theme.border_width.value(), danger))
            .corner_radius(BOOT_CHROME_CORNER_RADIUS),
        )
        .clicked()
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_settles_at_the_content_width_plus_the_inner_margin() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let view = BootErrorView {
            title: "Could not start the terminal",
            body: "Tasty could not start a terminal. Details: `zsh` not found",
            hint: "Check general.shell in your config.toml.",
            quit: "Quit",
        };
        let ctx = egui::Context::default();
        let screen = egui::vec2(1280.0, 720.0);
        let mut size = egui::Vec2::ZERO;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                ..Default::default()
            };
            drop(ctx.run(input, |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let id = ui.id().with("boot_error_card_size");
                        boot_error_screen(ui, &theme, &view);
                        size = ui
                            .data(|d| d.get_temp::<egui::Vec2>(id))
                            .unwrap_or_default();
                    });
            }));
        }
        let margin = (theme.spacing_lg.value() + theme.border_width.value()) * 2.0;
        assert!(
            (size.x - (BOOT_ERROR_CONTENT_WIDTH.value() + margin)).abs() < 1.0,
            "카드 폭은 내용 폭과 양쪽 안쪽 여백·테두리의 합이다: {size:?}"
        );
        assert!(size.y > QUIT_BUTTON_HEIGHT.value() + margin);
    }
}
