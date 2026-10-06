//! Foundations의 결정 기록 Spec: 그림자 두 개, 반 픽셀 글자 크기, tinted 상자 계수, 남은 구조 치수.
//! 시안 `foundations.jsx`의 같은 Spec을 옮긴다. 표 열 폭·예제 폭은 전시용 치수다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, MenuItemVariant, menu_item};

use crate::catalog::spec::{StageVariant, TokenChip, do_, dont, meta, note, stage};
use crate::catalog::widgets::dialog;

/// 시안 FloatMenu의 폭.
const FLOAT_MENU_W: LogicalPx = LogicalPx(180.0);
/// 시안 FloatModal 무대(scrim) 크기.
const FLOAT_MODAL_STAGE_W: LogicalPx = LogicalPx(300.0);
const FLOAT_MODAL_STAGE_H: LogicalPx = LogicalPx(148.0);
/// scrim 무대 안 모달 카드의 폭.
const FLOAT_MODAL_W: LogicalPx = LogicalPx(220.0);
/// 모달 카드를 scrim 무대 가운데에 두는 위 여백. 카드 높이에 맞춘 전시 값이다.
const FLOAT_MODAL_TOP: LogicalPx = LogicalPx(36.0);

/// 반 픽셀 표의 열 폭: id · 이전 값 · 현재 값. 위치 설명 열은 남는 폭을 쓴다.
const HALF_ID_COL: LogicalPx = LogicalPx(26.0);
const HALF_WAS_COL: LogicalPx = LogicalPx(200.0);
const HALF_NOW_COL: LogicalPx = LogicalPx(330.0);
/// 반 픽셀 표의 위치 설명 열. 시안은 남는 폭(1fr)이며 갤러리 문서 칸(최대 1080)에 맞춘 값이다.
const HALF_PLACE_COL: LogicalPx = LogicalPx(260.0);

/// 구조 치수 표의 열 폭: popup · 현재 코드 · 정본 W×H · 메모.
const STRUCT_COLS: [LogicalPx; 4] = [
    LogicalPx(160.0),
    LogicalPx(96.0),
    LogicalPx(120.0),
    LogicalPx(300.0),
];

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

fn text(ui: &mut egui::Ui, s: &str, size: LogicalPx, color: egui::Color32) {
    ui.label(egui::RichText::new(s).size(size.value()).color(color));
}

fn mono(ui: &mut egui::Ui, s: &str, size: LogicalPx, color: egui::Color32) {
    ui.label(
        egui::RichText::new(s)
            .monospace()
            .size(size.value())
            .color(color),
    );
}

/// 고정 폭 칸 하나. 칸 안의 글자는 줄바꿈하지 않고 자른다.
fn cell(ui: &mut egui::Ui, w: LogicalPx, add: impl FnOnce(&mut egui::Ui)) {
    // 칸 높이를 한 줄 높이로 고정해 행 안의 글자가 같은 가운데 선에 놓이게 한다.
    let line_h = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(w.value(), line_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(w.value());
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            add(ui);
        },
    );
}

/// 시안 `Lbl`: mono 토큰 이름과 muted 설명.
fn token_label(ui: &mut egui::Ui, theme: &Theme, tok: &str, desc: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        mono(ui, tok, theme.font_size_term_sm, ec(theme.text_primary()));
        text(ui, desc, theme.font_size_caption, ec(theme.text_muted()));
    });
}

/// 시안 FloatMenu: 고정된 팝오버 그림자를 가진 메뉴. 첫 행은 hover 상태다.
fn float_menu(ui: &mut egui::Ui, theme: &Theme) {
    dialog::frame_card_popover(
        ui,
        theme,
        FLOAT_MENU_W,
        theme.surface_raised().to_egui(),
        |ui| {
            dialog::region_sym(ui, theme.spacing_xs, theme.spacing_xs, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, label) in ["Split pane", "New tab", "Rename…"].into_iter().enumerate() {
                    // 첫 행은 hover 상태로 보여 준다. 글자 아래에 깔리도록 자리를 먼저 잡는다.
                    let under = ui.painter().add(egui::Shape::Noop);
                    let resp = menu_item(
                        ui,
                        theme,
                        None,
                        label,
                        None,
                        MenuItemVariant::Normal,
                        false,
                        true,
                    );
                    if i == 0 {
                        ui.painter().set(
                            under,
                            egui::Shape::rect_filled(
                                resp.rect,
                                theme.corner_radius_sm.value(),
                                theme.overlay_hover().to_egui_premultiplied(),
                            ),
                        );
                    }
                }
            });
        },
    );
}

/// 시안 FloatModal: scrim 위 가운데에 뜬 모달 카드.
fn float_modal(ui: &mut egui::Ui, theme: &Theme) {
    dialog::scrim_backdrop(
        ui,
        theme,
        FLOAT_MODAL_STAGE_W,
        FLOAT_MODAL_STAGE_H,
        FLOAT_MODAL_TOP,
        |ui| {
            dialog::frame_card(ui, theme, FLOAT_MODAL_W, dialog::panel_fill(theme), |ui| {
                dialog::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                    ui.label(
                        egui::RichText::new("Delete workspace")
                            .strong()
                            .size(theme.font_size_body.value())
                            .color(ec(theme.text_primary())),
                    );
                });
                let w = ui.available_width();
                let (line, _) = ui.allocate_exact_size(
                    egui::vec2(w, theme.border_width.value()),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(line, 0.0, theme.separator.to_egui_premultiplied());
                dialog::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                            Button::new("Delete")
                                .variant(ButtonVariant::Danger)
                                .size(ControlSize::Sm)
                                .show(ui, theme);
                            Button::new("Cancel")
                                .variant(ButtonVariant::Ghost)
                                .size(ControlSize::Sm)
                                .show(ui, theme);
                        });
                    });
                });
            });
        },
    );
}

/// Foundations › Elevation — 그림자는 두 개이고 표면과 바닥의 관계가 하나를 고른다.
pub fn draw_shadows(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            float_menu(ui, theme);
            token_label(
                ui,
                theme,
                "shadow-popover",
                "0 6px 18px · 40% — anchored, scrim-less",
            );
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            float_modal(ui, theme);
            token_label(
                ui,
                theme,
                "shadow-modal",
                "0 20px 60px · 55% — centered, scrim-backed",
            );
        });
    });
    meta(
        ui,
        theme,
        &[
            (
                "popover surfaces",
                "search bar · tools menu · context menu · tooltip · autocomplete · multiselect menu · banner + its more-menu · modifier-hint · tutorial callout · switch-number overlay",
            ),
            (
                "modal surfaces",
                "command palette · ports · remote · settings · plugins · preset editor · git viewer · clipboard viewer · DAG popup · centered dialogs",
            ),
            ("not listed", "no shadow"),
            ("layers", "single — never stacked"),
            ("color", "pure black alpha only — no tinted shadow"),
            (
                "geometry",
                "integer px, non-negative spread, off the 4px grid on purpose",
            ),
            ("themes", "one value for mocha and latte"),
            (
                "transition",
                "none — geometry is fixed; banner / modifier-hint fade by opacity only",
            ),
        ],
        &[
            TokenChip::without_color("shadow-popover", "anchored overlays"),
            TokenChip::without_color("shadow-modal", "scrim-backed surfaces"),
            TokenChip::new(
                "scrim-bg",
                "the dim behind a modal",
                theme.scrim().to_egui(),
            ),
        ],
    );
    do_(
        ui,
        theme,
        "Do reference the token. Every floating surface in this system resolves to one of these two values — a hand-written box-shadow is how a third shadow gets into the product.",
    );
    dont(
        ui,
        theme,
        "Don't put a shadow on an anchored surface that already sits inside a shadowed one (a menu opened inside a modal), and don't stack layers to make a \"bigger\" lift.",
    );
    note(
        ui,
        theme,
        "One sanctioned exception lives outside this pair: titlebar-csd-shadow (0 18px 50px -8px) is OS window-frame chrome, and its negative spread is functional — it keeps the falloff inside the 8px band that carries the resize edges.",
    );
}

/// 반 픽셀 결정 표의 행: (id, 위치, 이전 값, 현재 값).
const HALF_PIXEL_ROWS: &[(&str, &str, &str, &str)] = &[
    (
        "T1",
        "Plugins segment label / badge / count",
        "12.5 / 9.5 / 10.5",
        "12 / 10 / 10",
    ),
    (
        "T2",
        "Plugins Attention title / reason / fingerprint / label",
        "13.5 / 12.5 / 11.5 / 10.5",
        "13 / 12 / 11 / 10",
    ),
    (
        "T3",
        "sidebar notification badge number",
        "9.5",
        "10 (micro)",
    ),
    (
        "T4",
        "command palette hint",
        "10.5",
        "11 (caption — it is read, not counted)",
    ),
    (
        "T5",
        "first-run brand title / warning",
        "30 / 12.5",
        "30 kept as font-size-brand-display / 12",
    ),
    (
        "T6",
        "clipboard image glyph",
        "30",
        "28 — icon family, existing exception",
    ),
];

/// Foundations › Type — 반 픽셀 크기는 읽는 글자는 올리고 숫자 라벨은 내려 맞춘다.
pub fn draw_half_pixel(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            for (id, place, was, now) in HALF_PIXEL_ROWS {
                egui::Frame::new()
                    .fill(ec(theme.bg_panel()))
                    .stroke(egui::Stroke::new(
                        theme.border_width.value(),
                        ec(theme.border_default()),
                    ))
                    .corner_radius(theme.corner_radius.value())
                    .inner_margin(egui::Margin::symmetric(
                        theme.spacing_sm.value() as i8,
                        theme.spacing_sm.value() as i8,
                    ))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                            cell(ui, HALF_ID_COL, |ui| {
                                mono(ui, id, theme.font_size_caption, ec(theme.text_muted()))
                            });
                            cell(ui, HALF_PLACE_COL, |ui| {
                                text(
                                    ui,
                                    place,
                                    theme.font_size_term_sm,
                                    ec(theme.text_secondary()),
                                )
                            });
                            cell(ui, HALF_WAS_COL, |ui| {
                                mono(ui, was, theme.font_size_caption, ec(theme.text_muted()))
                            });
                            cell(ui, HALF_NOW_COL, |ui| {
                                mono(ui, now, theme.font_size_caption, ec(theme.text_primary()))
                            });
                        });
                    });
            }
        });
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xl.value();
            for zoom in [0.85_f32, 1.0, 1.2] {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                    mono(
                        ui,
                        &format!("ui_scale {zoom}"),
                        theme.font_size_micro,
                        ec(theme.text_muted()),
                    );
                    for size in [
                        theme.font_size_body,
                        theme.font_size_term_sm,
                        theme.font_size_caption,
                        theme.font_size_micro,
                    ] {
                        let px = (size.value() * zoom).round();
                        text(
                            ui,
                            &format!("Needs attention — {px}px"),
                            LogicalPx(px),
                            ec(theme.text_secondary()),
                        );
                    }
                });
            }
        });
    });
    meta(
        ui,
        theme,
        &[
            (
                "rule",
                "read text → snap up · numeric micro-label → snap down",
            ),
            ("scale used", "13 body · 12 term-sm · 11 caption · 10 micro"),
            ("new primitive", "font-size-30 (branding exception, named)"),
            ("glyphs", "judged on the icon family, not the type scale"),
            (
                "zoom",
                "integers at 0.85 / 1 / 1.2 — no .5 survives round()",
            ),
        ],
        &[
            TokenChip::without_color("font-size-brand-display", "first-run brand title (30)"),
            TokenChip::without_color("font-size-caption", "11 — hints"),
            TokenChip::without_color("font-size-micro", "10 — badge counts"),
        ],
    );
}

/// tinted 상자 한 줄: 네 accent를 같은 계수로 그린다.
fn tint_row(ui: &mut egui::Ui, theme: &Theme, label: &str, fill: bool, border: bool) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        text(ui, label, theme.font_size_caption, ec(theme.text_muted()));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            for (name, accent) in [
                ("info", theme.accent_info()),
                ("warning", theme.accent_warning()),
                ("danger", theme.accent_danger()),
                ("success", theme.accent_success()),
            ] {
                let accent = accent.to_egui();
                let font = egui::FontId::monospace(theme.font_size_caption.value());
                let galley = ui.painter().layout_no_wrap(name.to_string(), font, accent);
                let size = egui::vec2(
                    galley.size().x + theme.spacing_sm.value() * 2.0,
                    theme.item_height_tab.value(),
                );
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let p = ui.painter();
                let r = theme.corner_radius.value();
                if fill {
                    p.rect_filled(rect, r, accent.gamma_multiply(theme.tint_fill_alpha()));
                }
                if border {
                    p.rect_stroke(
                        rect,
                        r,
                        egui::Stroke::new(
                            theme.border_width.value(),
                            accent.gamma_multiply(theme.tint_border_alpha()),
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
                let pos = egui::pos2(
                    rect.left() + theme.spacing_sm.value(),
                    rect.center().y - galley.size().y * 0.5,
                );
                p.galley(pos, galley, accent);
            }
        });
    });
}

/// Foundations › Spacing — tinted 상자는 채움 12%, 테두리 36% 하나의 계수를 쓴다.
pub fn draw_tint(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        tint_row(ui, theme, "fill + border — the default", true, true);
        tint_row(ui, theme, "fill only — warning callout", true, false);
        tint_row(ui, theme, "border only — remote chip", false, true);
    });
    meta(
        ui,
        theme,
        &[
            ("fill", "0.12 — tint-fill-alpha"),
            ("edge", "0.36 — tint-border-alpha"),
            ("retired", "0.14/0.45 · 0.12/0.35 · 0.11/0.36"),
            (
                "partial uses",
                "fill-only · border-only (same coefficients)",
            ),
            (
                "affected",
                "file-picker info badge · keybinding IE notice & migrate card · Plugins error box / Installed badge / Attention banner · warning callout · remote & script badges · chip remote tag",
            ),
            (
                "clipboard inset (I1)",
                "14 → 12 (space-md) — the 4px grid wins; no 14 semantic is opened",
            ),
        ],
        &[
            TokenChip::without_color("tint-fill-alpha", "every tinted fill"),
            TokenChip::without_color("tint-border-alpha", "every tinted edge"),
            TokenChip::without_color("space-md", "clipboard header / type-bar / footer inset"),
        ],
    );
    note(
        ui,
        theme,
        "Opacity coefficients are primitives (opacity-tint-fill / -border) with semantic aliases, so a per-role override stays possible later without re-scattering literals.",
    );
}

/// 구조 치수 표의 행: (popup, 현재 코드, 정본 W×H, 메모).
const STRUCT_ROWS: &[(&str, &str, &str, &str)] = &[
    (
        "notifications",
        "350 × 400",
        "352 × 400",
        "W snapped to grid",
    ),
    (
        "script changed confirm",
        "360 × 150",
        "360 × 152",
        "H snapped to grid",
    ),
    ("search bar", "360 × 28", "360 × 28", ""),
    (
        "info modal",
        "440 × 160",
        "440 × 140..360",
        "named: info-modal-width / -max-height",
    ),
    ("approval", "480 × 240", "480 × 240", ""),
    ("file picker", "640 × 480", "640 × 480", ""),
    ("port scanner", "660 × 520", "660 × 520", ""),
    ("command palette", "540 × 412", "540 × 412", ""),
    (
        "DAG list",
        "560 × 460",
        "560 × 460",
        "already dag-popup-width / -height",
    ),
    ("remote tool", "520 × 460", "520 × 460", ""),
    ("remote attach", "680 × 460", "680 × 460", ""),
    ("transfer progress", "400 × 180", "400 × 180", ""),
    ("transfer error", "400 × 200", "400 × 200", ""),
    ("preset apply ×3", "360 × 320", "360 × 320", ""),
];

/// 표 한 행. 머리 행은 굵은 muted 글자와 border-strong 밑줄을 쓴다.
fn struct_row(ui: &mut egui::Ui, theme: &Theme, cols: [&str; 4], head: bool) {
    let resp = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, (s, w)) in cols.iter().zip(STRUCT_COLS).enumerate() {
            cell(ui, w, |ui| {
                ui.add_space(theme.spacing_md.value());
                let size = theme.font_size_caption.value();
                let rich = egui::RichText::new(*s).size(size);
                let rich = if head {
                    rich.strong().color(ec(theme.text_muted()))
                } else if i == 1 || i == 3 {
                    let rich = rich.color(ec(theme.text_muted()));
                    if i == 1 { rich.monospace() } else { rich }
                } else if i == 2 {
                    let changed = cols[1] != cols[2];
                    rich.monospace().color(if changed {
                        ec(theme.accent_warning())
                    } else {
                        ec(theme.text_primary())
                    })
                } else {
                    rich.color(ec(theme.text_secondary()))
                };
                ui.add_space(theme.spacing_xs.value());
                ui.label(rich);
            });
        }
    });
    let r = resp.response.rect;
    let line = if head {
        theme.border_strong()
    } else {
        theme.border_default()
    };
    ui.painter().hline(
        r.x_range(),
        r.bottom(),
        egui::Stroke::new(theme.border_width.value(), ec(line)),
    );
}

/// Foundations › Spacing — 토큰이 없던 구조 치수의 결정.
pub fn draw_structural(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Solo, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        struct_row(
            ui,
            theme,
            ["popup", "code today", "canonical W × H", "note"],
            true,
        );
        for (n, a, b, memo) in STRUCT_ROWS {
            struct_row(ui, theme, [n, a, b, memo], false);
        }
    });
    meta(
        ui,
        theme,
        &[
            ("A centre glyph", "22 → 24 · center-state-glyph-size"),
            ("B scripts glyph", "26 → 24 · merged with A"),
            ("C block height", "100 / 120 → none (centres in region)"),
            ("D toolbar 32", "→ toolbar-height (new semantic)"),
            (
                "D scroll max 200",
                "tutorial topic list · no DTCG token · hand-written Theme accessor · scales",
            ),
            ("D col min 200", "withdrawn — = port Process col (D7)"),
            ("E popup sizes", "outside tokens · 350 → 352 · 150 → 152"),
            (
                "ui_scale",
                "A–D scale (incl. the tutorial cap) · E popup sizes do not",
            ),
        ],
        &[
            TokenChip::without_color("icon-size-lg", "→ size-24 (new tier)"),
            TokenChip::without_color("toolbar-height", "→ size-32 (new)"),
            TokenChip::without_color("git-toolbar-height", "→ toolbar-height"),
        ],
    );
    note(
        ui,
        theme,
        "Moving A · B · D onto tokens puts them on ui_scale: at 0.85 / 1.2 the pixels change (glyph 20.4 / 28.8). That is intended — they are UI chrome, not screen frames.",
    );
}
