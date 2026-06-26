//! `StatusDot` primitive specimen — 디자인(4) `components/feedback/StatusDot` 카드.
//!
//! running(pulse) · idle · agent · waiting · error + reduced-motion. 하단 `meta` 로
//! 치수/토큰 노출.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{StatusKind, status_dot};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "states", |ui| {
            status_dot(ui, theme, StatusKind::Running, "running", true, false);
            status_dot(ui, theme, StatusKind::Agent, "agent", true, false);
            status_dot(ui, theme, StatusKind::Waiting, "waiting", false, false);
            status_dot(ui, theme, StatusKind::Idle, "idle", false, false);
            status_dot(ui, theme, StatusKind::Error, "error", false, false);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("dot", "status-dot-size"),
            ("pulse", "ring on running / agent"),
            ("agent", "accent-agent"),
        ],
        &[
            TokenChip::new(
                "accent-success",
                "running",
                egui::Color32::from(theme.accent_success()),
            ),
            TokenChip::new(
                "accent-agent",
                "agent",
                egui::Color32::from(theme.accent_agent()),
            ),
            TokenChip::new(
                "accent-warning",
                "waiting",
                egui::Color32::from(theme.accent_warning()),
            ),
            TokenChip::new(
                "accent-danger",
                "error",
                egui::Color32::from(theme.accent_danger()),
            ),
        ],
    );
}
