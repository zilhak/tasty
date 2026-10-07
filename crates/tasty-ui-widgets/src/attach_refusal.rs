//! 자동 attach 매핑을 연결하지 않았을 때의 Workspace 배너 내용과 사이드바 행 표지.
//! 배너는 행 [글리프 | 제목·본문 | 매핑 지우기 · 닫기]이며 모든 칸을 위쪽에 맞춘다.
//! 문자열은 호출자가 주입한다. 큐·표시 범위는 본체 BannerManager가 정한다.

use tasty_type_appearance::theme::Theme;

use crate::banner::banner_shell;
use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::icon_button::{IconButton, IconButtonVariant};
use crate::tooltip::{Tooltip, tooltip_hover_delay_elapsed};

/// 제목과 본문은 각각 두 줄까지 보이고 나머지는 말줄임한다.
const MAX_TEXT_ROWS: usize = 2;

/// 배너 입력값. 제목은 번역문의 대상 자리 앞뒤와 대상으로 나눠 받아 대상만 mono로 그린다.
pub struct AttachRefusalBannerView<'a> {
    pub title_before: &'a str,
    /// 매핑 대상. 프로필 이름 또는 `host:port` 같은 인라인 대상이다.
    pub target: &'a str,
    pub title_after: &'a str,
    /// 거절 이유와 안내 문장을 이은 본문.
    pub body: &'a str,
    pub remove: &'a str,
    /// 닫기 버튼의 툴팁.
    pub dismiss: &'a str,
}

/// 이번 프레임의 클릭.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttachRefusalBannerClicks {
    pub remove: bool,
    pub dismiss: bool,
}

fn title_galley(
    ui: &egui::Ui,
    theme: &Theme,
    view: &AttachRefusalBannerView<'_>,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let size = theme.banner_title_font_size().value();
    let format = |font_id: egui::FontId| egui::TextFormat {
        font_id,
        color: theme.banner_fg().to_egui(),
        line_height: Some(size * theme.line_height_ui),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        view.title_before,
        0.0,
        format(egui::FontId::proportional(size)),
    );
    job.append(view.target, 0.0, format(egui::FontId::monospace(size)));
    job.append(
        view.title_after,
        0.0,
        format(egui::FontId::proportional(size)),
    );
    job.wrap.max_width = wrap_width;
    job.wrap.max_rows = MAX_TEXT_ROWS;
    ui.fonts(|f| f.layout_job(job))
}

fn body_galley(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let size = theme.banner_body_font_size().value();
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color: theme.text_muted().to_egui(),
            line_height: Some(size * theme.line_height_ui),
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap_width;
    job.wrap.max_rows = MAX_TEXT_ROWS;
    ui.fonts(|f| f.layout_job(job))
}

/// 매핑 지우기 버튼과 닫기 버튼 묶음의 크기. 버튼은 `Button`과 같은 식으로 잰다.
fn actions_size(ui: &egui::Ui, theme: &Theme, remove: &str) -> egui::Vec2 {
    let font = egui::FontId::proportional(ControlSize::Sm.font_size(theme));
    let text_w = ui.fonts(|f| {
        f.layout_no_wrap(remove.to_owned(), font, egui::Color32::PLACEHOLDER)
            .rect
            .width()
    });
    let button_w = text_w + 2.0 * ControlSize::Sm.pad_x(theme);
    let close = theme.icon_button_size_sm().value();
    egui::vec2(
        button_w + theme.spacing_xs.value() + close,
        ControlSize::Sm.height(theme).max(close),
    )
}

/// 배너 셸 안쪽에 내용을 그린다. 본체 BannerManager처럼 셸을 따로 그리는 호출자가 쓴다.
pub fn attach_refusal_banner_content(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &AttachRefusalBannerView<'_>,
) -> AttachRefusalBannerClicks {
    let row_w = ui.available_width();
    let glyph = theme.icon_glyph_size_md.value();
    let gap = theme.banner_gap().value();
    let nudge = theme.banner_glyph_offset().value();
    let text_gap = theme.banner_text_gap().value();
    let actions = actions_size(ui, theme, view.remove);
    let text_w = (row_w - glyph - gap - gap - actions.x).max(0.0);

    let title = title_galley(ui, theme, view, text_w);
    let body = body_galley(ui, theme, view.body, text_w);
    let text_h = title.size().y + text_gap + body.size().y;
    let row_h = (glyph + nudge).max(text_h).max(actions.y);

    let (row, _) = ui.allocate_exact_size(egui::vec2(row_w, row_h), egui::Sense::hover());
    let glyph_rect =
        egui::Rect::from_min_size(row.min + egui::vec2(0.0, nudge), egui::vec2(glyph, glyph));
    tasty_icons::ALERT_TRIANGLE
        .image(glyph, theme.accent_warning().to_egui())
        .paint_at(ui, glyph_rect);

    let text_x = row.left() + glyph + gap;
    let title_h = title.size().y;
    let painter = ui.painter();
    painter.galley(
        egui::pos2(text_x, row.top()),
        title,
        egui::Color32::PLACEHOLDER,
    );
    painter.galley(
        egui::pos2(text_x, row.top() + title_h + text_gap),
        body,
        egui::Color32::PLACEHOLDER,
    );

    let actions_rect =
        egui::Rect::from_min_size(egui::pos2(row.right() - actions.x, row.top()), actions);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(actions_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_xs.value();
    let remove = Button::new(view.remove)
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .show(&mut child, theme)
        .clicked();
    let close = IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(&mut child, theme, &|ui, r, c| {
            tasty_icons::CLOSE.image(r.height(), c).paint_at(ui, r)
        });
    if tooltip_hover_delay_elapsed(ui.ctx(), theme, close.id, close.hovered()) {
        Tooltip::new(view.dismiss)
            .id_source(close.id)
            .show(ui, theme, close.rect);
    }
    AttachRefusalBannerClicks {
        remove,
        dismiss: close.clicked(),
    }
}

/// 셸과 내용을 함께 그린다. 갤러리처럼 BannerManager 없이 배너 한 장을 보이는 곳에서 쓴다.
pub fn attach_refusal_banner(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &AttachRefusalBannerView<'_>,
) -> AttachRefusalBannerClicks {
    let mut clicks = AttachRefusalBannerClicks::default();
    banner_shell(ui, theme, 1.0, |ui| {
        clicks = attach_refusal_banner_content(ui, theme, view);
    });
    clicks
}

/// 워크스페이스 행 끝 칸의 거절 표지 한 변 길이.
pub fn attach_refusal_mark_size(theme: &Theme) -> f32 {
    theme.icon_glyph_size_sm.value()
}

/// 워크스페이스 행 끝 칸에 경고 글리프를 놓고 hover 때 `tooltip`(대상과 이유)을 보인다.
/// 클릭은 받지 않는다. 행이 활성이든 아니든 같은 모양이다.
pub fn attach_refusal_mark(ui: &mut egui::Ui, theme: &Theme, tooltip: &str) -> egui::Response {
    let size = attach_refusal_mark_size(theme);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    tasty_icons::ALERT_TRIANGLE
        .image(size, theme.attach_refusal_glyph().to_egui())
        .paint_at(ui, rect);
    if tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered()) {
        Tooltip::new(tooltip)
            .id_source(resp.id)
            .show(ui, theme, rect);
    }
    resp
}

/// 접힌 레일 아바타의 왼쪽 위 모서리에 경고 칩을 그리고 칩 rect를 돌려준다.
/// 오른쪽 위는 알림 점, 오른쪽 아래는 mirror 칩, 왼쪽 아래는 move 칩이 쓴다. 네 표시는 함께 나올 수 있다.
/// 칩은 아바타 밖으로 border 폭만큼 나온다. `bed`는 칩 바탕색으로, 레일 배경과 같게 전달한다.
pub fn paint_attach_refusal_chip(
    ui: &egui::Ui,
    theme: &Theme,
    avatar: egui::Rect,
    bed: egui::Color32,
) -> egui::Rect {
    let chip = theme.attach_refusal_chip_size().value();
    let outset = theme.border_width.value();
    let rect = egui::Rect::from_min_size(
        egui::pos2(avatar.min.x - outset, avatar.min.y - outset),
        egui::vec2(chip, chip),
    );
    ui.painter().circle_filled(rect.center(), chip * 0.5, bed);
    let glyph = theme.attach_refusal_chip_glyph_size().value();
    let glyph_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    tasty_icons::ALERT_TRIANGLE
        .image(glyph, theme.attach_refusal_glyph().to_egui())
        .paint_at(ui, glyph_rect);
    rect
}

/// 레일 아바타 hover 때 행 표지와 같은 `tooltip`을 아바타 옆에 보인다. 칩은 따로 클릭을 받지 않는다.
pub fn attach_refusal_avatar_tooltip(
    ui: &egui::Ui,
    theme: &Theme,
    avatar: &egui::Response,
    tooltip: &str,
) {
    if tooltip_hover_delay_elapsed(ui.ctx(), theme, avatar.id, avatar.hovered()) {
        Tooltip::new(tooltip)
            .id_source(avatar.id)
            .show(ui, theme, avatar.rect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> AttachRefusalBannerView<'static> {
        AttachRefusalBannerView {
            title_before: "Remote not attached — ",
            target: "127.0.0.1:7420",
            title_after: "",
            body: "This mapping points at this Tasty, so it was not attached. \
                   Change or remove the mapping for this workspace.",
            remove: "Remove mapping",
            dismiss: "Dismiss",
        }
    }

    /// 배너를 폭 `width`로 한 번 그려 셸 rect와 모든 도형을 돌려준다.
    fn render(width: f32) -> (Theme, egui::Rect, Vec<egui::Shape>) {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let ctx = egui::Context::default();
        let mut rect = egui::Rect::NOTHING;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 400.0),
            )),
            ..Default::default()
        };
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let before = ui.cursor().min;
                    attach_refusal_banner(ui, &theme, &view());
                    rect = egui::Rect::from_min_max(before, ui.min_rect().max);
                });
        });
        (
            theme,
            rect,
            out.shapes.into_iter().map(|c| c.shape).collect(),
        )
    }

    #[test]
    fn the_target_is_set_in_the_mono_family() {
        let (_, _, shapes) = render(460.0);
        let mono = shapes.iter().any(|s| match s {
            egui::Shape::Text(t) => t.galley.job.sections.iter().any(|sec| {
                sec.format.font_id.family == egui::FontFamily::Monospace
                    && t.galley.job.text[sec.byte_range.clone()] == *"127.0.0.1:7420"
            }),
            _ => false,
        });
        assert!(mono, "the mapping target is not mono");
    }

    #[test]
    fn a_narrow_banner_stays_inside_its_width() {
        let (_, rect, shapes) = render(280.0);
        // 셸 그림자는 카드 밖으로 번지므로 글자와 버튼 바탕만 본다.
        for s in &shapes {
            let card_ink = match s {
                egui::Shape::Text(_) => true,
                egui::Shape::Rect(r) => r.blur_width == 0.0,
                _ => false,
            };
            if !card_ink {
                continue;
            }
            let b = s.visual_bounding_rect();
            if b.is_positive() {
                assert!(b.right() <= rect.right() + 0.5, "{b:?} past {rect:?}");
            }
        }
    }

    #[test]
    fn the_rail_chip_takes_the_top_left_corner_outside_the_avatar() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let ctx = egui::Context::default();
        let side = theme.sidebar_collapsed_workspace_height.value();
        let avatar = egui::Rect::from_min_size(egui::pos2(40.0, 30.0), egui::Vec2::splat(side));
        let mut chip = egui::Rect::NOTHING;
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                chip = paint_attach_refusal_chip(ui, &theme, avatar, theme.bg_sidebar().into());
            });
        });
        let size = theme.attach_refusal_chip_size().value();
        let outset = theme.border_width.value();
        assert_eq!(chip.size(), egui::vec2(size, size));
        assert_eq!(chip.min, avatar.min - egui::vec2(outset, outset));
        // 바탕 원은 칩 가운데에 칩 지름으로 그려진다. 글리프는 SVG 로더가 없는 시험에서 그려지지 않는다.
        let bed = out.shapes.iter().any(|c| match &c.shape {
            egui::Shape::Circle(circle) => {
                circle.center == chip.center() && (circle.radius - size * 0.5).abs() < 0.01
            }
            _ => false,
        });
        assert!(bed, "chip bed circle at {chip:?}");
    }
}
