//! Markdown 파일 열기 popup. (HTML 은 com.tasty.html plugin 으로 분리됨.)

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::{self, PopupAction};

const ITEM_HEIGHT: f32 = 22.0;
const MAX_RECENT: usize = 10;
const HORIZONTAL_MARGIN: f32 = 8.0;
const ITEM_SPACING_Y: f32 = 4.0;

/// Sizer for markdown open popup — uses AppState.recent_files cache (no disk IO).
pub fn markdown_popup_sizer(
    state: &AppState,
    _engine: &crate::engine_state::EngineState,
) -> egui::Vec2 {
    compute_popup_size(state.recent_files.markdown.len())
}

/// PopupDef::draw_fn entry for markdown open popup.
pub fn draw_markdown_open_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
) -> PopupAction {
    draw_file_open_content(ui, state, engine)
}

fn compute_popup_size(recent_count: usize) -> egui::Vec2 {
    let base = 16.0 + ITEM_SPACING_Y + 4.0 + 22.0 + ITEM_SPACING_Y + 8.0 + 24.0;
    let recent_h = if recent_count > 0 {
        16.0 + ITEM_SPACING_Y
            + 2.0
            + (recent_count.min(MAX_RECENT) as f32 * ITEM_HEIGHT)
            + ITEM_SPACING_Y
            + 4.0
    } else {
        0.0
    };
    egui::vec2(
        360.0,
        popup::TITLE_BAR_HEIGHT + popup::CONTENT_MARGIN * 2.0 + base + recent_h,
    )
}

fn draw_file_open_content(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(HORIZONTAL_MARGIN, 0.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        clear_dialog_state(state);
        return PopupAction::Close;
    }

    let path_buf = &mut state.dialogs.markdown_open_buffer;
    let label_key = "dialog.markdown.path_label";
    let filter_name = "Markdown";
    let filter_exts = vec!["md"];

    ui.label(
        egui::RichText::new(t(label_key))
            .size(th.font_size_body.value())
            .color(th.subtext1),
    );
    ui.add_space(4.0);

    let mut confirm = false;
    ui.horizontal(|ui| {
        let resp = ui.add_sized(
            [ui.available_width() - 30.0, 22.0],
            egui::TextEdit::singleline(path_buf)
                .font(egui::FontId::proportional(th.font_size_body.value()))
                .margin(egui::Margin::symmetric(4, 2)),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            confirm = true;
        }
        if !resp.has_focus() && path_buf.is_empty() {
            resp.request_focus();
        }

        if ui
            .add_sized([26.0, 22.0], egui::Button::new("\u{1F4C2}"))
            .clicked()
        {
            let mut dialog = rfd::FileDialog::new();
            dialog = dialog.add_filter(filter_name, &filter_exts);
            if let Some(path) = dialog.pick_file() {
                *path_buf = path.to_string_lossy().to_string();
                confirm = true;
            }
        }
    });

    if let Some(ref err) = state.dialogs.file_open_error {
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(err.as_str())
                .size(th.font_size_caption.value())
                .color(th.red),
        );
    }

    ui.add_space(8.0);

    let recent_list = state.recent_files.markdown.clone();

    if !recent_list.is_empty() {
        ui.label(
            egui::RichText::new(t("dialog.recent_files"))
                .size(th.font_size_caption.value())
                .color(th.overlay1),
        );
        ui.add_space(2.0);

        egui::ScrollArea::vertical()
            .max_height(MAX_RECENT as f32 * ITEM_HEIGHT)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                let mut clicked_path: Option<String> = None;
                for entry in recent_list.iter().take(MAX_RECENT) {
                    let display = shorten_path(entry);
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), ITEM_HEIGHT),
                        egui::Sense::click(),
                    );
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            th.hover_overlay.to_egui_premultiplied(),
                        );
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    ui.painter().text(
                        egui::pos2(
                            rect.min.x + 4.0,
                            rect.center().y - th.font_size_caption.value() / 2.0,
                        ),
                        egui::Align2::LEFT_TOP,
                        &display,
                        egui::FontId::proportional(th.font_size_caption.value()),
                        th.subtext0.into(),
                    );
                    if resp.clicked() {
                        clicked_path = Some(entry.clone());
                    }
                    resp.on_hover_text(entry);
                }
                if let Some(path) = clicked_path {
                    state.dialogs.markdown_open_buffer = path;
                    confirm = true;
                }
            });
    }

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("button.cancel")).clicked() {
                clear_dialog_state(state);
                confirm = false;
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
        let path_value = state.dialogs.markdown_open_buffer.clone();
        if !path_value.is_empty() {
            let local_path =
                file_uri_to_local_path(&path_value).unwrap_or_else(|| path_value.clone());
            let file_path = std::path::Path::new(&local_path);
            if !file_path.exists() {
                state.dialogs.file_open_error = Some(t("dialog.error.file_not_found").to_string());
                return PopupAction::None;
            }
            state.dialogs.file_open_error = None;
            apply_open(state, engine, &path_value);
            clear_dialog_state(state);
            return PopupAction::Close;
        }
    }

    PopupAction::None
}

fn apply_open(state: &mut AppState, engine: &mut crate::engine_state::EngineState, path: &str) {
    let file_path = file_uri_to_local_path(path).unwrap_or_else(|| path.to_string());
    state.recent_files.add_markdown(file_path.clone());
    if let Some(convert_sid) = state.dialogs.markdown_convert_surface_id.take() {
        state.dispatch_intent(
            crate::intent::Intent::ConvertSurface {
                surface_id: convert_sid,
                target: crate::intent::ConvertTarget::Kind {
                    kind: "markdown".to_string(),
                    params: serde_json::json!({ "file_path": file_path }),
                },
            }
            .from_user_menu("convert/markdown_open"),
        );
    } else {
        let pane_id = state
            .dialogs
            .file_open_pane_id
            .unwrap_or(state.active_workspace(engine).focused_pane);
        state.active_workspace_mut(engine).focused_pane = pane_id;
        state.dispatch_intent(
            crate::intent::Intent::NewTab {
                kind: Some("markdown".to_string()),
                params: serde_json::json!({ "file": file_path }),
            }
            .from_user_menu("file_open/markdown"),
        );
    }
}

fn clear_dialog_state(state: &mut AppState) {
    state.dialogs.markdown_open_buffer.clear();
    state.dialogs.markdown_convert_surface_id = None;
    state.dialogs.file_open_pane_id = None;
    state.dialogs.file_open_error = None;
}

/// Convert a file:// URI to a local filesystem path.
fn file_uri_to_local_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    #[cfg(windows)]
    {
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        Some(rest.replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        Some(rest.to_string())
    }
}

fn shorten_path(path: &str) -> String {
    if path.len() <= 50 {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    format!(".../{}", parts[parts.len() - 2..].join("/"))
}
