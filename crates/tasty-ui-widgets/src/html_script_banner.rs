//! HTML surface의 스크립트 차단 안내 배너와 탭 스트립 마커.
//! 배너는 inset 배치로 그리므로 WebView를 덮지 않는다. 문자열은 호출자가 주입한다.
//! 상자 경계는 셸 > 행 [글리프 | 본문 | 액션] + 우상단 절대 위치 닫기 슬롯이다.
//! 좁은 surface에서는 액션이 본문 왼쪽 가장자리에 맞춰 다음 줄로 내려간다.

use tasty_type_appearance::theme::Theme;

use crate::banner::banner_shell;
use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::icon_button::{IconButton, IconButtonVariant};
use crate::spinner::Spinner;
use crate::tooltip::{Tooltip, TooltipPlacement};

/// 제목과 본문은 각각 두 줄까지 보이고 나머지는 말줄임한다.
const MAX_TEXT_ROWS: usize = 2;

/// surface 폭이 `banner_narrow_below`보다 좁으면 액션을 본문 아래 줄로 내린다.
/// surface 크기가 바뀔 때마다 다시 판정하며 내용 길이로 임계값을 바꾸지 않는다.
pub fn html_script_banner_is_narrow(surface_width: f32, theme: &Theme) -> bool {
    surface_width < theme.banner_narrow_below().value()
}

/// 배너가 표시하는 단계. 닫힘·허용 뒤에는 배너 대신 탭 마커가 남는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlScriptBannerState {
    /// 스크립트가 차단된 기본 상태. 액션 버튼을 보이고 hover 때 닫기 버튼을 드러낸다.
    Blocked,
    /// 허용을 누른 뒤 한 번 다시 읽는 중. 액션 자리를 스피너와 라벨이 대신하고 닫기 버튼을 숨긴다.
    Reloading,
}

/// 배너 입력값. 문자열은 본체가 번역해 넣고 갤러리는 확정 영어 문안을 넣는다.
pub struct HtmlScriptBannerView<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub action: &'a str,
    pub reloading: &'a str,
    pub state: HtmlScriptBannerState,
    /// surface가 좁아 액션을 본문 아래 줄로 내린다.
    pub narrow: bool,
    /// 포인터와 무관하게 hover 상태로 그린다. 상태를 나란히 보여 주는 갤러리에서만 쓴다.
    pub force_hover: bool,
}

/// 그린 카드 영역과 이번 프레임의 클릭.
pub struct HtmlScriptBannerOutput {
    pub rect: egui::Rect,
    pub hovered: bool,
    pub allow_clicked: bool,
    pub dismiss_clicked: bool,
}

fn text_galley(
    ui: &egui::Ui,
    theme: &Theme,
    text: &str,
    size: f32,
    color: egui::Color32,
    wrap_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            line_height: Some(size * theme.line_height_ui),
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap_width;
    job.wrap.max_rows = MAX_TEXT_ROWS;
    ui.fonts(|f| f.layout_job(job))
}

/// 액션 자리 크기. 버튼은 `Button`과 같은 식으로 잰다.
fn action_size(ui: &egui::Ui, theme: &Theme, view: &HtmlScriptBannerView<'_>) -> egui::Vec2 {
    let height = ControlSize::Sm.height(theme);
    let width = match view.state {
        HtmlScriptBannerState::Blocked => {
            let font = egui::FontId::proportional(ControlSize::Sm.font_size(theme));
            let w = ui.fonts(|f| {
                f.layout_no_wrap(view.action.to_owned(), font, egui::Color32::PLACEHOLDER)
                    .rect
                    .width()
            });
            w + 2.0 * ControlSize::Sm.pad_x(theme)
        }
        HtmlScriptBannerState::Reloading => {
            let font = egui::FontId::proportional(theme.banner_body_font_size().value());
            let w = ui.fonts(|f| {
                f.layout_no_wrap(view.reloading.to_owned(), font, egui::Color32::PLACEHOLDER)
                    .rect
                    .width()
            });
            theme.icon_glyph_size_sm.value() + theme.spacing_sm.value() + w
        }
    };
    egui::vec2(width, height)
}

fn draw_action(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &HtmlScriptBannerView<'_>,
    rect: egui::Rect,
) -> bool {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    match view.state {
        HtmlScriptBannerState::Blocked => Button::new(view.action)
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Sm)
            .show(&mut child, theme)
            .clicked(),
        HtmlScriptBannerState::Reloading => {
            let spin = theme.icon_glyph_size_sm.value();
            let spin_rect = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.center().y - spin / 2.0),
                egui::vec2(spin, spin),
            );
            let mut spin_ui = child.new_child(egui::UiBuilder::new().max_rect(spin_rect));
            Spinner::new().size(spin).show(&mut spin_ui, theme);
            child.painter().text(
                egui::pos2(
                    spin_rect.right() + theme.spacing_sm.value(),
                    rect.center().y,
                ),
                egui::Align2::LEFT_CENTER,
                view.reloading,
                egui::FontId::proportional(theme.banner_body_font_size().value()),
                theme.text_muted().to_egui(),
            );
            false
        }
    }
}

/// 행 내용을 그리고 액션 클릭 여부를 돌려준다. `ui`는 셸 안쪽 여백을 뺀 영역이다.
fn draw_row(ui: &mut egui::Ui, theme: &Theme, view: &HtmlScriptBannerView<'_>) -> bool {
    let reloading = view.state == HtmlScriptBannerState::Reloading;
    // 오른쪽은 닫기 슬롯 몫을 비운다. 셸이 이미 좌우 padding을 넣었으므로 그 몫은 뺀다.
    let reserve =
        (theme.icon_button_size_sm().value() + theme.spacing_sm.value() + theme.spacing_xs.value()
            - theme.banner_padding_x().value())
        .max(0.0);
    let row_w = (ui.available_width() - reserve).max(0.0);
    let glyph = theme.icon_glyph_size_md.value();
    let gap = theme.banner_gap().value();
    let nudge = theme.banner_glyph_offset().value();
    let text_gap = theme.banner_text_gap().value();
    let action = action_size(ui, theme, view);

    let body_w = if view.narrow {
        row_w - glyph - gap
    } else {
        row_w - glyph - gap - gap - action.x
    }
    .max(0.0);
    let text_opacity = if reloading {
        theme.opacity_dimmed()
    } else {
        1.0
    };
    let title = text_galley(
        ui,
        theme,
        view.title,
        theme.banner_title_font_size().value(),
        theme.banner_fg().to_egui().gamma_multiply(text_opacity),
        body_w,
    );
    let body = text_galley(
        ui,
        theme,
        view.body,
        theme.banner_body_font_size().value(),
        theme.text_muted().to_egui().gamma_multiply(text_opacity),
        body_w,
    );
    let text_h = title.size().y + text_gap + body.size().y;

    let first_line_h = (glyph + nudge).max(text_h);
    let (row_h, text_y, action_pos) = if view.narrow {
        // flex-wrap: 줄 사이에도 같은 gap을 두고 액션은 본문 왼쪽 가장자리에서 시작한다.
        (
            first_line_h + gap + action.y,
            0.0,
            egui::vec2(glyph + gap, first_line_h + gap),
        )
    } else {
        let h = first_line_h.max(action.y);
        (
            h,
            (h - text_h) / 2.0,
            egui::vec2(row_w - action.x, (h - action.y) / 2.0),
        )
    };

    let (row, _) = ui.allocate_exact_size(egui::vec2(row_w, row_h), egui::Sense::hover());
    let glyph_rect =
        egui::Rect::from_min_size(row.min + egui::vec2(0.0, nudge), egui::vec2(glyph, glyph));
    tasty_icons::LOCK
        .image(glyph, theme.html_script_banner_glyph().to_egui())
        .paint_at(ui, glyph_rect);

    let text_x = row.left() + glyph + gap;
    let painter = ui.painter();
    let title_h = title.size().y;
    painter.galley(
        egui::pos2(text_x, row.top() + text_y),
        title,
        egui::Color32::PLACEHOLDER,
    );
    painter.galley(
        egui::pos2(text_x, row.top() + text_y + title_h + text_gap),
        body,
        egui::Color32::PLACEHOLDER,
    );

    let action_rect = egui::Rect::from_min_size(row.min + action_pos, action);
    draw_action(ui, theme, view, action_rect)
}

/// 스크립트 차단 안내 배너를 현재 `ui` 폭에 맞춰 그린다. inset 배치는 호출자가
/// [`crate::inset_banner_zone`]과 [`crate::inset_content_rect`]로 정한다.
pub fn html_script_banner(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &HtmlScriptBannerView<'_>,
) -> HtmlScriptBannerOutput {
    let mut allow_clicked = false;
    let rect = banner_shell(ui, theme, 1.0, |ui| {
        allow_clicked = draw_row(ui, theme, view);
    });
    let hovered = view.force_hover || ui.rect_contains_pointer(rect);
    let mut dismiss_clicked = false;
    if hovered && view.state == HtmlScriptBannerState::Blocked {
        let side = theme.icon_button_size_sm().value();
        let slot = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - theme.spacing_sm.value() - side,
                rect.top() + theme.banner_padding_y().value(),
            ),
            egui::vec2(side, side),
        );
        let mut slot_ui = ui.new_child(egui::UiBuilder::new().max_rect(slot));
        dismiss_clicked = IconButton::new()
            .variant(IconButtonVariant::Ghost)
            .size(ControlSize::Sm)
            .show(&mut slot_ui, theme, &|ui, r, c| {
                tasty_icons::CLOSE.image(r.height(), c).paint_at(ui, r)
            })
            .clicked();
    }
    HtmlScriptBannerOutput {
        rect,
        hovered,
        allow_clicked,
        dismiss_clicked,
    }
}

/// 탭 라벨 뒤에 붙는 스크립트 상태 표지.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtmlScriptMarkerKind {
    /// 차단된 채 배너를 닫았다. 클릭하면 배너를 다시 보인다(직접 허용하지 않는다).
    Blocked,
    /// 이 문서에 스크립트를 허용했다. 툴팁만 있고 클릭할 수 없다.
    Allowed,
}

/// 마커를 `html_script_marker_hit` 정사각 칸 가운데에 그린다. hover 때 `tooltip`을 보인다.
/// Blocked 마커만 클릭을 받고 hover 채움을 가진다. Allowed 마커는 툴팁만 있다.
pub fn html_script_marker(
    ui: &mut egui::Ui,
    theme: &Theme,
    kind: HtmlScriptMarkerKind,
    tooltip: &str,
) -> egui::Response {
    let hit = theme.html_script_marker_hit().value();
    let size = theme.html_script_marker_size().value();
    let (glyph, color, sense) = match kind {
        HtmlScriptMarkerKind::Blocked => (
            tasty_icons::LOCK,
            theme.html_script_marker_fg(),
            egui::Sense::click(),
        ),
        HtmlScriptMarkerKind::Allowed => (
            tasty_icons::SCRIPT,
            theme.html_script_marker_allowed_fg(),
            egui::Sense::hover(),
        ),
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(hit, hit), sense);
    if kind == HtmlScriptMarkerKind::Blocked && resp.hovered() {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            theme.html_script_marker_hover_bg().to_egui_premultiplied(),
        );
    }
    let glyph_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(size, size));
    glyph.image(size, color.to_egui()).paint_at(ui, glyph_rect);
    if resp.hovered() {
        Tooltip::new(tooltip)
            .placement(TooltipPlacement::Bottom)
            .id_source(resp.id)
            .show(ui, theme, rect);
    }
    if kind == HtmlScriptMarkerKind::Blocked {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_starts_just_below_the_token_width() {
        let t = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let below = t.banner_narrow_below().value();
        assert!(html_script_banner_is_narrow(below - 1.0, &t));
        assert!(!html_script_banner_is_narrow(below, &t));
        assert!(!html_script_banner_is_narrow(below + 1.0, &t));
    }
}
