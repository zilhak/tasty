//! Markdown and HTML file open popups using the PopupContent system.

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::{self, PopupAction, PopupContent, PopupId, PopupScope};

const ITEM_HEIGHT: f32 = 22.0;
const MAX_RECENT: usize = 10;
const HORIZONTAL_MARGIN: f32 = 8.0;
/// Base content height: label + spacing + input row + spacing + button row
const BASE_CONTENT_HEIGHT: f32 = 16.0 + 4.0 + 22.0 + 8.0 + 24.0;
/// Extra height for "Recent Files" label
const RECENT_HEADER_HEIGHT: f32 = 20.0;

fn compute_popup_size(recent_count: usize) -> egui::Vec2 {
    let recent_h = if recent_count > 0 {
        RECENT_HEADER_HEIGHT + (recent_count.min(MAX_RECENT) as f32 * ITEM_HEIGHT) + 4.0
    } else {
        0.0
    };
    egui::vec2(
        360.0,
        popup::TITLE_BAR_HEIGHT + popup::CONTENT_MARGIN * 2.0 + BASE_CONTENT_HEIGHT + recent_h,
    )
}

// ── Markdown Open Popup ──

pub struct MarkdownOpenPopup;

impl MarkdownOpenPopup {
    pub fn new() -> Self { Self }
}

impl PopupContent for MarkdownOpenPopup {
    fn id(&self) -> PopupId { "markdown_open" }

    fn title(&self) -> String {
        t("dialog.markdown.title").to_string()
    }

    fn default_size(&self) -> egui::Vec2 {
        let recent = crate::recent_files::RecentFiles::load();
        compute_popup_size(recent.markdown.len())
    }

    fn scope(&self) -> PopupScope { PopupScope::Window }

    fn close_on_outside_click(&self) -> bool { true }

    fn draw(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
        draw_file_open_content(ui, state, FileType::Markdown)
    }
}

// ── HTML Open Popup ──

pub struct HtmlOpenPopup;

impl HtmlOpenPopup {
    pub fn new() -> Self { Self }
}

impl PopupContent for HtmlOpenPopup {
    fn id(&self) -> PopupId { "html_open" }

    fn title(&self) -> String {
        t("dialog.html.title").to_string()
    }

    fn default_size(&self) -> egui::Vec2 {
        let recent = crate::recent_files::RecentFiles::load();
        compute_popup_size(recent.html.len())
    }

    fn scope(&self) -> PopupScope { PopupScope::Window }

    fn close_on_outside_click(&self) -> bool { true }

    fn draw(&mut self, ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
        draw_file_open_content(ui, state, FileType::Html)
    }
}

// ── Shared drawing logic ──

#[derive(Clone, Copy, PartialEq)]
enum FileType {
    Markdown,
    Html,
}

fn draw_file_open_content(ui: &mut egui::Ui, state: &mut AppState, file_type: FileType) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    // Add horizontal margin
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(HORIZONTAL_MARGIN, 0.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // Escape to close
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        clear_dialog_state(state, file_type);
        return PopupAction::Close;
    }

    let (path_buf, label_key, filter_name, filter_exts) = match file_type {
        FileType::Markdown => {
            let buf = &mut state.dialogs.markdown_open_buffer;
            (buf, "dialog.markdown.path_label", "Markdown", vec!["md"])
        }
        FileType::Html => {
            let buf = &mut state.dialogs.html_open_buffer;
            (buf, "dialog.html.url_label", "HTML", vec!["html", "htm"])
        }
    };

    ui.label(egui::RichText::new(t(label_key)).size(th.font_size_body).color(th.subtext1));
    ui.add_space(4.0);

    // Path input + file picker button
    let mut confirm = false;
    ui.horizontal(|ui| {
        let resp = ui.add_sized(
            [ui.available_width() - 30.0, 22.0],
            egui::TextEdit::singleline(path_buf)
                .font(egui::FontId::proportional(th.font_size_body))
                .margin(egui::Margin::symmetric(4, 2)),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            confirm = true;
        }
        // Auto-focus on first frame
        if !resp.has_focus() && path_buf.is_empty() {
            resp.request_focus();
        }

        if ui.add_sized([26.0, 22.0], egui::Button::new("\u{1F4C2}")).clicked() {
            let mut dialog = rfd::FileDialog::new();
            dialog = dialog.add_filter(filter_name, &filter_exts);
            if let Some(path) = dialog.pick_file() {
                *path_buf = path.to_string_lossy().to_string();
                confirm = true;
            }
        }
    });

    ui.add_space(8.0);

    // Recent files list
    let recent = crate::recent_files::RecentFiles::load();
    let recent_list = match file_type {
        FileType::Markdown => &recent.markdown,
        FileType::Html => &recent.html,
    };

    if !recent_list.is_empty() {
        ui.label(egui::RichText::new(t("dialog.recent_files")).size(th.font_size_caption).color(th.overlay1));
        ui.add_space(2.0);

        egui::ScrollArea::vertical()
            .max_height(MAX_RECENT as f32 * ITEM_HEIGHT)
            .show(ui, |ui| {
                let mut clicked_path: Option<String> = None;
                for entry in recent_list.iter().take(MAX_RECENT) {
                    let display = shorten_path(entry);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ITEM_HEIGHT),
                        egui::Sense::click(),
                    );
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, th.hover_overlay);
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    ui.painter().text(
                        egui::pos2(rect.min.x + 4.0, rect.center().y - th.font_size_caption / 2.0),
                        egui::Align2::LEFT_TOP,
                        &display,
                        egui::FontId::proportional(th.font_size_caption),
                        th.subtext0,
                    );
                    if resp.clicked() {
                        clicked_path = Some(entry.clone());
                    }
                    resp.on_hover_text(entry);
                }
                if let Some(path) = clicked_path {
                    match file_type {
                        FileType::Markdown => state.dialogs.markdown_open_buffer = path,
                        FileType::Html => state.dialogs.html_open_buffer = path,
                    }
                    confirm = true;
                }
            });
    }

    ui.add_space(4.0);

    // OK / Cancel buttons
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("button.cancel")).clicked() {
                clear_dialog_state(state, file_type);
                confirm = false;
                // Mark for close via a local flag
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
        let path_value = match file_type {
            FileType::Markdown => state.dialogs.markdown_open_buffer.clone(),
            FileType::Html => state.dialogs.html_open_buffer.clone(),
        };
        if !path_value.is_empty() {
            apply_open(state, file_type, &path_value);
            clear_dialog_state(state, file_type);
            return PopupAction::Close;
        }
    }

    PopupAction::None
}

fn apply_open(state: &mut AppState, file_type: FileType, path: &str) {
    let mut recent = crate::recent_files::RecentFiles::load();

    match file_type {
        FileType::Markdown => {
            recent.add_markdown(path.to_string());
            if let Some(convert_sid) = state.dialogs.markdown_convert_surface_id.take() {
                state.convert_surface_to_markdown(convert_sid, path.to_string());
            } else {
                let pane_id = state.dialogs.file_open_pane_id.unwrap_or(
                    state.active_workspace().focused_pane,
                );
                state.active_workspace_mut().focused_pane = pane_id;
                let _ = state.add_markdown_tab(path.to_string());
            }
        }
        FileType::Html => {
            let url = if !path.starts_with("http://") && !path.starts_with("https://") && !path.starts_with("file://") {
                format!("file://{}", path)
            } else {
                path.to_string()
            };
            recent.add_html(url.clone());
            if let Some(convert_sid) = state.dialogs.html_convert_surface_id.take() {
                state.convert_surface_to_html(convert_sid, url);
            } else {
                let pane_id = state.dialogs.file_open_pane_id.unwrap_or(
                    state.active_workspace().focused_pane,
                );
                state.active_workspace_mut().focused_pane = pane_id;
                let _ = state.add_html_tab(url);
            }
        }
    }
}

fn clear_dialog_state(state: &mut AppState, file_type: FileType) {
    match file_type {
        FileType::Markdown => {
            state.dialogs.markdown_open_buffer.clear();
            state.dialogs.markdown_convert_surface_id = None;
        }
        FileType::Html => {
            state.dialogs.html_open_buffer.clear();
            state.dialogs.html_convert_surface_id = None;
        }
    }
    state.dialogs.file_open_pane_id = None;
}

fn shorten_path(path: &str) -> String {
    if path.len() <= 50 {
        return path.to_string();
    }
    // Show .../<last 2 components>
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    format!(".../{}", parts[parts.len()-2..].join("/"))
}
