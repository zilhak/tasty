//! native WebView 아래의 배경·상태 안내. 페이지 자체는 OS WebView가 그린다.
//! Loading·Failed는 탐색 상태를 표시하고 나머지는 URL 유무로 경로 배경 또는 빈 화면을 고른다.
//! 메뉴·팝업 때문에 native 화면이 숨겨지면 이 배경이 보인다.

use crate::adapters::ui::icons;
use crate::theme;
use crate::webview::NavState;

/// webview-kind surface 의 host chrome 을 패널에 그린다. `nav` 가 Loading/Failed 면
/// 해당 상태 chrome, 그 외(Idle/Done)는 `url` 기반으로 boundary(Some)/placeholder(None).
pub fn draw_webview_chrome(ui: &mut egui::Ui, url: Option<&str>, nav: NavState) {
    let th = theme::theme();
    let panel_rect = ui.max_rect();
    ui.painter()
        .rect_filled(panel_rect, 0.0, th.bg_panel().to_egui());
    ui.painter().rect_stroke(
        panel_rect,
        0.0,
        egui::Stroke::new(th.border_width.value(), th.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );

    let glyph = th.icon_glyph_size_md.value();
    let block_h = glyph + th.spacing_sm.value() + th.font_size_body.value() * 2.0;
    let top_pad = ((panel_rect.height() - block_h) / 2.0).max(th.spacing_xl.value());

    ui.allocate_ui_with_layout(
        panel_rect.size(),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(top_pad);
            match nav {
                NavState::Failed => {
                    ui.add(icons::ALERT_CIRCLE.image(glyph, th.accent_danger().to_egui()));
                    ui.add_space(th.spacing_sm.value());
                    label(
                        ui,
                        crate::i18n::t("webview.error"),
                        th.accent_danger().to_egui(),
                    );
                    if let Some(url) = url {
                        label(ui, url, th.text_disabled().to_egui());
                    }
                }
                NavState::Loading => {
                    tasty_ui_widgets::Spinner::new()
                        .size(th.spinner_size.value())
                        .show(ui, &th);
                    ui.add_space(th.spacing_sm.value());
                    label(
                        ui,
                        crate::i18n::t("webview.loading"),
                        th.text_muted().to_egui(),
                    );
                }
                NavState::Idle | NavState::Done => match url {
                    None => {
                        ui.add(icons::HTML.image(glyph, th.text_disabled().to_egui()));
                        ui.add_space(th.spacing_sm.value());
                        label(
                            ui,
                            crate::i18n::t("webview.no_page"),
                            th.text_muted().to_egui(),
                        );
                    }
                    Some(url) => {
                        ui.add(icons::HTML.image(glyph, th.text_muted().to_egui()));
                        ui.add_space(th.spacing_sm.value());
                        label(
                            ui,
                            crate::i18n::t("webview.region"),
                            th.text_muted().to_egui(),
                        );
                        label(ui, url, th.text_disabled().to_egui());
                    }
                },
            }
        },
    );
}

/// 캡션 한 줄(body · 지정색).
fn label(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    let th = theme::theme();
    ui.label(
        egui::RichText::new(text)
            .size(th.font_size_body.value())
            .color(color),
    );
}
