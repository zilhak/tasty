//! 이름 변경 팝업. view는 입력값과 버퍼를 받아 액션을 반환하고
//! 호출자가 실제 대상 변경을 적용한다. 갤러리도 같은 view를 사용한다.

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::{MainViewState, RenameTarget};
use crate::theme;
use crate::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::margin_sym;
use tasty_ui_widgets::tokens::{STRUCT_GAP_2, STRUCT_GAP_4};
use tasty_ui_widgets::vspace;
use tasty_ui_widgets::{Button, ButtonVariant};

/// 오류 줄이 있으면 직전 프레임에 잰 그 높이만큼 기본 크기보다 높인다.
pub fn rename_popup_sizer(
    state: &MainViewState,
    _engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> egui::Vec2 {
    rename_popup_default_size() + egui::vec2(0.0, state.dialogs.rename_error_height)
}

pub fn rename_popup_default_size() -> egui::Vec2 {
    egui::vec2(
        280.0,
        (popup::title_bar_height() + popup::content_margin().scaled(2.0) + LogicalPx(64.0)).value(),
    )
}

pub fn rename_popup_title(
    state: &MainViewState,
    _engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> String {
    state
        .dialogs
        .rename
        .as_ref()
        .map(|(target, _)| t(target.heading_key()).to_string())
        .unwrap_or_else(|| t("rename_dialog.tab_heading").to_string())
}

/// view 입력. 앱 상태 대신 전달된 텍스트 버퍼를 직접 편집한다.
pub struct RenamePopupProps<'a> {
    pub theme: &'a Theme,
    pub buffer: &'a mut String,
    pub save_label: &'a str,
    pub cancel_label: &'a str,
    pub body_font_size: f32,
    /// 인라인 검증 에러 메시지(카테고리 생성/이름변경). `Some` 이면 필드 아래 danger
    /// 라인으로 표시. 비카테고리 대상은 항상 `None`.
    pub error: Option<&'a str>,
    /// false이면 Save와 Enter 확정을 모두 막는다.
    pub save_enabled: bool,
    /// view 가 채우는 값. 이번 프레임에 그린 오류 줄이나 대기 줄(앞 간격 포함)의 높이이고 없으면 0 이다.
    /// 호출자가 팝업 높이를 이만큼 늘려 버튼 줄이 잘리지 않게 한다.
    pub error_height: f32,
    /// 원격의 답을 기다리는 동안의 대기 줄 문구("Renaming on {host}…"). `Some` 이면 입력을 읽기 전용으로
    /// 두고 Spinner 줄을 보이며, 두 버튼과 Enter·Esc 를 막는다.
    pub waiting: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenamePopupAction {
    None,
    Cancel,
    Confirm(String),
}

/// 어떤 경로로 닫혀도 대상과 입력 버퍼를 정리한다.
pub fn on_close_rename_popup(
    _ctx: &egui::Context,
    state: &mut MainViewState,
    _engine: &crate::runtime::engine_read::EngineRead<'_>,
) {
    state.dialogs.rename = None;
    state.dialogs.rename_error_height = 0.0;
    state.dialogs.rename_waiting = None;
}

pub fn draw_rename_popup(
    ui: &mut egui::Ui,
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> PopupAction {
    let th = theme::theme();

    let Some((ref target, _)) = state.dialogs.rename else {
        return PopupAction::Close;
    };

    let is_add_favorite = matches!(target, RenameTarget::ExplorerAddFavorite { .. });

    if !rename_target_exists(target, engine)
        || state
            .dialogs
            .rename_waiting
            .as_ref()
            .is_some_and(|w| w.done)
    {
        state.dialogs.rename = None;
        return PopupAction::Close;
    }
    let waiting = state
        .dialogs
        .rename_waiting
        .as_ref()
        .map(|w| crate::i18n::t_fmt("explorer.remote.renaming", &w.host));

    // 직전 프레임의 입력으로 카테고리 이름과 탐색기 항목 이름을 검사한다. 다른 대상에는 이 제한이 없다.
    let (validation_error, save_enabled) = {
        let buffer = &state.dialogs.rename.as_ref().unwrap().1;
        match target {
            RenameTarget::ExplorerEntry {
                surface_id, path, ..
            } => explorer_validation(buffer, explorer_name_rules(engine, *surface_id, path)),
            _ => category_validation(target, buffer, engine),
        }
    };

    let margin = 8.0;
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(margin, 2.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let inner = &mut child_ui;

    let save_label = if is_add_favorite {
        t("explorer.popup.add_favorite.add")
    } else {
        t("button.save")
    };
    let cancel_label = t("button.cancel");

    let action = {
        let buffer = &mut state.dialogs.rename.as_mut().unwrap().1;
        let mut props = RenamePopupProps {
            theme: &th,
            buffer,
            save_label,
            cancel_label,
            body_font_size: th.font_size_body.value(),
            error: validation_error.as_deref(),
            save_enabled,
            error_height: 0.0,
            waiting: waiting.as_deref(),
        };
        let action = draw_rename_popup_view(inner, &mut props);
        let error_height = props.error_height;
        (action, error_height)
    };
    let (action, error_height) = action;
    state.dialogs.rename_error_height = error_height;

    match action {
        RenamePopupAction::None => PopupAction::None,
        RenamePopupAction::Cancel => {
            state.dialogs.rename = None;
            PopupAction::Close
        }
        RenamePopupAction::Confirm(buffer) => {
            match request_remote_rename(state, engine, &buffer) {
                Some(Some(waiting)) => {
                    state.dialogs.rename_waiting = Some(waiting);
                    return PopupAction::None;
                }
                // 받지 않은 이유는 그 칸이 알린다.
                Some(None) => {
                    state.dialogs.rename = None;
                    return PopupAction::Close;
                }
                None => {}
            }
            let (target, _) = state.dialogs.rename.take().unwrap();
            apply_rename(state, engine, target, buffer);
            PopupAction::Close
        }
    }
}

/// Enter/Save는 확정, Escape/Cancel은 취소다. Enter는 입력 필드 포커스가 필요하다.
/// 같은 프레임에 둘 다 발생하면 확정을 우선한다.
pub fn draw_rename_popup_view(
    ui: &mut egui::Ui,
    props: &mut RenamePopupProps<'_>,
) -> RenamePopupAction {
    let ctx = ui.ctx().clone();
    let waiting = props.waiting.is_some();
    if !waiting && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return RenamePopupAction::Cancel;
    }

    // 기다리는 동안에는 보낸 이름을 Input readOnly 모양으로 보이고 고치지 못하게 한다.
    // egui는 편집할 수 없는 TextEdit의 프레임을 그리지 않으므로 칸 크기는 그대로 두고 상자를 직접 그린다.
    let mut sent = props.buffer.as_str();
    let bed = ui.painter().add(egui::Shape::Noop);
    let field = if waiting {
        egui::TextEdit::singleline(&mut sent)
            .frame(false)
            .text_color(props.theme.input_readonly_fg().to_egui())
    } else {
        egui::TextEdit::singleline(props.buffer)
    };
    let resp = ui.add_sized(
        [ui.available_width(), 22.0],
        field
            .font(egui::FontId::proportional(props.body_font_size))
            // 입력 위젯 안의 위치 보정이며 레이아웃 간격 토큰과는 별개다.
            .margin(margin_sym(STRUCT_GAP_4, STRUCT_GAP_2)),
    );

    if waiting {
        let radius = props.theme.input_radius().value();
        ui.painter().set(
            bed,
            egui::Shape::rect_filled(resp.rect, radius, props.theme.input_readonly_bg().to_egui()),
        );
        ui.painter().rect_stroke(
            resp.rect,
            radius,
            egui::Stroke::new(
                props.theme.border_width.value(),
                props.theme.input_readonly_border().to_egui(),
            ),
            egui::StrokeKind::Inside,
        );
    }

    if !resp.has_focus() {
        resp.request_focus();
    }

    if resp.gained_focus()
        && let Some(mut text_state) = egui::TextEdit::load_state(&ctx, resp.id)
    {
        let len = props.buffer.chars().count();
        text_state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(len),
            )));
        text_state.store(&ctx, resp.id);
    }

    let mut confirm = false;
    let mut cancel = false;

    if !waiting
        && props.save_enabled
        && resp.has_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        confirm = true;
    }

    props.error_height = 0.0;
    if let Some(text) = props.waiting {
        let top = ui.cursor().top();
        vspace(ui, props.theme.spacing_sm);
        waiting_line(ui, props.theme, text);
        props.error_height = ui.cursor().top() - top;
    } else if let Some(err) = props.error {
        let top = ui.cursor().top();
        vspace(ui, props.theme.spacing_xs);
        ui.colored_label(props.theme.accent_danger(), err);
        props.error_height = ui.cursor().top() - top;
    }

    vspace(ui, props.theme.spacing_sm);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if Button::new(props.save_label)
                .variant(ButtonVariant::Primary)
                .enabled(props.save_enabled && !waiting)
                .show(ui, props.theme)
                .clicked()
            {
                confirm = true;
            }
            if Button::new(props.cancel_label)
                .variant(ButtonVariant::Ghost)
                .enabled(!waiting)
                .show(ui, props.theme)
                .clicked()
            {
                cancel = true;
            }
        });
    });

    if confirm {
        return RenamePopupAction::Confirm(props.buffer.clone());
    }
    if cancel {
        return RenamePopupAction::Cancel;
    }
    RenamePopupAction::None
}

/// 대기 줄: `icon-size-sm` Spinner 와 caption text-muted 문구를 `space-sm` 간격으로 둔다.
fn waiting_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        tasty_ui_widgets::Spinner::new()
            .size(theme.icon_glyph_size_sm.value())
            .show(ui, theme);
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
    ui.ctx().request_repaint();
}

/// 원격 탐색기 항목의 이름 변경이면 요청을 넣는다. 원격이 아니면 None 이고 호출자가 로컬과 같이
/// 처리한다. 원격이면 받은 요청의 기다림을, 받지 않았거나 보낼 수 없는 이름이면 `Some(None)` 을 돌려준다.
fn request_remote_rename(
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    buffer: &str,
) -> Option<Option<crate::state::RenameWaiting>> {
    let Some((
        RenameTarget::ExplorerEntry {
            surface_id,
            path,
            binding,
        },
        _,
    )) = &state.dialogs.rename
    else {
        return None;
    };
    let surface_id = *surface_id;
    engine.mirror_workspace_of(surface_id)?;
    let windows = explorer_name_rules(engine, surface_id, path);
    let name = buffer.trim();
    if !binding.current_in(engine)
        || name.is_empty()
        || explorer_name_error(buffer, windows).is_some()
    {
        return Some(None);
    }
    let host = state
        .explorer_views
        .get(surface_id)
        .and_then(|v| v.remote_host.as_ref())
        .map(|h| h.label.clone())
        .unwrap_or_default();
    let operation = crate::app::explorer_files::Operation::Rename {
        path: path.clone(),
        name: name.to_owned(),
    };
    let request = state.request_explorer_file_numbered(
        engine,
        surface_id,
        operation,
        crate::intent::IntentOrigin::User {
            source: crate::intent::UserSource::Menu("rename_popup"),
        },
    );
    Some(request.map(|request| crate::state::RenameWaiting {
        request,
        host,
        done: false,
    }))
}

/// 탐색기 항목 이름은 새 항목 입력과 같은 규칙으로 검사하고 이유를 보인다.
fn explorer_validation(buffer: &str, windows: bool) -> (Option<String>, bool) {
    match explorer_name_error(buffer, windows) {
        Some(error) => (Some(error), false),
        None => (None, true),
    }
}

/// 이름 변경에 Windows 이름 규칙을 쓰는가. 원격 항목은 그 경로의 형식을 따른다.
fn explorer_name_rules(
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    surface_id: u32,
    path: &std::path::Path,
) -> bool {
    crate::adapters::ui::surface::explorer::create::windows_rules(
        path,
        engine.mirror_workspace_of(surface_id).is_some(),
    )
}

/// 이름 바꾸기 대화상자의 이름 검사. 새 항목 입력과 같은 규칙(빈 이름·금지 글자·앞뒤 공백·예약 이름)이다.
/// 문제가 없으면 `None`, 있으면 보일 문구다. 이미 있는지는 실제 이름 바꾸기가 알린다.
fn explorer_name_error(name: &str, windows: bool) -> Option<String> {
    crate::adapters::ui::surface::explorer::create::check_name(name, windows)
        .err()
        .map(|e| e.message(name))
}

/// 빈 카테고리 이름은 확인만 막고 오류 문구는 숨긴다. rename에서 자기 이름은 중복으로 보지 않는다.
fn category_validation(
    target: &RenameTarget,
    buffer: &str,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> (Option<String>, bool) {
    let result = match target {
        RenameTarget::NewCategory => {
            let existing: Vec<&str> = engine
                .categories()
                .iter()
                .map(|c| c.name.as_str())
                .collect();
            crate::model::validate_new_category_name(buffer, existing)
        }
        RenameTarget::CategoryName { cat_id } => {
            let existing: Vec<&str> = engine
                .categories()
                .iter()
                .filter(|c| c.id != *cat_id)
                .map(|c| c.name.as_str())
                .collect();
            crate::model::validate_rename_category_name(buffer, existing)
        }
        _ => return (None, true),
    };
    match result {
        Ok(_) => (None, true),
        Err(e) => {
            let text = if buffer.trim().is_empty() {
                None
            } else {
                Some(t(e.i18n_key()).to_string())
            };
            (text, false)
        }
    }
}

fn rename_target_exists(
    target: &RenameTarget,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> bool {
    match target {
        RenameTarget::WorkspaceName { workspace_id }
        | RenameTarget::WorkspaceSubtitle { workspace_id } => {
            engine.find_workspace_index_for_id(*workspace_id).is_some()
        }
        RenameTarget::TabName { tab_id } => engine.find_pane_for_tab(*tab_id).is_some(),
        // 원격 항목은 이 컴퓨터에서 확인할 수 없다. 없으면 서버의 이름 변경이 실패로 알린다.
        RenameTarget::ExplorerEntry {
            path,
            binding,
            surface_id,
        } => {
            binding.current_in(engine)
                && (engine.mirror_workspace_of(*surface_id).is_some() || path.exists())
        }
        RenameTarget::ExplorerAddFavorite { path } => path.exists(),
        RenameTarget::NewCategory => true,
        RenameTarget::CategoryName { cat_id } => engine.category_index(*cat_id).is_some(),
    }
}

fn apply_rename(
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    target: RenameTarget,
    buffer: String,
) {
    match target {
        RenameTarget::WorkspaceName { workspace_id } => {
            apply_rename_workspace_name(state, engine, workspace_id, buffer)
        }
        RenameTarget::WorkspaceSubtitle { workspace_id } => {
            apply_rename_workspace_subtitle(state, engine, workspace_id, buffer)
        }
        RenameTarget::TabName { tab_id } => apply_rename_tab_name(state, engine, tab_id, buffer),
        RenameTarget::ExplorerEntry {
            surface_id,
            path,
            binding,
        } => {
            // 팝업이 떠 있는 동안 surface 가 바뀌었으면 새 surface 에 요청하지 않는다.
            // 확인 버튼은 직전 프레임 입력으로 막으므로, 같은 프레임의 Enter 도 여기서 다시 검사한다.
            let windows = explorer_name_rules(engine, surface_id, &path);
            if binding.current_in(engine) && explorer_name_error(&buffer, windows).is_none() {
                apply_rename_explorer_entry(state, engine, surface_id, path, buffer)
            }
        }
        RenameTarget::ExplorerAddFavorite { path } => {
            apply_rename_explorer_add_favorite(state, path, buffer)
        }
        RenameTarget::NewCategory => apply_rename_new_category(state, buffer),
        RenameTarget::CategoryName { cat_id } => apply_rename_category_name(state, cat_id, buffer),
    }
}

fn apply_rename_workspace_name(
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    workspace_id: u32,
    buffer: String,
) {
    if buffer.is_empty() {
        return;
    }
    if engine.find_workspace_index_for_id(workspace_id).is_some() {
        state.dispatch_intent(
            crate::intent::Intent::DirectRename(
                crate::intent::rename::DirectRename::WorkspaceName {
                    workspace_id,
                    name: buffer,
                },
            )
            .from_user_menu("rename_popup"),
        );
    }
}

fn apply_rename_workspace_subtitle(
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    workspace_id: u32,
    buffer: String,
) {
    if engine.find_workspace_index_for_id(workspace_id).is_some() {
        state.dispatch_intent(
            crate::intent::Intent::DirectRename(
                crate::intent::rename::DirectRename::WorkspaceSubtitle {
                    workspace_id,
                    subtitle: buffer,
                },
            )
            .from_user_menu("rename_popup"),
        );
    }
}

fn apply_rename_tab_name(
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    tab_id: u32,
    buffer: String,
) {
    if engine.find_pane_for_tab(tab_id).is_none() {
        return;
    }
    // 빈 이름은 사용자 이름을 지우고 선택된 surface의 제목으로 돌아가게 한다.
    let name = buffer.trim();
    let name = (!name.is_empty()).then(|| name.to_string());
    state.dispatch_intent(
        crate::intent::Intent::DirectRename(crate::intent::rename::DirectRename::TabName {
            tab_id,
            name,
        })
        .from_user_menu("rename_popup"),
    );
}

fn apply_rename_explorer_entry(
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    surface_id: u32,
    path: std::path::PathBuf,
    buffer: String,
) {
    let name = buffer.trim();
    if !name.is_empty() {
        state.request_explorer_file(
            engine,
            surface_id,
            crate::app::explorer_files::Operation::Rename {
                path,
                name: name.to_owned(),
            },
            crate::intent::IntentOrigin::User {
                source: crate::intent::UserSource::Menu("rename_popup"),
            },
        );
    }
}

fn apply_rename_explorer_add_favorite(
    state: &mut MainViewState,
    path: std::path::PathBuf,
    label: String,
) {
    state.dispatch_intent(
        crate::intent::Intent::Engine(
            crate::app::engine_action::EngineAction::AddExplorerFavorite { path, label },
        )
        .from_user_context_menu(),
    );
}

fn apply_rename_new_category(state: &mut MainViewState, buffer: String) {
    state.dispatch_intent(
        crate::app::command::DomainIntent::CreateCategory { name: buffer }
            .from_user_menu("rename_popup"),
    );
}

fn apply_rename_category_name(
    state: &mut MainViewState,
    cat_id: crate::model::WorkspaceCategoryId,
    buffer: String,
) {
    state.dispatch_intent(
        crate::app::command::DomainIntent::RenameCategory {
            id: cat_id,
            name: buffer,
        }
        .from_user_menu("rename_popup"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_with_input(raw: egui::RawInput, initial_buffer: &str) -> (RenamePopupAction, String) {
        let ctx = egui::Context::default();
        let mut out = RenamePopupAction::None;
        let theme = test_theme();
        let mut buffer = initial_buffer.to_string();
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut props = RenamePopupProps {
                    theme: &theme,
                    buffer: &mut buffer,
                    save_label: "Save",
                    cancel_label: "Cancel",
                    body_font_size: 12.0,
                    error: None,
                    save_enabled: true,
                    error_height: 0.0,
                    waiting: None,
                };
                out = draw_rename_popup_view(ui, &mut props);
            });
        }));
        (out, buffer)
    }

    /// 글자 `text`를 감싸는 사각형들의 채움색과 테두리색. 같은 색을 쓰는 다른 위젯(꺼진 버튼)은 뺀다.
    fn box_colors(
        output: &egui::FullOutput,
        text: &str,
    ) -> (Vec<egui::Color32>, Vec<egui::Color32>) {
        let at = output
            .shapes
            .iter()
            .find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.text() == text => {
                    Some(t.visual_bounding_rect().center())
                }
                _ => None,
            })
            .expect("the text is drawn");
        let rects = || {
            output.shapes.iter().filter_map(move |c| match &c.shape {
                egui::Shape::Rect(r) if r.rect.contains(at) => Some(r),
                _ => None,
            })
        };
        (
            rects().map(|r| r.fill).collect(),
            rects()
                .filter(|r| r.stroke.width > 0.0)
                .map(|r| r.stroke.color)
                .collect(),
        )
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// 내용 높이와 view 가 알린 오류 줄 높이를 잰다.
    fn measured(error: Option<&str>) -> (f32, f32) {
        let ctx = egui::Context::default();
        let theme = test_theme();
        let mut buffer = "note.md ".to_string();
        let mut out = (0.0, 0.0);
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_width(200.0);
                let top = ui.cursor().top();
                let mut props = RenamePopupProps {
                    theme: &theme,
                    buffer: &mut buffer,
                    save_label: "Save",
                    cancel_label: "Cancel",
                    body_font_size: 12.0,
                    error,
                    save_enabled: error.is_none(),
                    error_height: 0.0,
                    waiting: None,
                };
                let _ = draw_rename_popup_view(ui, &mut props); // 높이만 잰다 — action 무시.
                out = (ui.cursor().top() - top, props.error_height);
            });
        }));
        out
    }

    #[test]
    fn an_error_line_grows_the_popup_by_its_height_so_the_buttons_stay_inside() {
        let (plain, none) = measured(None);
        assert_eq!(none, 0.0);
        let (with_error, error_h) = measured(Some("Names can't start or end with a space."));
        assert!(error_h > 0.0);
        assert_eq!(
            with_error,
            plain + error_h,
            "the error line is the only thing that pushes the button row down"
        );
        // 기본 크기는 오류가 없는 내용이 들어가는 높이다. sizer 는 오류 줄만큼 더한다.
        let content = LogicalPx(64.0).value();
        assert!(plain <= content, "{plain} > {content}");
        assert!(with_error <= content + error_h);
    }

    #[test]
    fn rename_view_no_input_yields_none() {
        let (action, _) = run_with_input(egui::RawInput::default(), "hello");
        assert_eq!(action, RenamePopupAction::None);
    }

    #[test]
    fn rename_view_escape_yields_cancel() {
        let mut raw = egui::RawInput::default();
        raw.events.push(key_event(egui::Key::Escape));
        let (action, _) = run_with_input(raw, "hello");
        assert_eq!(action, RenamePopupAction::Cancel);
    }

    #[test]
    fn rename_view_enter_yields_confirm_with_buffer() {
        // 첫 프레임에 포커스를 얻고 다음 프레임에 Enter를 보내 실제 입력 조건을 맞춘다.
        let ctx = egui::Context::default();
        let theme = test_theme();
        let mut buffer = String::from("renamed");
        let mut last: RenamePopupAction = RenamePopupAction::None;

        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut props = RenamePopupProps {
                    theme: &theme,
                    buffer: &mut buffer,
                    save_label: "Save",
                    cancel_label: "Cancel",
                    body_font_size: 12.0,
                    error: None,
                    save_enabled: true,
                    error_height: 0.0,
                    waiting: None,
                };
                let _ = draw_rename_popup_view(ui, &mut props); // focus priming frame — action 무시.
            });
        }));

        let mut raw = egui::RawInput::default();
        raw.events.push(key_event(egui::Key::Enter));
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut props = RenamePopupProps {
                    theme: &theme,
                    buffer: &mut buffer,
                    save_label: "Save",
                    cancel_label: "Cancel",
                    body_font_size: 12.0,
                    error: None,
                    save_enabled: true,
                    error_height: 0.0,
                    waiting: None,
                };
                last = draw_rename_popup_view(ui, &mut props);
            });
        }));

        assert_eq!(last, RenamePopupAction::Confirm("renamed".to_string()));
    }

    #[test]
    fn a_waiting_rename_keeps_the_name_and_ignores_keys_until_the_remote_answers() {
        let ctx = egui::Context::default();
        let theme = test_theme();
        let mut buffer = String::from("report-final.pdf");
        let mut actions = Vec::new();
        let mut heights = Vec::new();
        let mut output = egui::FullOutput::default();
        let frames = [
            vec![],
            vec![key_event(egui::Key::Escape)],
            vec![egui::Event::Text("x".into())],
            vec![key_event(egui::Key::Enter)],
        ];
        for events in frames {
            let raw = egui::RawInput {
                events,
                ..Default::default()
            };
            output = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_width(200.0);
                    let top = ui.cursor().top();
                    let mut props = RenamePopupProps {
                        theme: &theme,
                        buffer: &mut buffer,
                        save_label: "Save",
                        cancel_label: "Cancel",
                        body_font_size: 12.0,
                        error: None,
                        save_enabled: true,
                        error_height: 0.0,
                        waiting: Some("Renaming on build-eu…"),
                    };
                    actions.push(draw_rename_popup_view(ui, &mut props));
                    heights.push((ui.cursor().top() - top, props.error_height));
                });
            });
        }
        assert!(
            actions.iter().all(|a| *a == RenamePopupAction::None),
            "{actions:?}"
        );
        assert_eq!(buffer, "report-final.pdf");
        // 보낸 이름 칸은 시안 Input readOnly처럼 readonly 바탕과 테두리를 둔다.
        let (fills, strokes) = box_colors(&output, "report-final.pdf");
        assert!(
            fills.contains(&theme.input_readonly_bg().to_egui()),
            "{fills:?}"
        );
        assert!(
            strokes.contains(&theme.input_readonly_border().to_egui()),
            "{strokes:?}"
        );
        // 대기 줄이 팝업을 그만큼 늘려 버튼 줄이 안에 남는다.
        let (plain, _) = measured(None);
        let (content, wait_h) = heights[0];
        assert!(wait_h > 0.0);
        assert_eq!(content, plain + wait_h);
    }

    #[test]
    fn rename_view_renders_empty_buffer_without_panic() {
        let (action, buffer) = run_with_input(egui::RawInput::default(), "");
        assert_eq!(action, RenamePopupAction::None);
        assert_eq!(buffer, "");
    }

    fn engine() -> crate::runtime::engine_session::EngineSession {
        use tasty_core::DomainEvent as E;
        crate::state::tests::test_state_from_model(crate::state::tests::test_model(vec![
            E::CategoryCreated {
                id: 0,
                name: "normal".into(),
                index: 0,
            },
            E::CategoryCreated {
                id: 1,
                name: "Services".into(),
                index: 1,
            },
            E::CategoryCreated {
                id: 2,
                name: "Infra".into(),
                index: 2,
            },
        ]))
        .1
    }

    #[test]
    fn category_validation_new_category_rules() {
        let mut e_session = engine();
        let e = e_session.borrow_mut();
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "  ", &e.read());
        assert!(!ok && err.is_none());
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "normal", &e.read());
        assert!(!ok && err.is_some());
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "services", &e.read());
        assert!(!ok && err.is_some());
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "Other", &e.read());
        assert!(ok && err.is_none());
    }

    #[test]
    fn category_validation_rename_allows_self_name() {
        let mut e_session = engine();
        let e = e_session.borrow_mut();
        let id = 1;
        let (err, ok) = category_validation(
            &RenameTarget::CategoryName { cat_id: id },
            "SERVICES",
            &e.read(),
        );
        assert!(ok && err.is_none());
        let (err, ok) = category_validation(
            &RenameTarget::CategoryName { cat_id: id },
            "Infra",
            &e.read(),
        );
        assert!(!ok && err.is_some());
    }

    #[test]
    fn the_rename_sizer_adds_the_last_error_line_height() {
        let (mut state, session) = crate::state::tests::test_state();
        let base = rename_popup_sizer(&state, &session.read());
        assert_eq!(base, rename_popup_default_size());
        state.dialogs.rename_error_height = 18.0;
        assert_eq!(
            rename_popup_sizer(&state, &session.read()),
            base + egui::vec2(0.0, 18.0)
        );
    }

    #[test]
    fn an_explorer_rename_uses_the_new_item_name_rules() {
        crate::i18n::init("en");
        assert_eq!(explorer_validation("b.txt", false), (None, true));
        assert_eq!(
            explorer_validation("b.txt ", false),
            (
                Some("Names can't start or end with a space.".to_string()),
                false
            )
        );
        assert!(!explorer_validation("   ", false).1);

        let (mut state, session) = crate::state::tests::test_state();
        let sid = session.read().workspace_at(0).unwrap().all_surface_ids()[0];
        let binding =
            crate::runtime::surface_binding::SurfaceBinding::capture(&session.read(), sid).unwrap();
        let target = RenameTarget::ExplorerEntry {
            surface_id: sid,
            path: crate::test_support::abs_path("proj/a.txt"),
            binding,
        };
        apply_rename(&mut state, &session.read(), target, " b.txt".into());
        assert_eq!(
            state.explorer_file_requests.len(),
            0,
            "a name the dialog refuses is never sent, even on the frame it was typed"
        );
    }

    #[test]
    fn an_explorer_rename_is_dropped_when_its_surface_changed_while_the_popup_was_open() {
        let (mut state, mut session) = crate::state::tests::test_state();
        let sid = session.read().workspace_at(0).unwrap().all_surface_ids()[0];
        let path = crate::test_support::abs_path("proj/a.txt");
        let target = |binding| RenameTarget::ExplorerEntry {
            surface_id: sid,
            path: path.clone(),
            binding,
        };
        let opened =
            crate::runtime::surface_binding::SurfaceBinding::capture(&session.read(), sid).unwrap();

        session.borrow_mut().runtime.terminals.insert(
            sid,
            tasty_terminal::Terminal::new_detached(80, 24),
            None,
        );
        apply_rename(&mut state, &session.read(), target(opened), "b.txt".into());
        assert_eq!(state.explorer_file_requests.len(), 0);

        let current =
            crate::runtime::surface_binding::SurfaceBinding::capture(&session.read(), sid).unwrap();
        apply_rename(&mut state, &session.read(), target(current), "b.txt".into());
        assert_eq!(
            state.explorer_file_requests.len(),
            1,
            "an unchanged surface is renamed"
        );
    }
}
