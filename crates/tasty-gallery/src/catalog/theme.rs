//! Foundations 색 specimen — 디자인(4) Foundations 의 색 3 Spec.
//!
//! 디자인은 색을 토큰 grid 가 아니라 **역할 데모**로 보여준다. 한 `draw` 가 아니라
//! 3 개로 분할:
//! - [`elevation`] — surface ramp (bg-app→…→surface-active 중첩, 그림자 없이 tint 로만 깊이)
//! - [`text`] — text tint 위계 (primary→placeholder) + placeholder Input
//! - [`accents`] — accent 역할 매핑 (primary/info/success/warning/danger/agent + demo 위젯)
//!
//! 모든 색·치수는 `Theme` 토큰에서만 가져온다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{status_dot, tag, Input, StatusKind, TagVariant};

use crate::catalog::spec::{meta, note, stage, StageVariant, TokenChip};

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

// ── elevation (surface ramp) ────────────────────────────────────────────────

/// Spec "Depth reads through surface tint, never shadow".
pub fn elevation(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        ui.scope(|ui| {
            ui.set_max_width(theme.measure_sm.value());
            // bg-app → bg-sidebar → bg-panel → surface-raised 중첩, 깊이는 tint 로만.
            ramp(ui, theme, ec(theme.bg_app()), "bg-app", |ui| {
                ramp(ui, theme, ec(theme.bg_sidebar()), "bg-sidebar", |ui| {
                    ramp(ui, theme, ec(theme.bg_panel()), "bg-panel", |ui| {
                        ramp(ui, theme, ec(theme.surface_raised()), "surface-raised", |ui| {
                            tile(ui, theme, ec(theme.surface_hover()), "surface-hover");
                            ui.add_space(theme.spacing_sm.value());
                            tile(ui, theme, ec(theme.surface_active()), "surface-active");
                        });
                    });
                });
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("ramp", "crust → … → surface2"),
            ("border", "1px border-default"),
            ("shadow", "none (UI surfaces)"),
            ("stack width", "measure-sm 300"),
        ],
        &[
            TokenChip::new("bg-app", "outermost frame", ec(theme.bg_app())),
            TokenChip::new("bg-sidebar", "nav / sidebar", ec(theme.bg_sidebar())),
            TokenChip::new("bg-panel", "content panel", ec(theme.bg_panel())),
            TokenChip::new("surface-raised", "raised control", ec(theme.surface_raised())),
            TokenChip::new("surface-hover", "hover overlay", ec(theme.surface_hover())),
            TokenChip::new("surface-active", "active / selected", ec(theme.surface_active())),
        ],
    );
    note(
        ui,
        theme,
        "깊이는 그림자가 아니라 surface tint 의 한 단계 차이로만 읽힌다 — 터미널 UI 는 평면이다.",
    );
}

/// 한 단계 ramp 프레임 — fill + 1px border + 토큰 라벨 + 중첩 콘텐츠.
fn ramp(ui: &mut egui::Ui, theme: &Theme, fill: egui::Color32, label: &str, inner: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(theme.border_width.value(), ec(theme.border_default())))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(ec(theme.text_muted())),
            );
            ui.add_space(theme.spacing_sm.value());
            inner(ui);
        });
}

/// ramp 안쪽 tile (hover/active 행) — fill + 라벨, 풀폭.
fn tile(ui: &mut egui::Ui, theme: &Theme, fill: egui::Color32, label: &str) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(egui::Margin {
            left: theme.spacing_md.value() as i8,
            right: theme.spacing_md.value() as i8,
            top: theme.spacing_sm.value() as i8,
            bottom: theme.spacing_sm.value() as i8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(label)
                    .monospace()
                    .size(theme.font_size_caption.value())
                    .color(ec(theme.text_secondary())),
            );
        });
}

// ── text (hierarchy by color) ────────────────────────────────────────────────

/// Spec "Hierarchy by text color, on any surface".
pub fn text(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        text_row(ui, theme, ec(theme.text_primary()), "text-primary", "Primary — titles, active labels");
        text_row(ui, theme, ec(theme.text_secondary()), "text-secondary", "Secondary — body, descriptions");
        text_row(ui, theme, ec(theme.text_muted()), "text-muted", "Muted — captions, meta, hints");
        text_row(ui, theme, ec(theme.text_disabled()), "text-disabled", "Disabled — inert controls");
        // placeholder 는 Input 의 빈 상태로 시연.
        let mut buf = String::new();
        Input::new()
            .placeholder("Placeholder — text-placeholder")
            .width(theme.toast_max_width.value())
            .show(ui, theme, &mut buf);
    });

    meta(
        ui,
        theme,
        &[
            ("size", "body 13px, all tints"),
            ("contrast", "primary ≥ 4.5:1 on any surface"),
        ],
        &[
            TokenChip::new("text-primary", "titles", ec(theme.text_primary())),
            TokenChip::new("text-secondary", "body", ec(theme.text_secondary())),
            TokenChip::new("text-muted", "captions", ec(theme.text_muted())),
            TokenChip::new("text-disabled", "inert", ec(theme.text_disabled())),
            TokenChip::new("text-placeholder", "empty input", ec(theme.text_placeholder())),
        ],
    );
}

fn text_row(ui: &mut egui::Ui, theme: &Theme, color: egui::Color32, tok: &str, sample: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        ui.label(egui::RichText::new(sample).size(theme.font_size_body.value()).color(color));
        ui.label(
            egui::RichText::new(tok)
                .monospace()
                .size(theme.font_size_micro.value())
                .color(ec(theme.text_muted())),
        );
    });
}

// ── accents (roles, not decoration) ──────────────────────────────────────────

/// Spec "Accents map to roles, not decoration".
pub fn accents(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        accent_row(ui, theme, ec(theme.accent_primary()), "accent-primary", "primary action", |ui, theme| {
            tasty_ui_widgets::Button::new("Open").show(ui, theme);
        });
        accent_row(ui, theme, ec(theme.accent_info()), "accent-info", "informational", |ui, theme| {
            tag(ui, theme, "info", TagVariant::Accent, false);
        });
        accent_row(ui, theme, ec(theme.accent_success()), "accent-success", "running / ok", |ui, theme| {
            status_dot(ui, theme, StatusKind::Running, "running", false, false);
        });
        accent_row(ui, theme, ec(theme.accent_warning()), "accent-warning", "caution / readonly", |ui, theme| {
            tag(ui, theme, "readonly", TagVariant::Warning, true);
        });
        accent_row(ui, theme, ec(theme.accent_danger()), "accent-danger", "destructive / error", |ui, theme| {
            tasty_ui_widgets::Button::new("Delete")
                .variant(tasty_ui_widgets::ButtonVariant::Danger)
                .show(ui, theme);
        });
        accent_row(ui, theme, ec(theme.accent_agent()), "accent-agent", "agent — always mauve", |ui, theme| {
            status_dot(ui, theme, StatusKind::Agent, "agent", false, false);
            tag(ui, theme, "plugin", TagVariant::Agent, false);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("fill text", "text-on-accent"),
            ("focus ring", "2px accent-primary"),
            ("agent", "mauve = always agent, never decorative"),
        ],
        &[
            TokenChip::new("accent-primary", "primary", ec(theme.accent_primary())),
            TokenChip::new("accent-info", "info", ec(theme.accent_info())),
            TokenChip::new("accent-success", "success", ec(theme.accent_success())),
            TokenChip::new("accent-warning", "warning", ec(theme.accent_warning())),
            TokenChip::new("accent-danger", "danger", ec(theme.accent_danger())),
            TokenChip::new("accent-agent", "agent", ec(theme.accent_agent())),
        ],
    );
}

/// accent 한 행 — [16px swatch] [role 150px] [demo 위젯].
fn accent_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    swatch: egui::Color32,
    tok: &str,
    role: &str,
    demo: impl FnOnce(&mut egui::Ui, &Theme),
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        // 16px 색 swatch.
        let s = theme.spacing_lg.value();
        let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
        ui.painter().rect_filled(r, theme.corner_radius_sm.value(), swatch);
        // role 라벨 (token + 용도) 고정폭.
        ui.allocate_ui(egui::vec2(theme.tab_width.value(), s), |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(tok)
                        .monospace()
                        .size(theme.font_size_micro.value())
                        .color(ec(theme.text_primary())),
                );
                ui.label(
                    egui::RichText::new(role)
                        .size(theme.font_size_micro.value())
                        .color(ec(theme.text_muted())),
                );
            });
        });
        demo(ui, theme);
    });
}
