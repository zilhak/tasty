//! Rename popup (workspace name / workspace subtitle / tab name) — Tier 3 분리.
//!
//! Pure view (`draw_rename_popup_view`) 는 `RenamePopupProps` (theme + label
//! 라벨 + buffer mut ref) 만 받아 `RenamePopupAction` 을 반환한다. wrapper
//! (`draw_rename_popup`) 는 AppState/CoreState 에서 target 유효성 검증 + buffer
//! 추출 + view 호출 + action 을 mutation (enqueue_host_event + mark_layout_dirty)
//! 으로 번역한다. gallery 는 같은 view 를 mock 라벨 + 로컬 buffer 로 호출해
//! 시각 검증한다 — Tier 3 패턴
//! (`.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::{AppState, RenameTarget};
use crate::theme;
use crate::theme::Theme;

/// Default size for the rename popup.
pub fn rename_popup_default_size() -> egui::Vec2 {
    egui::vec2(
        280.0,
        popup::title_bar_height() + popup::content_margin() * 2.0 + 64.0,
    )
}

/// Dynamic title for the rename popup (based on RenameTarget).
pub fn rename_popup_title(state: &AppState, _engine: &crate::core::CoreState) -> String {
    state
        .dialogs
        .rename
        .as_ref()
        .map(|(target, _)| t(target.heading_key()).to_string())
        .unwrap_or_else(|| t("rename_dialog.tab_heading").to_string())
}

/// Pure inputs to [`draw_rename_popup_view`]. AppState / CoreState 의존 0.
///
/// `buffer` 는 `&mut String` — view 가 TextEdit 으로 직접 mutate. gallery 에서는
/// 로컬 `String` 의 `&mut` 를 넘기면 된다.
pub struct RenamePopupProps<'a> {
    /// Tier 3 popup props 일관성 보존용. 현재 view 본문은 egui 기본 스타일에
    /// 의존해 theme 토큰 직접 사용은 없지만, 다른 popup props 와 시그니처
    /// 통일성 (gallery mirror 도 동일 field 보유) 을 위해 유지.
    #[allow(dead_code)] // popup props 시그니처 일관성 — 현재 미read, 생성부(타 파일) 유지
    pub theme: &'a Theme,
    pub buffer: &'a mut String,
    pub save_label: &'a str,
    pub cancel_label: &'a str,
    pub body_font_size: f32,
}

/// User intent surfaced by [`draw_rename_popup_view`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenamePopupAction {
    None,
    Cancel,
    /// 확정 — view 가 buffer 의 현재 값을 owned String 으로 떠서 반환.
    Confirm(String),
}

/// Draw function for the rename popup (PopupDef draw_fn).
pub fn draw_rename_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let th = theme::theme();

    // target 유효성: target 자체가 없으면 닫는다.
    let Some((ref target, _)) = state.dialogs.rename else {
        return PopupAction::Close;
    };

    // 즐겨찾기 추가 팝업은 확정 버튼 라벨이 "Add" (design §3.5) — 나머지는 "Save".
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
    };
    if !valid {
        state.dialogs.rename = None;
        return PopupAction::Close;
    }

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
            // target 은 위에서 유효성 검증을 통과했으니 take().unwrap() 안전.
            let (target, _) = state.dialogs.rename.take().unwrap();
            apply_rename(state, engine, target, buffer);
            PopupAction::Close
        }
    }
}

/// Pure view: TextEdit + Save/Cancel 버튼만 그린다. AppState/CoreState 접근 없음.
///
/// Escape 키 → Cancel, Enter 키 (text field focus 시) → Confirm. Save 버튼 클릭
/// → Confirm, Cancel 버튼 클릭 → Cancel. action 우선순위는 Confirm > Cancel > None.
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
            .margin(egui::Margin::symmetric(4, 2)),
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

    if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        confirm = true;
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(props.cancel_label).clicked() {
                cancel = true;
            }
            if ui.button(props.save_label).clicked() {
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

fn apply_rename(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    target: RenameTarget,
    buffer: String,
) {
    match target {
        RenameTarget::WorkspaceName { ws_idx } => {
            if !buffer.is_empty() {
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
        }
        RenameTarget::WorkspaceSubtitle { ws_idx } => {
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
        RenameTarget::TabName { pane_id, tab_index } => {
            let name = buffer.trim().to_string();
            let mut renamed: Option<(u32, String)> = None;
            if let Some(pane) = state
                .active_workspace_mut(engine)
                .pane_layout_mut()
                .find_pane_mut(pane_id)
                && let Some(tab) = pane.tabs.get_mut(tab_index)
            {
                if name.is_empty() {
                    tab.explicit_name = None;
                } else {
                    tab.explicit_name = Some(name.clone());
                }
                renamed = Some((tab.id, tab.display_name().to_string()));
            }
            if let Some((tab_id, title)) = renamed {
                state.enqueue_host_event(crate::state::PendingHostEvent::TabRenamed {
                    tab_id,
                    title,
                    user_direct: true,
                });
            }
        }
        RenameTarget::ExplorerEntry { surface_id, path } => {
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
        RenameTarget::ExplorerAddFavorite { path } => {
            engine.explorer_favorites.add(path, buffer);
            engine.explorer_favorites.save();
        }
    }
    engine.mark_layout_dirty();
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
        // Enter 는 text field 가 focus 를 가질 때만 confirm. view 는 첫 프레임에
        // request_focus 를 호출하지만, ctx.run 1 회로는 focus state 가 다음 프레임에
        // 확정되므로 동일 프레임에 Enter 만 누르면 None 이 될 수 있다 — 두 프레임
        // (focus 획득 → Enter) 시뮬레이션.
        let ctx = egui::Context::default();
        let theme = test_theme();
        let mut buffer = String::from("renamed");
        let mut last: RenamePopupAction = RenamePopupAction::None;

        // Frame 1: focus 획득. action 은 None 이라 폐기.
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut props = RenamePopupProps {
                    theme: &theme,
                    buffer: &mut buffer,
                    save_label: "Save",
                    cancel_label: "Cancel",
                    body_font_size: 12.0,
                };
                let _ = draw_rename_popup_view(ui, &mut props); // focus priming frame — action 무시.
            });
        }));

        // Frame 2: Enter key 주입.
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
}
