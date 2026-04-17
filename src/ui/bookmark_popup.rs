//! Bookmark name input popup using the PopupContent system.

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::{self, PopupAction, PopupContent, PopupId, PopupScope};

pub struct BookmarkNamePopup;

impl BookmarkNamePopup {
    pub fn new() -> Self { Self }
}

impl PopupContent for BookmarkNamePopup {
    fn id(&self) -> PopupId { "bookmark_name" }

    fn title(&self) -> String {
        t("explorer.bookmark_add").to_string()
    }

    fn default_size(&self) -> egui::Vec2 {
        egui::vec2(280.0, popup::TITLE_BAR_HEIGHT + popup::CONTENT_MARGIN * 2.0 + 80.0)
    }

    fn scope(&self) -> PopupScope { PopupScope::Window }

    fn close_on_outside_click(&self) -> bool { false }

    fn draw(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
        let th = theme::theme();
        let ctx = ui.ctx().clone();

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.dialogs.bookmark_input = None;
            return PopupAction::Close;
        }

        let Some((_, _, ref mut name_buf)) = state.dialogs.bookmark_input else {
            return PopupAction::Close;
        };

        let margin = 8.0;
        let available = ui.available_rect_before_wrap();
        let inner_rect = available.shrink2(egui::vec2(margin, 0.0));
        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
        let ui = &mut child_ui;

        ui.label(
            egui::RichText::new(t("explorer.bookmark_name_label"))
                .size(th.font_size_body)
                .color(th.subtext1),
        );
        ui.add_space(4.0);

        let resp = ui.add_sized(
            [ui.available_width(), 22.0],
            egui::TextEdit::singleline(name_buf)
                .font(egui::FontId::proportional(th.font_size_body))
                .margin(egui::Margin::symmetric(4, 2)),
        );
        if !resp.has_focus() {
            resp.request_focus();
        }

        let mut confirm = false;
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            confirm = true;
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t("button.cancel")).clicked() {
                    state.dialogs.bookmark_input = None;
                    state.dialogs.file_popup_cancel = true;
                }
                if ui.button(t("button.ok")).clicked() {
                    confirm = true;
                }
            });
        });

        if state.dialogs.file_popup_cancel {
            state.dialogs.file_popup_cancel = false;
            return PopupAction::Close;
        }

        if confirm {
            if let Some((surface_id, path, name)) = state.dialogs.bookmark_input.take() {
                let final_name = if name.trim().is_empty() {
                    std::path::Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone())
                } else {
                    name
                };
                let bookmark = crate::model::Bookmark {
                    name: final_name,
                    path,
                };
                state.with_explorer_mut(surface_id, |panel| {
                    panel.bookmarks.push(bookmark);
                });
            }
            return PopupAction::Close;
        }

        PopupAction::None
    }
}
