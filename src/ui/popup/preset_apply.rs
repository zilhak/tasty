//! Preset 적용 picker popup × 3.
//!
//! 저장된 Workspace/Tab/Pane preset 목록을 보여주고, 사용자가 선택해 [적용] 버튼이나
//! Enter 로 적용하면 `DialogState::pending_preset_apply` 에 enqueue 한다.
//! App 메인 루프가 drain 해 `state.apply_*_preset` 를 호출한다.
//!
//! preset 목록 자체는 `engine.preset_store: Option<Arc<Mutex<PresetStore>>>` 에서
//! 매 프레임 lock 으로 읽는다.

use tasty_presets::PresetKind;

use crate::i18n::t;
use crate::intent::Intent;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

pub const APPLY_WORKSPACE_POPUP_ID: &str = "apply_workspace_preset";
pub const APPLY_TAB_POPUP_ID: &str = "apply_tab_preset";
pub const APPLY_PANE_POPUP_ID: &str = "apply_pane_preset";

pub fn draw_apply_workspace_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
) -> PopupAction {
    draw_apply_popup(ui, state, engine, PresetKind::Workspace)
}

pub fn draw_apply_tab_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
) -> PopupAction {
    draw_apply_popup(ui, state, engine, PresetKind::Tab)
}

pub fn draw_apply_pane_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
) -> PopupAction {
    draw_apply_popup(ui, state, engine, PresetKind::Pane)
}

fn draw_apply_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    kind: PresetKind,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        state.dialogs.preset_picker_selected = None;
        return PopupAction::Close;
    }

    let th = theme::theme();
    let names: Vec<String> = match engine.preset_store.as_ref() {
        Some(arc) => match arc.lock() {
            Ok(g) => g.list(kind),
            Err(poisoned) => poisoned.into_inner().list(kind),
        },
        None => Vec::new(),
    };

    let selected = state.dialogs.preset_picker_selected.clone();
    if selected.is_none() {
        if let Some(first) = names.first() {
            state.dialogs.preset_picker_selected = Some(first.clone());
        }
    } else if let Some(sel) = &selected {
        if !names.iter().any(|n| n == sel) {
            state.dialogs.preset_picker_selected = names.first().cloned();
        }
    }

    let enter_pressed = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));
    let up = ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowUp));
    let down = ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowDown));

    if (up || down) && !names.is_empty() {
        let cur = state
            .dialogs
            .preset_picker_selected
            .as_ref()
            .and_then(|s| names.iter().position(|n| n == s))
            .unwrap_or(0);
        let next = if up {
            if cur == 0 { names.len() - 1 } else { cur - 1 }
        } else {
            (cur + 1) % names.len()
        };
        state.dialogs.preset_picker_selected = names.get(next).cloned();
    }

    let mut apply_clicked = false;
    let mut cancel_clicked = false;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        if names.is_empty() {
            ui.label(
                egui::RichText::new(t("preset.popup.empty"))
                    .color(th.subtext0)
                    .italics(),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for name in &names {
                        let is_selected =
                            state.dialogs.preset_picker_selected.as_deref() == Some(name.as_str());
                        let full_width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(full_width, 22.0),
                            egui::Sense::click(),
                        );
                        if is_selected {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            name,
                            egui::FontId::proportional(12.0),
                            if is_selected {
                                th.text.into()
                            } else {
                                th.subtext0.into()
                            },
                        );
                        if resp.clicked() {
                            state.dialogs.preset_picker_selected = Some(name.clone());
                        }
                        if resp.double_clicked() {
                            state.dialogs.preset_picker_selected = Some(name.clone());
                            apply_clicked = true;
                        }
                    }
                });
        }

        ui.separator();
        ui.horizontal(|ui| {
            let can_apply = !names.is_empty() && state.dialogs.preset_picker_selected.is_some();
            if ui
                .add_enabled(can_apply, egui::Button::new(t("preset.popup.apply_button")))
                .clicked()
            {
                apply_clicked = true;
            }
            if ui.button(t("preset.popup.cancel_button")).clicked() {
                cancel_clicked = true;
            }
        });
    });

    if enter_pressed && !names.is_empty() {
        apply_clicked = true;
    }

    if apply_clicked {
        if let Some(name) = state.dialogs.preset_picker_selected.clone() {
            state.dispatch_intent(
                Intent::ApplyPreset { kind, name }.from_user_menu("preset_apply_popup"),
            );
            state.dialogs.preset_picker_selected = None;
            return PopupAction::Close;
        }
    }
    if cancel_clicked {
        state.dialogs.preset_picker_selected = None;
        return PopupAction::Close;
    }

    PopupAction::None
}
