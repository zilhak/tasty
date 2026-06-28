//! Webview(html) surface 의 **host chrome** egui 렌더.
//!
//! `rendering = "webview"` kind 의 surface 는 host 가 OS-level native WebView overlay
//! 를 위에 붙인다(`src/host_api/webview/*`, `engine/surface_registry/webview_kind.rs`).
//! 실제 페이지 픽셀은 OS WebView 가 그리므로 콘텐츠는 토큰 무관 — host 가 토큰으로
//! 책임지는 것은 overlay 가 붙기 전/숨겨질 때의 *chrome* 뿐이다.
//!
//! 갤러리 specimen `crates/tasty-gallery/src/catalog/components/html_chrome.rs` 의
//! boundary/placeholder 상태를 동일 토큰으로 구조 전사한다. 두 상태는 backend 변경 없이
//! host 가 webview 가시성을 제어하는 것만으로 노출된다:
//!
//! - **placeholder** — URL 미지정(navigation 전): overlay 가 아예 생성되지 않아
//!   이 chrome 이 그대로 보인다(GLOBE text-disabled + "No page loaded").
//! - **boundary** — URL 지정됨: overlay 가 영역을 덮지만, egui overlay(메뉴/팝업/
//!   다이얼로그) 가 열려 webview 가 일시 숨겨질 때 이 backdrop 이 보인다(GLOBE
//!   text-muted + "WebView region" + url).
//!
//! loading/error 상태는 navigation 생명주기 신호(start/finish/fail)가 필요한데 현재
//! 3개 backend 어디에도 콜백이 배선돼 있지 않아 본 모듈에서 다루지 않는다(보류).
//!
//! 색·치수·폰트는 전부 `Theme` 토큰. 문구는 `t()`.

use crate::adapters::ui::icons;
use crate::theme;

/// webview-kind surface 의 host chrome 을 패널에 그린다. `url` 이 Some 이면 boundary
/// (overlay backdrop), None 이면 placeholder(no page loaded).
pub fn draw_webview_chrome(ui: &mut egui::Ui, url: Option<&str>) {
    let th = theme::theme();
    let panel_rect = ui.max_rect();
    // bg-panel 타일 배경 + 1px border-default 경계(specimen tile 과 동일 토큰).
    ui.painter()
        .rect_filled(panel_rect, 0.0, th.bg_panel().to_egui());
    ui.painter().rect_stroke(
        panel_rect,
        0.0,
        egui::Stroke::new(th.border_width.value(), th.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );

    let glyph = th.icon_glyph_size_md.value();
    // 콘텐츠 블록(글리프 + 캡션)을 세로 가운데에 배치.
    let block_h = glyph + th.spacing_sm.value() + th.font_size_body.value() * 2.0;
    let top_pad = ((panel_rect.height() - block_h) / 2.0).max(th.spacing_xl.value());

    ui.allocate_ui_with_layout(
        panel_rect.size(),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(top_pad);
            match url {
                None => {
                    // placeholder — no URL.
                    ui.add(icons::GLOBE.image(glyph, th.text_disabled().to_egui()));
                    ui.add_space(th.spacing_sm.value());
                    label(ui, crate::i18n::t("webview.no_page"), th.text_muted().to_egui());
                }
                Some(url) => {
                    // boundary — webview region backdrop.
                    ui.add(icons::GLOBE.image(glyph, th.text_muted().to_egui()));
                    ui.add_space(th.spacing_sm.value());
                    label(ui, crate::i18n::t("webview.region"), th.text_muted().to_egui());
                    label(ui, url, th.text_disabled().to_egui());
                }
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
