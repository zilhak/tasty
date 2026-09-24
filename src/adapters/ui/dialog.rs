//! 이름 변경 팝업. view는 입력값과 버퍼를 받아 액션을 반환하고
//! 호출자가 실제 대상 변경을 적용한다. 갤러리도 같은 view를 사용한다.

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::{AppState, RenameTarget};
use crate::theme;
use crate::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::margin_sym;
use tasty_ui_widgets::tokens::{STRUCT_GAP_2, STRUCT_GAP_4};
use tasty_ui_widgets::vspace;

pub fn rename_popup_default_size() -> egui::Vec2 {
    egui::vec2(
        280.0,
        (popup::title_bar_height() + popup::content_margin().scaled(2.0) + LogicalPx(64.0)).value(),
    )
}

pub fn rename_popup_title(state: &AppState, _engine: &crate::core::CoreState) -> String {
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
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.rename = None;
}

pub fn draw_rename_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let th = theme::theme();

    let Some((ref target, _)) = state.dialogs.rename else {
        return PopupAction::Close;
    };

    let is_add_favorite = matches!(target, RenameTarget::ExplorerAddFavorite { .. });

    let valid = match target {
        RenameTarget::WorkspaceName { ws_idx } | RenameTarget::WorkspaceSubtitle { ws_idx } => {
            *ws_idx < engine.workspaces.len()
        }
        RenameTarget::TabName { pane_id, tab_index } => state
            .active_workspace(engine)
            .pane_layout()
            .find_pane(*pane_id)
            .is_some_and(|p| *tab_index < p.tabs.len()),
        RenameTarget::ExplorerEntry { path, .. } => path.exists(),
        RenameTarget::ExplorerAddFavorite { path } => path.exists(),
        RenameTarget::NewCategory => true,
        RenameTarget::CategoryName { cat_id } => engine.category_index(*cat_id).is_some(),
    };
    if !valid {
        state.dialogs.rename = None;
        return PopupAction::Close;
    }

    // 직전 프레임의 입력으로 카테고리 이름을 검사한다. 다른 대상에는 이 제한이 없다.
    let (category_error, save_enabled) = {
        let buffer = &state.dialogs.rename.as_ref().unwrap().1;
        category_validation(target, buffer, engine)
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
            error: category_error.as_deref(),
            save_enabled,
        };
        draw_rename_popup_view(inner, &mut props)
    };

    match action {
        RenamePopupAction::None => PopupAction::None,
        RenamePopupAction::Cancel => {
            state.dialogs.rename = None;
            PopupAction::Close
        }
        RenamePopupAction::Confirm(buffer) => {
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
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return RenamePopupAction::Cancel;
    }

    let resp = ui.add_sized(
        [ui.available_width(), 22.0],
        egui::TextEdit::singleline(props.buffer)
            .font(egui::FontId::proportional(props.body_font_size))
            // 입력 위젯 안의 위치 보정이며 레이아웃 간격 토큰과는 별개다.
            .margin(margin_sym(STRUCT_GAP_4, STRUCT_GAP_2)),
    );

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

    if props.save_enabled && resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }

    if let Some(err) = props.error {
        vspace(ui, props.theme.spacing_xs);
        ui.colored_label(props.theme.accent_danger(), err);
    }

    vspace(ui, props.theme.spacing_sm);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(props.cancel_label).clicked() {
                cancel = true;
            }
            if ui
                .add_enabled(props.save_enabled, egui::Button::new(props.save_label))
                .clicked()
            {
                confirm = true;
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

/// 빈 카테고리 이름은 확인만 막고 오류 문구는 숨긴다. rename에서 자기 이름은 중복으로 보지 않는다.
fn category_validation(
    target: &RenameTarget,
    buffer: &str,
    engine: &crate::core::CoreState,
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

fn apply_rename(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    target: RenameTarget,
    buffer: String,
) {
    match target {
        RenameTarget::WorkspaceName { ws_idx } => {
            apply_rename_workspace_name(state, engine, ws_idx, buffer)
        }
        RenameTarget::WorkspaceSubtitle { ws_idx } => {
            apply_rename_workspace_subtitle(state, engine, ws_idx, buffer)
        }
        RenameTarget::TabName { pane_id, tab_index } => {
            apply_rename_tab_name(state, engine, pane_id, tab_index, buffer)
        }
        RenameTarget::ExplorerEntry { surface_id, path } => {
            apply_rename_explorer_entry(state, surface_id, path, buffer)
        }
        RenameTarget::ExplorerAddFavorite { path } => {
            apply_rename_explorer_add_favorite(engine, path, buffer)
        }
        RenameTarget::NewCategory => apply_rename_new_category(engine, buffer),
        RenameTarget::CategoryName { cat_id } => apply_rename_category_name(engine, cat_id, buffer),
    }
    engine.mark_layout_dirty();
}

fn apply_rename_workspace_name(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    ws_idx: usize,
    buffer: String,
) {
    if buffer.is_empty() {
        return;
    }
    let workspace_id = engine.workspaces.get(ws_idx).map(|w| w.id);
    if let Some(ws) = engine.workspaces.get_mut(ws_idx) {
        ws.name = buffer.clone();
    }
    if let Some(workspace_id) = workspace_id {
        state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
            workspace_id,
            name: Some(buffer),
            subtitle: None,
            description: None,
            user_direct: true,
        });
    }
}

fn apply_rename_workspace_subtitle(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    ws_idx: usize,
    buffer: String,
) {
    let workspace_id = engine.workspaces.get(ws_idx).map(|w| w.id);
    if let Some(ws) = engine.workspaces.get_mut(ws_idx) {
        ws.subtitle = buffer.clone();
    }
    if let Some(workspace_id) = workspace_id {
        state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
            workspace_id,
            name: None,
            subtitle: Some(buffer),
            description: None,
            user_direct: true,
        });
    }
}

fn apply_rename_tab_name(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_id: u32,
    tab_index: usize,
    buffer: String,
) {
    let name = buffer.trim().to_string();
    let clear = name.is_empty();
    let mut located: Option<(u32, u32)> = None;
    if let Some(pane) = state
        .active_workspace_mut(engine)
        .pane_layout_mut()
        .find_pane_mut(pane_id)
        && let Some(tab) = pane.tabs.get_mut(tab_index)
    {
        if clear {
            tab.explicit_name = None;
        } else {
            tab.explicit_name = Some(name.clone());
        }
        located = Some((tab.id, tab.focused_surface));
    }
    if let Some((tab_id, focused)) = located {
        // 사용자 이름을 지우면 현재 포커스된 surface 제목으로 돌아간다.
        if clear {
            engine.refresh_tab_osc_title(focused);
        }
        let title = state
            .active_workspace_mut(engine)
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .and_then(|pane| pane.tabs.get(tab_index))
            .map(|tab| tab.display_name().to_string())
            .unwrap_or_default();
        state.enqueue_host_event(crate::state::PendingHostEvent::TabRenamed {
            tab_id,
            title,
            user_direct: true,
        });
    }
}

fn apply_rename_explorer_entry(
    state: &mut AppState,
    surface_id: u32,
    path: std::path::PathBuf,
    buffer: String,
) {
    let new_name = buffer.trim();
    if !new_name.is_empty()
        && let Some(parent) = path.parent()
    {
        let new_path = parent.join(new_name);
        if new_path != path
            && let Err(e) = std::fs::rename(&path, &new_path)
        {
            tracing::warn!("explorer: rename {} failed: {e}", path.display());
        }
    }
    if let Some(view) = state.explorer_views.get_mut(surface_id) {
        view.selected.clear();
        view.anchor = None;
        view.request_reload();
    }
}

fn apply_rename_explorer_add_favorite(
    engine: &mut crate::core::CoreState,
    path: std::path::PathBuf,
    buffer: String,
) {
    engine.explorer_favorites.add(path, buffer);
    engine.explorer_favorites.save();
}

fn apply_rename_new_category(engine: &mut crate::core::CoreState, buffer: String) {
    if let Err(e) = engine.create_category(&buffer) {
        tracing::warn!("create_category '{buffer}' failed: {e:?}");
    }
}

fn apply_rename_category_name(
    engine: &mut crate::core::CoreState,
    cat_id: crate::model::WorkspaceCategoryId,
    buffer: String,
) {
    if let Err(e) = engine.rename_category(cat_id, &buffer) {
        tracing::warn!("rename_category {cat_id} '{buffer}' failed: {e:?}");
    }
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
                };
                out = draw_rename_popup_view(ui, &mut props);
            });
        }));
        (out, buffer)
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
                };
                last = draw_rename_popup_view(ui, &mut props);
            });
        }));

        assert_eq!(last, RenamePopupAction::Confirm("renamed".to_string()));
    }

    #[test]
    fn rename_view_renders_empty_buffer_without_panic() {
        let (action, buffer) = run_with_input(egui::RawInput::default(), "");
        assert_eq!(action, RenamePopupAction::None);
        assert_eq!(buffer, "");
    }

    fn engine() -> crate::core::CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::core::CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn category_validation_new_category_rules() {
        let mut e = engine();
        e.create_category("Services").unwrap();
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "  ", &e);
        assert!(!ok && err.is_none());
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "normal", &e);
        assert!(!ok && err.is_some());
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "services", &e);
        assert!(!ok && err.is_some());
        let (err, ok) = category_validation(&RenameTarget::NewCategory, "Infra", &e);
        assert!(ok && err.is_none());
    }

    #[test]
    fn category_validation_rename_allows_self_name() {
        let mut e = engine();
        let id = e.create_category("Services").unwrap();
        let (err, ok) =
            category_validation(&RenameTarget::CategoryName { cat_id: id }, "SERVICES", &e);
        assert!(ok && err.is_none());
        e.create_category("Infra").unwrap();
        let (err, ok) =
            category_validation(&RenameTarget::CategoryName { cat_id: id }, "Infra", &e);
        assert!(!ok && err.is_some());
    }
}
