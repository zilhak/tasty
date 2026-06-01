//! Update-available popup.
//!
//! Shows the current version, the latest detected version, the release notes
//! (plain text), and a button to open the GitHub Releases page. A "Check now"
//! button forces an immediate poll. Phase 1 — no in-app download.

use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

pub const UPDATE_POPUP_ID: &str = "update_check";

pub fn draw_update_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    let th = theme::theme();
    let snapshot = state.update_status.lock().unwrap().clone();
    let current = env!("CARGO_PKG_VERSION");

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        ui.label(
            egui::RichText::new(t("update.heading"))
                .color(th.text)
                .size(13.0),
        );
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("update.current_label")).color(th.subtext0));
            ui.label(egui::RichText::new(current).color(th.text).strong());
        });

        match &snapshot.latest {
            Some(info) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t("update.latest_label")).color(th.subtext0));
                    ui.label(egui::RichText::new(&info.version).color(th.green).strong());
                });
                ui.separator();
                ui.label(
                    egui::RichText::new(t("update.notes_label"))
                        .color(th.subtext0)
                        .size(12.0),
                );
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&info.body).color(th.text).size(12.0),
                            )
                            .wrap(),
                        );
                    });
                ui.separator();
                let url = info.html_url.clone();
                ui.horizontal(|ui| {
                    if ui.button(t("update.open_release")).clicked() {
                        if let Err(e) = webbrowser::open(&url) {
                            tracing::warn!("update popup: open browser failed: {e}");
                        }
                    }
                    if ui.button(t("update.check_now")).clicked() {
                        trigger_check(state);
                    }
                });
            }
            None => {
                if let Some(err) = &snapshot.last_error {
                    ui.label(
                        egui::RichText::new(format!("{}: {err}", t("update.error_label")))
                            .color(th.red)
                            .size(12.0),
                    );
                } else if snapshot.last_checked.is_none() && !snapshot.in_flight {
                    ui.label(egui::RichText::new(t("update.never_checked")).color(th.subtext0));
                } else {
                    ui.label(egui::RichText::new(t("update.up_to_date")).color(th.green));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("update.check_now")).clicked() {
                        trigger_check(state);
                    }
                    if snapshot.in_flight {
                        ui.label(
                            egui::RichText::new(t("update.checking"))
                                .color(th.subtext0)
                                .italics(),
                        );
                    }
                });
            }
        }
    });

    PopupAction::None
}

fn trigger_check(state: &AppState) {
    crate::state::update_check::trigger_check(
        state.update_status.clone(),
        "zilhak",
        "tasty",
        env!("CARGO_PKG_VERSION"),
    );
}
