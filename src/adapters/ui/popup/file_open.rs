//! Markdown 파일 열기 popup. (HTML 은 com.tasty.html plugin 으로 분리됨.)
//!
//! 구조:
//! - `MarkdownOpenProps` / `MarkdownOpenAction` / `draw_markdown_open_view`: 순수 view
//!   (`tasty-gallery` 가 mock props 로 직접 호출 가능).
//! - `draw_markdown_open_popup`: 본체 wrapper — `AppState`/`CoreState` 에서 props 추출,
//!   view 호출, action 을 mutation/IPC 로 매핑.
//!
//! 분리 패턴 문서: `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`.

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::margin_sym;
use tasty_ui_widgets::tokens::{STRUCT_GAP_2, STRUCT_GAP_4};
use tasty_ui_widgets::vspace;

const ITEM_HEIGHT: f32 = 22.0;
const MAX_RECENT: usize = 10;
const HORIZONTAL_MARGIN: f32 = 8.0;
const ITEM_SPACING_Y: f32 = 4.0;

/// Pure view 의 입력. AppState / CoreState 의 존재를 알지 못한다.
pub struct MarkdownOpenProps<'a> {
    pub theme: &'a Theme,
    /// 경로 입력 버퍼 — view 가 TextEdit 으로 직접 mutate.
    pub path_input: &'a mut String,
    /// 최근 markdown 파일 목록 (이미 MAX_RECENT 이내로 trim 된 상태로 전달 권장).
    pub recents: &'a [String],
    /// 파일 열기 실패 메시지. `None` 이면 표시 안 함.
    pub error: Option<&'a str>,
}

/// view 가 보고하는 사용자 의도. side-effect 는 없다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownOpenAction {
    None,
    /// Escape 키 — popup close.
    Close,
    /// Cancel 버튼 — popup close + 부수상태 reset.
    Cancel,
    /// OK 버튼 / Enter / 최근 항목 클릭 — 현재 `path_input` 으로 열기 시도.
    Confirm,
    /// 📂 버튼 — wrapper 가 OS file dialog 호출.
    BrowseFile,
}

/// markdown_open popup 의 동적 크기. recent 갯수에 비례.
pub fn markdown_open_popup_size(recent_count: usize) -> egui::Vec2 {
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
        popup::title_bar_height() + popup::content_margin() * 2.0 + base + recent_h,
    )
}

/// Sizer for markdown open popup — uses AppState.recent_files cache (no disk IO).
pub fn markdown_popup_sizer(state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    markdown_open_popup_size(state.recent_files.markdown.len())
}

/// 순수 view. 그리기 + Action 산출. AppState/CoreState 접근 금지.
pub fn draw_markdown_open_view(
    ui: &mut egui::Ui,
    props: &mut MarkdownOpenProps<'_>,
) -> MarkdownOpenAction {
    let th = props.theme;
    let ctx = ui.ctx().clone();

    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(HORIZONTAL_MARGIN, 0.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return MarkdownOpenAction::Close;
    }

    ui.label(
        egui::RichText::new(t("dialog.markdown.path_label"))
            .size(th.font_size_body.value())
            .color(th.text_secondary()),
    );
    vspace(ui, th.spacing_xs);

    let mut action = MarkdownOpenAction::None;

    ui.horizontal(|ui| {
        let resp = ui.add_sized(
            [ui.available_width() - 30.0, 22.0],
            egui::TextEdit::singleline(props.path_input)
                .font(egui::FontId::proportional(th.font_size_body.value()))
                // structural: input control-internal nudge (size-4/size-2), spacing 리듬 아님.
                .margin(margin_sym(STRUCT_GAP_4, STRUCT_GAP_2)),
        );
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            action = MarkdownOpenAction::Confirm;
        }
        if !resp.has_focus() && props.path_input.is_empty() {
            resp.request_focus();
        }

        if ui
            .add_sized([26.0, 22.0], egui::Button::new("\u{1F4C2}"))
            .clicked()
        {
            action = MarkdownOpenAction::BrowseFile;
        }
    });

    if let Some(err) = props.error {
        vspace(ui, STRUCT_GAP_2);
        ui.label(
            egui::RichText::new(err)
                .size(th.font_size_caption.value())
                .color(th.accent_danger()),
        );
    }

    vspace(ui, th.spacing_sm);

    if !props.recents.is_empty() {
        ui.label(
            egui::RichText::new(t("dialog.recent_files"))
                .size(th.font_size_caption.value())
                .color(th.text_disabled()),
        );
        vspace(ui, STRUCT_GAP_2);

        egui::ScrollArea::vertical()
            .max_height(MAX_RECENT as f32 * ITEM_HEIGHT)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                let mut clicked_path: Option<String> = None;
                for entry in props.recents.iter().take(MAX_RECENT) {
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
                        th.text_muted().into(),
                    );
                    if resp.clicked() {
                        clicked_path = Some(entry.clone());
                    }
                    resp.on_hover_text(entry);
                }
                if let Some(path) = clicked_path {
                    *props.path_input = path;
                    action = MarkdownOpenAction::Confirm;
                }
            });
    }

    vspace(ui, th.spacing_xs);

    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("button.cancel")).clicked() {
                action = MarkdownOpenAction::Cancel;
            }
            if ui.button(t("button.ok")).clicked() {
                action = MarkdownOpenAction::Confirm;
            }
        });
    });

    action
}

/// PopupDef::draw_fn entry for markdown open popup.
pub fn draw_markdown_open_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let recents = state.recent_files.markdown.clone();
    let mut path_input = std::mem::take(&mut state.dialogs.markdown_open_buffer);
    let error_owned = state.dialogs.file_open_error.clone();

    let action = {
        let theme_guard = theme::theme();
        let mut props = MarkdownOpenProps {
            theme: &theme_guard,
            path_input: &mut path_input,
            recents: &recents,
            error: error_owned.as_deref(),
        };
        draw_markdown_open_view(ui, &mut props)
    };

    state.dialogs.markdown_open_buffer = path_input;

    match action {
        MarkdownOpenAction::None => PopupAction::None,
        MarkdownOpenAction::Close => {
            clear_dialog_state(state);
            PopupAction::Close
        }
        MarkdownOpenAction::Cancel => {
            clear_dialog_state(state);
            PopupAction::Close
        }
        MarkdownOpenAction::BrowseFile => {
            let mut dialog = rfd::FileDialog::new();
            dialog = dialog.add_filter("Markdown", &["md"]);
            if let Some(path) = dialog.pick_file() {
                state.dialogs.markdown_open_buffer = path.to_string_lossy().to_string();
                try_confirm(state, engine)
            } else {
                PopupAction::None
            }
        }
        MarkdownOpenAction::Confirm => try_confirm(state, engine),
    }
}

fn try_confirm(state: &mut AppState, engine: &mut crate::core::CoreState) -> PopupAction {
    let path_value = state.dialogs.markdown_open_buffer.clone();
    if path_value.is_empty() {
        return PopupAction::None;
    }
    let local_path = file_uri_to_local_path(&path_value).unwrap_or_else(|| path_value.clone());
    let file_path = std::path::Path::new(&local_path);
    if !file_path.exists() {
        state.dialogs.file_open_error = Some(t("dialog.error.file_not_found").to_string());
        return PopupAction::None;
    }
    state.dialogs.file_open_error = None;
    apply_open(state, engine, &path_value);
    clear_dialog_state(state);
    PopupAction::Close
}

fn apply_open(state: &mut AppState, engine: &mut crate::core::CoreState, path: &str) {
    let file_path = file_uri_to_local_path(path).unwrap_or_else(|| path.to_string());
    state.recent_files.add_markdown(file_path.clone());
    if let Some(convert_sid) = state.dialogs.markdown_convert_surface_id.take() {
        state.dispatch_intent(
            crate::intent::Intent::ConvertSurface {
                surface_id: convert_sid,
                target: crate::intent::ConvertTarget::Kind {
                    cwd: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn run_view_once(inject_events: Vec<egui::Event>) -> MarkdownOpenAction {
        let ctx = egui::Context::default();
        let theme = tasty_themes::mocha_fallback();
        let mut path_input = String::new();
        let recents: Vec<String> = Vec::new();

        let input = egui::RawInput {
            events: inject_events,
            ..Default::default()
        };

        let mut captured = MarkdownOpenAction::None;
        let _full_output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut props = MarkdownOpenProps {
                    theme: &theme,
                    path_input: &mut path_input,
                    recents: &recents,
                    error: None,
                };
                captured = draw_markdown_open_view(ui, &mut props);
            });
        });
        captured
    }

    #[test]
    fn escape_key_returns_close() {
        let action = run_view_once(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert_eq!(action, MarkdownOpenAction::Close);
    }

    #[test]
    fn no_input_returns_none() {
        let action = run_view_once(Vec::new());
        assert_eq!(action, MarkdownOpenAction::None);
    }

    #[test]
    fn popup_size_grows_with_recents() {
        let zero = markdown_open_popup_size(0);
        let many = markdown_open_popup_size(10);
        assert!(many.y > zero.y);
        assert_eq!(zero.x, 360.0);
        assert_eq!(many.x, 360.0);
    }

    #[test]
    fn popup_size_caps_at_max_recent() {
        let ten = markdown_open_popup_size(10);
        let hundred = markdown_open_popup_size(100);
        assert_eq!(ten.y, hundred.y);
    }
}
