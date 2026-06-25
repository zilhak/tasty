//! Foundations 색 specimen — 디자인(4) Foundations 의 색 3 Spec.
//!
//! 디자인은 색을 토큰 grid 가 아니라 **역할 데모**로 보여준다. 한 `draw` 가 아니라
//! 4 개로 분할:
//! - [`elevation`] — surface ramp (bg-app→…→surface-active 중첩, 그림자 없이 tint 로만 깊이)
//! - [`text`] — text tint 위계 (primary→placeholder) + placeholder Input
//! - [`accents`] — accent 역할 매핑 (primary/info/success/warning/danger/agent + demo 위젯)
//! - [`terminal`] — 터미널 콘텐츠 셀 색 (ANSI 16 + selection/vi-cursor/search 상태 채움),
//!   UI accent role 과 구분되는 별개 축
//!
//! 모든 색·치수는 `Theme` 토큰에서만 가져온다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{status_dot, tag, Input, StatusKind, TagVariant};

use crate::catalog::spec::{dont, meta, note, stage, StageVariant, TokenChip};

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

// ── terminal / ANSI palette ──────────────────────────────────────────────────

/// Spec "The colors a terminal cell paints with — not UI chrome".
///
/// 터미널 셀이 칠하는 16 ANSI(normal 8 + bright 8) + Tasty 가 셀 위에 직접 칠하는
/// 4 상태 채움(selection / vi cursor / search). UI accent role 과 같은 hue 를
/// 공유해도 **다른 축** — accent-success(green) 와 ansi-green 은 서로 다른 역할이다.
pub fn terminal(ui: &mut egui::Ui, theme: &Theme) {
    let normal: [(&str, &str, egui::Color32); 8] = [
        ("ansi-black", "30", ec(theme.ansi_black)),
        ("ansi-red", "31", ec(theme.ansi_red)),
        ("ansi-green", "32", ec(theme.ansi_green)),
        ("ansi-yellow", "33", ec(theme.ansi_yellow)),
        ("ansi-blue", "34", ec(theme.ansi_blue)),
        ("ansi-magenta", "35", ec(theme.ansi_magenta)),
        ("ansi-cyan", "36", ec(theme.ansi_cyan)),
        ("ansi-white", "37", ec(theme.ansi_white)),
    ];
    let bright: [(&str, &str, egui::Color32); 8] = [
        ("ansi-bright-black", "90", ec(theme.ansi_bright_black)),
        ("ansi-bright-red", "91", ec(theme.ansi_bright_red)),
        ("ansi-bright-green", "92", ec(theme.ansi_bright_green)),
        ("ansi-bright-yellow", "93", ec(theme.ansi_bright_yellow)),
        ("ansi-bright-blue", "94", ec(theme.ansi_bright_blue)),
        ("ansi-bright-magenta", "95", ec(theme.ansi_bright_magenta)),
        ("ansi-bright-cyan", "96", ec(theme.ansi_bright_cyan)),
        ("ansi-bright-white", "97", ec(theme.ansi_bright_white)),
    ];

    stage(ui, theme, StageVariant::Column, |ui| {
        // ANSI 16 — normal / bright 를 hue 로 짝지어 2열.
        ui.columns(2, |cols| {
            ansi_col(&mut cols[0], theme, "normal · SGR 30–37", &normal);
            ansi_col(&mut cols[1], theme, "bright · SGR 90–97", &bright);
        });

        // 터미널 상태 채움 — 가짜 터미널 줄 위에 시연 + 토큰 행.
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("TERMINAL STATE FILLS — PAINTED ONTO CELLS")
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(ec(theme.text_muted())),
            );
            ui.add_space(theme.spacing_sm.value());
            faux_terminal(ui, theme);
            ui.add_space(theme.spacing_sm.value());
            sem_row(ui, theme, ec(theme.selection_bg), "selection-bg", "mouse / keyboard selection region");
            sem_row(ui, theme, ec(theme.vi_cursor_bg), "vi-cursor-bg", "vi-mode block cursor");
            sem_row(ui, theme, ec(theme.search_match_bg), "search-match-bg", "search matches in scrollback");
            sem_row(
                ui,
                theme,
                ec(theme.search_match_active_bg),
                "search-match-active-bg",
                "the current / active match",
            );
        });
    });

    meta(
        ui,
        theme,
        &[
            ("set", "16 ANSI (8 normal + 8 bright)"),
            ("source", "derived from the Catppuccin palette"),
            ("scope", "terminal cells — not UI chrome"),
            ("state fills", "selection · vi cursor · search"),
        ],
        &[
            TokenChip::new("ansi-red", "SGR 31 red", ec(theme.ansi_red)),
            TokenChip::new("ansi-green", "SGR 32 green", ec(theme.ansi_green)),
            TokenChip::new("ansi-bright-blue", "SGR 94 br.blue", ec(theme.ansi_bright_blue)),
            TokenChip::new("selection-bg", "selection fill", ec(theme.selection_bg)),
            TokenChip::new("search-match-active-bg", "active match", ec(theme.search_match_active_bg)),
        ],
    );
    dont(
        ui,
        theme,
        "Don't use an ANSI color for UI chrome or an accent for terminal output — \
         ansi-green and accent-success are different roles that only share a hue.",
    );
}

/// ANSI 한 열 — micro uppercase 라벨 + 8 개 `ansi_row`.
fn ansi_col(ui: &mut egui::Ui, theme: &Theme, heading: &str, rows: &[(&str, &str, egui::Color32)]) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new(heading.to_uppercase())
                .monospace()
                .size(theme.font_size_micro.value())
                .color(ec(theme.text_muted())),
        );
        for (tok, sgr, color) in rows {
            ansi_row(ui, theme, *color, tok, sgr);
        }
    });
}

/// ANSI 한 행 — [16px swatch] [token 1fr] [SGR NN 우측정렬].
fn ansi_row(ui: &mut egui::Ui, theme: &Theme, swatch: egui::Color32, tok: &str, sgr: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        swatch_box(ui, theme, swatch);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("SGR {sgr}"))
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(ec(theme.text_muted())),
            );
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(tok)
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(ec(theme.text_primary())),
                );
            });
        });
    });
}

/// 터미널 상태 채움 한 행 — [16px swatch] [token] [role muted].
fn sem_row(ui: &mut egui::Ui, theme: &Theme, swatch: egui::Color32, tok: &str, role: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        swatch_box(ui, theme, swatch);
        ui.label(
            egui::RichText::new(tok)
                .monospace()
                .size(theme.font_size_caption.value())
                .color(ec(theme.text_primary())),
        );
        ui.label(
            egui::RichText::new(role)
                .size(theme.font_size_caption.value())
                .color(ec(theme.text_muted())),
        );
    });
}

/// 16px 색 swatch (border-strong 1px).
fn swatch_box(ui: &mut egui::Ui, theme: &Theme, fill: egui::Color32) {
    let s = theme.spacing_lg.value();
    let (r, _) = ui.allocate_exact_size(egui::vec2(s, s), egui::Sense::hover());
    ui.painter().rect_filled(r, theme.corner_radius_sm.value(), fill);
    ui.painter().rect_stroke(
        r,
        theme.corner_radius_sm.value(),
        egui::Stroke::new(theme.border_width.value(), ec(theme.border_strong())),
        egui::StrokeKind::Inside,
    );
}

/// 상태 채움을 보여주는 가짜 터미널 줄 — 가장 깊은 surface(bg-app) 위 mono 텍스트.
fn faux_terminal(ui: &mut egui::Ui, theme: &Theme) {
    let fg = ec(theme.text_primary());
    let mono = theme.font_size_caption.value();
    egui::Frame::new()
        .fill(ec(theme.bg_app()))
        .stroke(egui::Stroke::new(theme.border_width.value(), ec(theme.separator)))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // 줄 1: 프롬프트.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(
                    egui::RichText::new("~/tasty")
                        .monospace()
                        .size(mono)
                        .color(ec(theme.ansi_green)),
                );
                ui.label(egui::RichText::new(" $ vi notes.md").monospace().size(mono).color(fg));
            });
            // 줄 2: selection / search-match / active-match / vi-cursor 채움.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let seg = |ui: &mut egui::Ui, txt: &str, bg: Option<egui::Color32>, color: egui::Color32| {
                    let mut rt = egui::RichText::new(txt).monospace().size(mono).color(color);
                    if let Some(b) = bg {
                        rt = rt.background_color(b);
                    }
                    ui.label(rt);
                };
                seg(ui, "The ", None, fg);
                seg(ui, "selected words", Some(ec(theme.selection_bg)), fg);
                seg(ui, " wrap the ", None, fg);
                seg(ui, "match", Some(ec(theme.search_match_bg)), fg);
                seg(ui, " and the ", None, fg);
                seg(ui, "active match", Some(ec(theme.search_match_active_bg)), fg);
                seg(ui, " sit ", None, fg);
                // vi 블록 커서: 밝은 채움 위 어두운 글리프.
                seg(ui, "h", Some(ec(theme.vi_cursor_bg)), ec(theme.text_on_accent()));
                seg(ui, "ere", None, fg);
            });
        });
}
