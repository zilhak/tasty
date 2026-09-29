//! Badge, Tag, Kbd를 각각 보여주는 예제.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    BadgeVariant, TagVariant, badge, badge_disabled, badge_dot, disabled_chip_scope, kbd, tag,
    tag_disabled,
};

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
        // 시안 Badge `disabled` — 모든 variant가 중립 채움과 disabled ink 한 벌이다.
        cluster(ui, theme, "disabled", |ui| {
            badge_disabled(ui, theme, "3");
            badge_disabled(ui, theme, "12");
            badge_disabled(ui, theme, "ok");
            disabled_chip_scope(ui, |ui| {
                badge_dot(ui, theme, BadgeVariant::Danger);
            });
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
                "badge-danger-bg",
                "default",
                egui::Color32::from(theme.badge_danger_bg()),
            ),
            TokenChip::new(
                "badge-agent-bg",
                "agent",
                egui::Color32::from(theme.badge_agent_bg()),
            ),
            TokenChip::new(
                "badge-disabled-bg",
                "disabled fill",
                egui::Color32::from(theme.badge_disabled_bg()),
            ),
            TokenChip::without_color("font-size-caption", "label 11px"),
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
            tag(ui, theme, "main", TagVariant::Info, false);
            tag(ui, theme, "running", TagVariant::Success, true);
            tag(ui, theme, "readonly", TagVariant::Warning, true);
            tag(ui, theme, "error", TagVariant::Danger, true);
        });
        // 시안 Tag `disabled` — accent 채움과 tint 테두리가 빠지고 점도 같은 ink다.
        cluster(ui, theme, "disabled", |ui| {
            tag_disabled(ui, theme, "terminal", false);
            tag_disabled(ui, theme, "markdown", false);
            tag_disabled(ui, theme, "running", true);
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
            TokenChip::without_color("font-mono", "label face"),
            TokenChip::without_color("border-default", "outline"),
            TokenChip::new(
                "accent-info",
                "info tone",
                egui::Color32::from(theme.accent_info()),
            ),
            TokenChip::new(
                "tag-disabled-fg",
                "disabled ink",
                egui::Color32::from(theme.tag_disabled_fg()),
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
            TokenChip::without_color("font-mono", "keycap face"),
            TokenChip::new(
                "surface-raised",
                "keycap fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::without_color("border-default", "keycap edge"),
        ],
    );
}
