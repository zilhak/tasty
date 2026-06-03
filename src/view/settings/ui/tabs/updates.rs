//! Settings → Updates 탭. background poller 결과 표시 + "Check now" 버튼 +
//! release page 열기. In-app swap 은 phase J.H 범위 밖 — CLI `tasty update` 권장.

use std::sync::{Arc, Mutex};

use crate::i18n::t;
use crate::state::update_check::{self, UpdateStatus};
use crate::theme;

const OWNER: &str = "zilhak";
const REPO: &str = "tasty";

pub fn draw_updates_tab(ui: &mut egui::Ui, update_status: Option<&Arc<Mutex<UpdateStatus>>>) {
    let th = theme::theme();
    ui.add_space(8.0);

    let Some(shared) = update_status else {
        ui.label(t("settings.updates.unavailable"));
        return;
    };
    let snapshot = shared.lock().unwrap().clone();

    egui::Grid::new("updates_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.updates.current"));
            ui.label(
                egui::RichText::new(env!("CARGO_PKG_VERSION"))
                    .color(th.text)
                    .strong(),
            );
            ui.end_row();

            ui.label(t("settings.updates.latest"));
            match &snapshot.latest {
                Some(info) => {
                    ui.label(egui::RichText::new(&info.version).color(th.green).strong());
                }
                None => {
                    ui.label(
                        egui::RichText::new(t("settings.updates.no_update")).color(th.subtext0),
                    );
                }
            }
            ui.end_row();

            ui.label(t("settings.updates.last_checked"));
            ui.label(format_last_checked(&snapshot));
            ui.end_row();
        });

    if let Some(err) = &snapshot.last_error {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!("{}: {err}", t("update.error_label")))
                .color(th.red)
                .size(12.0),
        );
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let trigger_enabled = !snapshot.in_flight;
        if ui
            .add_enabled(
                trigger_enabled,
                egui::Button::new(t("settings.updates.check_now")),
            )
            .clicked()
        {
            update_check::trigger_check(Arc::clone(shared), OWNER, REPO, env!("CARGO_PKG_VERSION"));
        }
        if snapshot.in_flight {
            ui.label(
                egui::RichText::new(t("update.checking"))
                    .color(th.subtext0)
                    .italics(),
            );
        }
        if let Some(info) = &snapshot.latest
            && ui.button(t("settings.updates.install")).clicked()
            && let Err(e) = webbrowser::open(&info.html_url)
        {
            tracing::warn!("settings updates: open browser failed: {e}");
        }
    });

    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.updates.cli_hint"))
            .color(th.subtext0)
            .size(12.0),
    );
}

fn format_last_checked(snapshot: &UpdateStatus) -> String {
    match snapshot.last_checked {
        Some(instant) => {
            let secs = instant.elapsed().as_secs();
            if secs < 60 {
                format!("{secs}s ago")
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else {
                format!("{}h ago", secs / 3600)
            }
        }
        None => t("update.never_checked").to_string(),
    }
}
