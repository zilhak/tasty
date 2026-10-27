//! The live notification widget in popup, fullscreen and sidebar specimens.
use crate::catalog::popup_frame::{self, TitleButtons};
use crate::catalog::spec::{self, StageVariant};
use crate::i18n::t;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    NotificationAction, NotificationLabels, NotificationPanelState, NotificationRow,
    NotificationSummary, NotificationTone, notification_panel, notification_summary,
};

fn level_labels() -> [&'static str; 3] {
    [
        t("notification_panel.errors"),
        t("notification_panel.warnings"),
        t("notification_panel.info"),
    ]
}

fn labels() -> NotificationLabels<'static> {
    NotificationLabels {
        levels: level_labels(),
        no_unread: t("notification_panel.no_unread"),
        empty: t("notification_panel.empty_message"),
        mark_all: t("button.mark_all_read"),
        mark_read: t("notification_panel.mark_read"),
        jump: t("button.jump_to_workspace"),
        closed: t("notification_panel.closed"),
        source_closed: t("notification_panel.source_closed"),
        more: t("notification_panel.more"),
        less: t("notification_panel.less"),
    }
}

fn rows() -> Vec<NotificationRow<'static>> {
    vec![
        NotificationRow {
            id: 1,
            tone: NotificationTone::Error,
            unread: true,
            title: "Build failed: cargo build exited with status 101",
            body: "error[E0308]: mismatched types\n  --> src/routes/auth.rs:42:17\nExpected Token, found &str.\n\nThe agent stopped after the failed build. Three more errors were reported in src/routes/session.rs; run cargo build in the surface to see the full output.",
            source: "api-gateway › build",
            source_closed: false,
            time: "2m",
        },
        NotificationRow {
            id: 2,
            tone: NotificationTone::Warning,
            unread: true,
            title: "Disk usage above 85%",
            body: "The nightly ETL keeps running, but the next snapshot may not fit. Free space or move old partitions.",
            source: "data-pipeline › ingest",
            source_closed: false,
            time: "9m",
        },
        NotificationRow {
            id: 3,
            tone: NotificationTone::Info,
            unread: true,
            title: "Deploy to staging finished",
            body: "4 services updated in 2m 31s.",
            source: "agents-prod › deploy",
            source_closed: false,
            time: "14m",
        },
        NotificationRow {
            id: 5,
            tone: NotificationTone::Warning,
            unread: false,
            title: "Disk usage above 85% (read)",
            body: "The nightly ETL keeps running, but the next snapshot may not fit.",
            source: "data-pipeline › ingest",
            source_closed: false,
            time: "9m",
        },
        NotificationRow {
            id: 4,
            tone: NotificationTone::Info,
            unread: false,
            title: "Script completed",
            body: "212 passed, 0 failed.",
            source: "scratch › surface 2",
            source_closed: true,
            time: "1h",
        },
    ]
}

pub(super) fn content(ui: &mut egui::Ui, th: &Theme, empty: bool, wide: bool) {
    let key = ui.id().with("notification_demo");
    let mut state = ui
        .data(|d| d.get_temp::<NotificationPanelState>(key))
        .unwrap_or_default();
    let mut read = ui
        .data(|d| d.get_temp::<std::collections::HashSet<u64>>(key.with("read")))
        .unwrap_or_default();
    let mut rows = if empty { Vec::new() } else { rows() };
    for row in &mut rows {
        if read.contains(&row.id) {
            row.unread = false;
        }
    }
    for action in notification_panel(ui, th, &rows, &labels(), &mut state, wide) {
        match action {
            NotificationAction::Read(id) | NotificationAction::Jump(id) => {
                read.insert(id);
            }
            NotificationAction::ReadAll => read.extend(rows.iter().map(|r| r.id)),
            NotificationAction::Toggle(_) => {}
        }
    }
    ui.data_mut(|d| {
        d.insert_temp(key, state);
        d.insert_temp(key.with("read"), read);
    });
}

pub fn draw(ui: &mut egui::Ui, th: &Theme) {
    spec::stage(ui, th, StageVariant::Wrap, |ui| {
        for (title, empty) in [
            ("Unread levels · expand or acknowledge", false),
            ("Empty", true),
        ] {
            spec::cluster(ui, th, title, |ui| {
                ui.push_id(title, |ui| {
                    popup_frame::draw_panel(
                        ui,
                        th,
                        t("notification_panel.window_title"),
                        th.notifications_popup_width(),
                        th.notifications_popup_height(),
                        TitleButtons::FULLSCREEN_AND_CLOSE,
                        |ui| content(ui, th, empty, false),
                    )
                });
            });
        }
    });
    spec::note(
        ui,
        th,
        "Opening does not mark read. Expand a row or use its check button; mark all only affects this window. Read and unread warning glyphs share accent-warning-glyph, including hover; the count badge keeps its separate role. Error and info retain their original colors.",
    );
}

pub fn draw_summary(ui: &mut egui::Ui, th: &Theme) {
    // Specimen frame matches the 212-wide expanded sidebar in the design kit.
    const SIDEBAR_WIDTH: LogicalPx = LogicalPx(212.0);
    spec::stage(ui, th, StageVariant::Wrap, |ui| {
        for (name, counts) in [
            ("Empty", [0, 0, 0]),
            ("Mixed", [2, 1, 14]),
            ("Error", [3, 0, 0]),
            ("Warning", [0, 2, 0]),
            ("Info", [0, 0, 8]),
            ("Overflow", [123, 100, 999]),
            ("Open", [2, 1, 14]),
        ] {
            spec::cluster(ui, th, name, |ui| {
                let props = NotificationSummary {
                    counts,
                    label: t("notification_panel.window_title"),
                    tooltip: "Unread notifications by level",
                    level_labels: level_labels(),
                    latest: if counts[0] > 0 {
                        Some((
                            NotificationTone::Error,
                            "Build failed: long title preserves its tooltip",
                            "2m",
                        ))
                    } else if counts[1] > 0 {
                        Some((NotificationTone::Warning, "Disk usage above 85%", "9m"))
                    } else if counts[2] > 0 {
                        Some((NotificationTone::Info, "Deploy to staging finished", "14m"))
                    } else {
                        None
                    },
                    open: name == "Open",
                };
                egui::Frame::new()
                    .fill(th.bg_sidebar().to_egui())
                    .show(ui, |ui| {
                        ui.set_width(SIDEBAR_WIDTH.value());
                        notification_summary(ui, th, &props, false);
                    });
                egui::Frame::new()
                    .fill(th.bg_sidebar().to_egui())
                    .show(ui, |ui| {
                        notification_summary(ui, th, &props, true);
                    });
            });
        }
    });
}
