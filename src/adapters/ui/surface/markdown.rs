//! Markdown surface renderer.
//!
//! Parsing is `pulldown-cmark`; drawing is tasty's own [`render`] module so the design's
//! six-level prose heading hierarchy and `line_height_prose` body leading come straight
//! from `Theme` tokens (egui_commonmark could only scale a single heading style). The
//! load-fail / empty chrome reuses the Explorer centered-state pattern instead of leaking
//! a raw `Error:` string into the body.

pub mod render;
pub mod view;

use crate::i18n::t;
use crate::settings::EffectiveFont;
use crate::theme;
pub use render::LinkClick;
use render::MdStyle;
use view::MarkdownView;

/// Draw a Markdown surface from its host-side view: scroll area + content render.
/// The caller (egui_panels) is responsible for refreshing `view.content` from the
/// model's `MarkdownPanel::poll_reload` before calling.
/// `scroll_delta` is applied before the ScrollArea (positive = scroll up).
/// `id_suffix` uniquifies egui ids when multiple panels share a context.
/// `font` carries the markdown surface's effective font settings.
///
/// Returns a [`LinkClick`] when the user clicked a link this frame, so the caller
/// (`egui_panels`) can route it through the shared file-handler dispatch (filesystem
/// paths) or hand it to the OS (external URLs). The render module itself never dispatches.
pub fn draw_markdown(
    ui: &mut egui::Ui,
    view: &mut MarkdownView,
    scroll_delta: f32,
    id_suffix: &str,
    font: &EffectiveFont,
) -> Option<LinkClick> {
    let th = theme::theme();
    // Per-panel slot so concurrent markdown panels' link clicks never collide in the
    // shared egui temp memory.
    let link_slot = egui::Id::new(("md_link_click", id_suffix));
    let style = MdStyle::new(&th, font, view.base_dir.clone(), link_slot);

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

            if let Some(err) = &view.load_error {
                state_failed(ui, &th, err);
                return;
            }
            if view.content.trim().is_empty() {
                state_empty(ui, &th);
                return;
            }

            render::render(ui, &style, &view.content);

            // Trailing space so the last line doesn't visually collide with the
            // panel's bottom inner_margin when scrolled to the end.
            ui.add_space(8.0);
        });

    // Drain the deferred link click (if a link was clicked during render) and hand it up.
    // (`remove_temp` requires `Default`, which `LinkClick` has no sensible value for, so
    // we read-then-remove.)
    ui.ctx().data_mut(|d| {
        let click = d.get_temp::<LinkClick>(link_slot);
        if click.is_some() {
            d.remove::<LinkClick>(link_slot);
        }
        click
    })
}

/// Load failure — a "Failed to load" title (accent-danger, matching the HTML viewer's
/// error chrome) over the underlying error in a muted mono caption. Design proposed a
/// peach tone; we align to the shared viewer error token (`accent-danger`) for
/// cross-viewer consistency and semantic-token purity.
fn state_failed(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme, detail: &str) {
    centered(ui, |ui| {
        ui.label(
            egui::RichText::new(t("markdown.state.failed"))
                .size(th.font_size_max.value())
                .color(th.accent_danger().to_egui()),
        );
        ui.add_space(th.spacing_xs.value());
        ui.label(
            egui::RichText::new(detail)
                .monospace()
                .size(th.font_size_caption.value())
                .color(th.text_muted().to_egui()),
        );
    });
}

/// Empty file — a centered, muted "This file is empty".
fn state_empty(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme) {
    centered(ui, |ui| {
        ui.label(
            egui::RichText::new(t("markdown.state.empty"))
                .size(th.font_size_body.value())
                .color(th.text_muted().to_egui()),
        );
    });
}

/// Center `content` within the scroll viewport (Explorer centered-state pattern).
fn centered(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let h = ui.available_height().max(1.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.vertical_centered(|ui| content(ui));
        },
    );
}
