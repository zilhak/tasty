//! Command palette popup — VS Code 스타일 명령 검색기.
//!
//! 사용자가 입력한 쿼리에 대해 `command_palette::search`로 후보를 매칭하고, 위/아래로
//! 선택하고 Enter로 실행한다. 실행 시 `state.command_palette.pending_run`에 action_id를
//! 적재하고 popup을 닫는다. 실제 dispatch는 `MainWindow`가 다음 프레임 시작에 수행한다.

use crate::command_palette::{self, PaletteCommand};
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

pub const COMMAND_PALETTE_POPUP_ID: &str = "command_palette";

pub fn draw_command_palette_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    let th = theme::theme();

    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        state.command_palette.reset();
        return PopupAction::Close;
    }

    let commands = command_palette::all_commands();
    let labels: Vec<String> = commands.iter().map(|c| label_for(c)).collect();
    let matches = command_palette::search(&state.command_palette.query, &commands, &labels);

    // Clamp selection within result range.
    if matches.is_empty() {
        state.command_palette.selected = 0;
    } else if state.command_palette.selected >= matches.len() {
        state.command_palette.selected = matches.len() - 1;
    }

    // Arrow key navigation — applied before the TextEdit consumes them.
    let (up, down, enter) = ui.ctx().input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
        )
    });
    if down && !matches.is_empty() {
        state.command_palette.selected =
            (state.command_palette.selected + 1).min(matches.len() - 1);
    }
    if up && !matches.is_empty() {
        state.command_palette.selected = state.command_palette.selected.saturating_sub(1);
    }

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        let resp = ui.add(
            egui::TextEdit::singleline(&mut state.command_palette.query)
                .hint_text(crate::theme_bridge::hint_text(t("command_palette.placeholder")))
                .desired_width(ui.available_width() - 8.0)
                .font(egui::TextStyle::Body),
        );
        if !resp.has_focus() {
            resp.request_focus();
        }
        if resp.changed() {
            state.command_palette.selected = 0;
        }

        ui.separator();

        if matches.is_empty() {
            ui.label(
                egui::RichText::new(t("command_palette.no_results"))
                    .color(th.subtext0)
                    .italics(),
            );
        } else {
            let row_height = 24.0;
            let selected_idx = state.command_palette.selected;
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    for (i, (_score, cmd)) in matches.iter().enumerate() {
                        let label = label_for(cmd);
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_height),
                            egui::Sense::click(),
                        );
                        let is_selected = i == selected_idx;
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
                        let color = if is_selected || resp.hovered() {
                            th.text
                        } else {
                            th.subtext0
                        };
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(th.font_size_body.value()),
                            color.into(),
                        );

                        // Shortcut text (first binding) on the right.
                        if let Some(shortcut) = first_binding(state, cmd.id) {
                            ui.painter().text(
                                egui::pos2(rect.max.x - 8.0, rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                shortcut,
                                egui::FontId::proportional(th.font_size_body.value() - 1.0),
                                th.subtext0.into(),
                            );
                        }

                        if resp.clicked() {
                            state.command_palette.pending_run = Some(cmd.id);
                        }
                    }
                });
        }
    });

    if enter && !matches.is_empty() {
        let (_, cmd) = matches[state.command_palette.selected];
        state.command_palette.pending_run = Some(cmd.id);
    }

    if state.command_palette.pending_run.is_some() {
        state.command_palette.reset();
        return PopupAction::Close;
    }

    PopupAction::None
}

/// label_key를 통해 i18n 라벨을 얻되, 끝의 `:`는 떼어낸다 (Settings UI 라벨 재활용).
fn label_for(cmd: &PaletteCommand) -> String {
    let raw = t(cmd.label_key);
    raw.trim_end_matches(':').to_string()
}

fn first_binding(state: &AppState, action_id: &str) -> Option<String> {
    let bindings = state
        .engine
        .settings
        .keybindings
        .get_bindings(action_id)?;
    bindings.first().cloned()
}
