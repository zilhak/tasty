/// Markdown renderer using egui_commonmark.
/// Supports CommonMark spec: tables, checkboxes, links, code blocks, etc.
use crate::model::MarkdownPanel;
use crate::theme;

/// Draw a MarkdownPanel surface: file reload check + scroll area + content render.
/// `scroll_delta` is applied before the ScrollArea (positive = scroll up).
/// `id_suffix` uniquifies egui ids when multiple panels share a context.
pub fn draw_markdown(
    ui: &mut egui::Ui,
    panel: &mut MarkdownPanel,
    scroll_delta: f32,
    id_suffix: &str,
) {
    panel.check_reload();

    let th = theme::theme();

    // Force the frame content to fill the available width
    ui.set_min_width(ui.available_width());
    if scroll_delta != 0.0 {
        ui.scroll_with_delta(egui::vec2(0.0, scroll_delta));
    }
    egui::ScrollArea::vertical()
        .id_salt(format!("md_scroll_{}", id_suffix))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.style_mut().interaction.selectable_labels = true;
            ui.style_mut().url_in_tooltip = true;

            // Apply theme colors to egui visuals for commonmark rendering
            let visuals = &mut ui.style_mut().visuals;
            visuals.override_text_color = Some(th.subtext1);
            visuals.hyperlink_color = th.blue;
            visuals.code_bg_color = th.surface0;

            let content = panel.content.clone();
            egui_commonmark::CommonMarkViewer::new().show(
                ui,
                &mut panel.commonmark_cache,
                &content,
            );
        });
}

/// Render markdown content into an egui Ui (standalone, without MarkdownPanel).
/// Uses a temporary cache per call — for cached rendering, use `draw_markdown` instead.
pub fn render_markdown(ui: &mut egui::Ui, content: &str) {
    let th = theme::theme();

    let visuals = &mut ui.style_mut().visuals;
    visuals.override_text_color = Some(th.subtext1);
    visuals.hyperlink_color = th.blue;
    visuals.code_bg_color = th.surface0;

    let mut cache = egui_commonmark::CommonMarkCache::default();
    egui_commonmark::CommonMarkViewer::new().show(ui, &mut cache, content);
}
