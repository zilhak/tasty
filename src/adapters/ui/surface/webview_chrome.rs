//! native WebView 아래의 배경·상태 안내. 페이지 자체는 OS WebView가 그린다.
//! Loading·Failed는 탐색 상태를 표시하고 나머지는 URL 유무로 경로 배경 또는 빈 화면을 고른다.
//! 메뉴·팝업 때문에 native 화면이 숨겨지면 이 배경이 보인다.

use crate::adapters::ui::icons;
use crate::theme;
use crate::webview::NavState;

/// URL 줄에 보일 문자열을 고른다. `label`이 있으면 그것을, 없으면 탐색 가능한 URL만 보인다.
/// raw HTML을 url로 보내는 surface(markdown 등)는 원문 대신 label을 보이거나 줄을 비운다.
pub fn chrome_caption<'a>(url: Option<&'a str>, label: Option<&'a str>) -> Option<&'a str> {
    label.or_else(|| {
        url.filter(|u| {
            u.starts_with("file://") || u.starts_with("http://") || u.starts_with("https://")
        })
    })
}

/// webview-kind surface 의 host chrome 을 패널에 그린다. `nav` 가 Loading/Failed 면
/// 해당 상태 chrome, 그 외(Idle/Done)는 `url` 유무로 boundary(Some)/placeholder(None).
/// URL 줄은 [`chrome_caption`]이 고른 문자열을 보인다.
pub fn draw_webview_chrome(
    ui: &mut egui::Ui,
    url: Option<&str>,
    page_label: Option<&str>,
    nav: NavState,
) {
    let caption = chrome_caption(url, page_label);
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

    // 시안 `HtmlTile`의 내용 padding 16(space-lg)을 좌우에 둬 긴 URL 줄이 가장자리에 닿지 않게 한다.
    let content_rect = panel_rect.shrink2(egui::vec2(th.spacing_lg.value(), 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
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
                    if let Some(caption) = caption {
                        url_line(ui, caption);
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
                    Some(_) => {
                        ui.add(icons::HTML.image(glyph, th.text_muted().to_egui()));
                        ui.add_space(th.spacing_sm.value());
                        label(
                            ui,
                            crate::i18n::t("webview.region"),
                            th.text_muted().to_egui(),
                        );
                        if let Some(caption) = caption {
                            url_line(ui, caption);
                        }
                    }
                },
            }
        },
    );
}

/// URL 줄(mono caption · text-disabled · 한 줄 말줄임).
fn url_line(ui: &mut egui::Ui, text: &str) {
    let th = theme::theme();
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(egui::FontId::monospace(th.font_size_caption.value()))
                .color(th.text_disabled().to_egui()),
        )
        .truncate(),
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

#[cfg(test)]
mod tests {
    use super::chrome_caption;

    #[test]
    fn a_label_wins_over_the_url() {
        assert_eq!(
            chrome_caption(Some("<html>…</html>"), Some("/docs/readme.md")),
            Some("/docs/readme.md")
        );
        assert_eq!(
            chrome_caption(Some("https://tasty.dev"), Some("Tasty")),
            Some("Tasty")
        );
    }

    #[test]
    fn a_navigable_url_shows_without_a_label() {
        for url in [
            "https://tasty.dev",
            "http://localhost:8080",
            "file:///tmp/a.html",
        ] {
            assert_eq!(chrome_caption(Some(url), None), Some(url));
        }
    }

    #[test]
    fn raw_html_without_a_label_shows_no_caption() {
        assert_eq!(
            chrome_caption(Some("<!doctype html><html></html>"), None),
            None
        );
        assert_eq!(chrome_caption(None, None), None);
    }
}
