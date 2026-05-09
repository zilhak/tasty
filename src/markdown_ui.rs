/// Markdown renderer using egui_commonmark.
/// Supports CommonMark spec: tables, checkboxes, links, code blocks, etc.
use crate::settings::EffectiveFont;
use crate::theme;
use crate::ui::font_registry;
use crate::ui::markdown_view::MarkdownView;

/// Draw a Markdown surface from its host-side view: scroll area + content render.
/// The caller (egui_panels) is responsible for refreshing `view.content` from the
/// model's `MarkdownPanel::poll_reload` before calling.
/// `scroll_delta` is applied before the ScrollArea (positive = scroll up).
/// `id_suffix` uniquifies egui ids when multiple panels share a context.
/// `font` carries the markdown surface's effective font settings.
pub fn draw_markdown(
    ui: &mut egui::Ui,
    view: &mut MarkdownView,
    scroll_delta: f32,
    id_suffix: &str,
    font: &EffectiveFont,
) {
    let th = theme::theme();

    // Force the frame content to fill the available width
    ui.set_min_width(ui.available_width());
    egui::ScrollArea::vertical()
        .id_salt(format!("md_scroll_{}", id_suffix))
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            if scroll_delta != 0.0 {
                ui.scroll_with_delta(egui::vec2(0.0, scroll_delta));
            }
            ui.set_min_width(ui.available_width());
            ui.style_mut().interaction.selectable_labels = true;
            ui.style_mut().url_in_tooltip = true;

            apply_font_text_styles(ui, font);

            // Apply theme colors to egui visuals for commonmark rendering
            let visuals = &mut ui.style_mut().visuals;
            visuals.override_text_color = Some(th.subtext1.into());
            visuals.hyperlink_color = th.blue.into();
            visuals.code_bg_color = th.surface0.into();

            let content = view.content.clone();
            egui_commonmark::CommonMarkViewer::new().show(
                ui,
                &mut view.commonmark_cache,
                &content,
            );

            // Trailing space so the last line doesn't visually collide with the
            // panel's bottom inner_margin when scrolled to the end.
            ui.add_space(8.0);
        });
}

/// Render markdown content into an egui Ui (standalone, without MarkdownPanel).
/// Uses a temporary cache per call — for cached rendering, use `draw_markdown` instead.
pub fn render_markdown(ui: &mut egui::Ui, content: &str, font: &EffectiveFont) {
    let th = theme::theme();

    apply_font_text_styles(ui, font);

    let visuals = &mut ui.style_mut().visuals;
    visuals.override_text_color = Some(th.subtext1.into());
    visuals.hyperlink_color = th.blue.into();
    visuals.code_bg_color = th.surface0.into();

    let mut cache = egui_commonmark::CommonMarkCache::default();
    egui_commonmark::CommonMarkViewer::new().show(ui, &mut cache, content);
}

/// Override the standard egui text styles with the markdown surface's font.
/// Headings scale proportionally to the body size.
fn apply_font_text_styles(ui: &mut egui::Ui, font: &EffectiveFont) {
    let body_size = font.font_size.max(1.0);
    let family = font_registry::markdown_family();

    let style = ui.style_mut();
    let text_styles = &mut style.text_styles;
    text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(body_size, family.clone()),
    );
    text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(body_size, family.clone()),
    );
    text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(body_size * 1.5, family.clone()),
    );
    text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new((body_size * 0.85).max(1.0), family),
    );
    // Code blocks stay monospace; size mirrors body for legibility.
    text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(body_size, egui::FontFamily::Monospace),
    );
}
