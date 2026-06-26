//! Badge · Tag · Kbd primitive specimen — 디자인(4) `components/chips/*` 3 카드.
//!
//! 디자인 components 페이지의 chips 섹션은 Badge / Tag / Kbd 를 각각 독립 Spec 으로
//! 노출한다. 한 파일에서 3 draw 함수로 나눠 catalog 의 3 Spec 에 연결한다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{BadgeVariant, TagVariant, badge, badge_dot, kbd, tag};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

/// Badge — count pill + dot.
pub fn draw_badge(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "counts", |ui| {
            badge(ui, theme, "3", BadgeVariant::Danger);
            badge(ui, theme, "99+", BadgeVariant::Danger);
            badge(ui, theme, "12", BadgeVariant::Primary);
            badge(ui, theme, "new", BadgeVariant::Agent);
            badge(ui, theme, "ok", BadgeVariant::Success);
        });
        cluster(ui, theme, "dot", |ui| {
            badge_dot(ui, theme, BadgeVariant::Danger);
            badge_dot(ui, theme, BadgeVariant::Agent);
            badge_dot(ui, theme, BadgeVariant::Success);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("radius", "pill (full)"),
            ("font", "caption 11px"),
            ("dot", "status-dot-size"),
        ],
        &[
            TokenChip::new(
                "accent-danger",
                "count fill",
                egui::Color32::from(theme.accent_danger()),
            ),
            TokenChip::new(
                "accent-agent",
                "agent fill",
                egui::Color32::from(theme.accent_agent()),
            ),
            TokenChip::new(
                "font-size-caption",
                "label 11px",
                egui::Color32::from(theme.text_muted()),
            ),
        ],
    );
}

/// Tag — outlined chip + state dot.
pub fn draw_tag(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "variants", |ui| {
            tag(ui, theme, "terminal", TagVariant::Default, false);
            tag(ui, theme, "markdown", TagVariant::Accent, false);
            tag(ui, theme, "plugin", TagVariant::Agent, false);
            tag(ui, theme, "running", TagVariant::Success, true);
            tag(ui, theme, "readonly", TagVariant::Warning, true);
            tag(ui, theme, "error", TagVariant::Danger, true);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("font", "mono"),
            ("radius", "radius-sm 2"),
            ("dot", "leading status-dot"),
        ],
        &[
            TokenChip::new(
                "font-mono",
                "label face",
                egui::Color32::from(theme.text_secondary()),
            ),
            TokenChip::new(
                "border-default",
                "outline",
                egui::Color32::from(theme.border_default()),
            ),
            TokenChip::new(
                "accent-agent",
                "agent dot",
                egui::Color32::from(theme.accent_agent()),
            ),
        ],
    );
}

/// Kbd — keycaps.
pub fn draw_kbd(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "shortcuts", |ui| {
            kbd(ui, theme, "Ctrl+K");
            kbd(ui, theme, "Ctrl+Shift+N");
            kbd(ui, theme, "⌘,");
            kbd(ui, theme, "Esc");
        });
    });

    meta(
        ui,
        theme,
        &[
            ("font", "mono"),
            ("radius", "radius-sm 2"),
            ("fill", "surface-raised"),
        ],
        &[
            TokenChip::new(
                "font-mono",
                "keycap face",
                egui::Color32::from(theme.text_secondary()),
            ),
            TokenChip::new(
                "surface-raised",
                "keycap fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "border-default",
                "keycap edge",
                egui::Color32::from(theme.border_default()),
            ),
        ],
    );
}
