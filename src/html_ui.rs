//! HtmlPanel surface의 egui 렌더링. 현재는 URL 라벨만 표시하는 플레이스홀더.
//! 실제 WebView는 별도 네이티브 오버레이로 그려진다.

use crate::model::HtmlPanel;
use crate::theme;

/// Draw the HtmlPanel content region (the WebView is overlaid on top separately).
pub fn draw_html(ui: &mut egui::Ui, panel: &HtmlPanel) {
    let th = theme::theme();
    ui.centered_and_justified(|ui| {
        ui.label(
            egui::RichText::new(&panel.url)
                .color(th.overlay0)
                .size(th.font_size_body),
        );
    });
}
