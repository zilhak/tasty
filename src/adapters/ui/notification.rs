use std::time::Instant;

use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{margin_all, vspace};

/// Draw notification panel content inside a popup Ui.
/// Called by the notifications popup's `draw_fn` (see popup_defs).
pub(crate) fn draw_notification_content_inner(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) {
    let th = theme::theme();

    // Header with mark-all-read button
    ui.horizontal(|ui| {
        let unread = engine.notifications.unread_count();
        ui.label(
            egui::RichText::new(t_fmt(
                "notification_panel.unread_count",
                &unread.to_string(),
            ))
            .small()
            .color(th.text_muted()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(t("button.mark_all_read")).clicked() {
                state.dispatch_intent(
                    crate::core::intent::DomainIntent::MarkAllNotificationsRead
                        .from_user_menu("notification_panel.mark_all_read"),
                );
            }
        });
    });
    ui.separator();

    // Scrollable notification list (newest first)
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            let notification_count = engine.notifications.all().len();
            if notification_count == 0 {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new(t("notification_panel.empty_message"))
                            .color(th.text_muted()),
                    );
                });
                return;
            }

            let now = Instant::now();
            let entries: Vec<_> = engine
                .notifications
                .all()
                .rev()
                .map(|n| {
                    let elapsed = now.duration_since(n.timestamp);
                    let time_str = if elapsed.as_secs() < 60 {
                        format!("{}s ago", elapsed.as_secs())
                    } else if elapsed.as_secs() < 3600 {
                        format!("{}m ago", elapsed.as_secs() / 60)
                    } else {
                        format!("{}h ago", elapsed.as_secs() / 3600)
                    };

                    let ws_name = engine
                        .workspaces
                        .iter()
                        .find(|ws| ws.id == n.source_workspace)
                        .map(|ws| ws.name.as_str())
                        .unwrap_or(t("notification_panel.unknown_workspace"));

                    (
                        n.id,
                        n.read,
                        n.title.clone(),
                        n.body.clone(),
                        time_str,
                        ws_name.to_string(),
                        n.source_workspace,
                    )
                })
                .collect();

            let mut mark_read_id = None;
            let mut jump_to_ws = None;

            for (id, read, title, body, time_str, ws_name, ws_id) in &entries {
                let bg = if *read {
                    egui::Color32::TRANSPARENT
                } else {
                    // unread 알림 항목 배경: theme blue 의 살짝 깔린 톤.
                    // 대응 토큰 없음 — 값에 이름만 둔다.
                    const UNREAD_ROW_BG_ALPHA: u8 = 20;
                    crate::theme::theme()
                        .blue
                        .with_alpha(UNREAD_ROW_BG_ALPHA)
                        .to_egui()
                };

                egui::Frame::new()
                    .fill(bg)
                    .inner_margin(margin_all(th.spacing_xs))
                    .corner_radius(th.corner_radius.value())
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if !*read {
                                ui.label(
                                    egui::RichText::new("*").color(th.accent_primary()).strong(),
                                );
                            }
                            ui.label(egui::RichText::new(title).strong().small());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(time_str)
                                            .small()
                                            .color(th.text_muted()),
                                    );
                                },
                            );
                        });

                        if !body.is_empty() {
                            ui.label(egui::RichText::new(body).small());
                        }

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(ws_name)
                                    .small()
                                    .color(th.accent_primary()),
                            );

                            if ui
                                .small_button(t("button.jump_to_workspace"))
                                .on_hover_text(t("tooltip.jump_to_workspace"))
                                .clicked()
                            {
                                jump_to_ws = Some(*ws_id);
                                mark_read_id = Some(*id);
                            }
                        });
                    });

                vspace(ui, STRUCT_GAP_2);
            }

            if let Some(id) = mark_read_id {
                state.dispatch_intent(
                    crate::core::intent::DomainIntent::MarkNotificationRead { id }
                        .from_user_menu("notification_panel.mark_read"),
                );
            }
            if let Some(ws_id) = jump_to_ws
                && let Some(idx) = engine.workspaces.iter().position(|ws| ws.id == ws_id)
            {
                state.switch_workspace(engine, idx);
            }
        });
}

/// PopupDef::draw_fn for the notifications panel.
pub fn draw_notification_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> crate::adapters::ui::popup::PopupAction {
    draw_notification_content_inner(ui, state, engine);
    crate::adapters::ui::popup::PopupAction::None
}
