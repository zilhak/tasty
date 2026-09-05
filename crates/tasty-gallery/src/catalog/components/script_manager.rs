//! `script_manager` specimen — 디자인 Misc · Scripts (Lua script manager, 05).
//!
//! 권위 원본: `ui_kits/terminal/overlays/settings_window.jsx` `ScriptManager` /
//! `ScriptRow` / `ScriptPath` / `ScriptChangedBadge`. 갤러리 미러:
//! `gallery/overlays-shared.jsx` `ScriptManagerFrame({ empty })`. 매핑:
//! `design-gallery-mapping.md`.
//!
//! 정적 specimen 이라 add-card / 인라인 rename / 인라인 remove 같은 상호작용 상태는
//! 본체(`view/settings/ui/tabs/misc.rs`)가 소유하고, 여기서는 갤러리 프레임과 동일하게
//! **헤더 + 목록(bound/unbound/changed) 또는 빈 상태**만 전사한다.
//!
//! 토큰: 프레임 `bg-panel`/`border-strong`/`radius`/shadow(모달), 행 하단 `separator`,
//! name `font-size-body`(13)/semibold `text-primary`, path mono `font-size-term-sm`(12)
//! dir=`text-muted`·file=`text-secondary`, changed 배지 `accent-warning` color-mix,
//! Unbound `text-disabled`, help `font-size-caption`(11) `accent-warning`.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, IconButtonVariant, kbd};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 갤러리 프레임 최대 폭 (jsx `maxWidth: 560`). 본체는 settings content 폭을 상속하나
/// 갤러리 미러는 카드로 감싸 560 으로 bound.
const FRAME_MAX_W: LogicalPx = LogicalPx(560.0);
// 빈 상태 글리프 크기는 본체와 **같은 상수**를 읽는다(`tasty-ui-widgets::tokens`).
use tasty_ui_widgets::tokens::EMPTY_STATE_GLYPH_SIZE as EMPTY_GLYPH;
/// 행 중앙 컬럼의 name→path→help 사이 hairline 간격 (jsx `gap: 2` — 4px 그리드 하위).
const ROW_LINE_GAP: LogicalPx = LogicalPx(2.0);

/// RTL 클러스터에서 kbd 키캡이 역순으로 그려지는 것을 상쇄하려 combo 파트를 미리
/// 뒤집는다(`"Ctrl+Shift+J"` → `"J+Shift+Ctrl"` → RTL 렌더 후 화면상 정순).
fn rtl_combo(combo: &str) -> String {
    combo.split('+').rev().collect::<Vec<_>>().join("+")
}

/// 한 스크립트 행(seed). `dir`+`file` 은 중간생략 경로용, `shortcut` 빈값=Unbound.
struct Seed {
    name: &'static str,
    dir: &'static str,
    file: &'static str,
    shortcut: &'static str,
    changed: bool,
}

const SEEDS: &[Seed] = &[
    Seed {
        name: "Reformat JSON",
        dir: "~/.tasty/scripts/",
        file: "reformat-json.lua",
        shortcut: "Ctrl+Shift+J",
        changed: false,
    },
    Seed {
        name: "Tail & highlight errors",
        dir: "~/.tasty/scripts/",
        file: "tail-errors.lua",
        shortcut: "",
        changed: false,
    },
    Seed {
        name: "Deploy staging",
        dir: "~/work/ops/tasty/",
        file: "deploy-staging.lua",
        shortcut: "Ctrl+Alt+D",
        changed: true,
    },
];

/// 목록 variant — 3 seed 행(bound / unbound / changed+help).
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    frame(ui, theme, false);
    meta_note(ui, theme);
}

/// 빈 상태 variant.
pub fn draw_empty(ui: &mut egui::Ui, theme: &Theme) {
    frame(ui, theme, true);
    spec::note(
        ui,
        theme,
        "Empty state — same tone as the FileHandler / Explorer favorites empty states: \
         centered glyph, a title, and an Add-script prompt.",
    );
}

fn frame(ui: &mut egui::Ui, theme: &Theme, empty: bool) {
    let width = LogicalPx(ui.available_width()).min(FRAME_MAX_W);
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        kit::frame_card(ui, theme, width, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                header(ui, theme);
                if empty {
                    empty_state(ui, theme);
                } else {
                    // 행 리스트 — 세로 적층, 간격 0(각 행 하단 separator 가 구분).
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for s in SEEDS {
                            script_row(ui, theme, s);
                        }
                    });
                }
            });
        });
    });
}

fn header(ui: &mut egui::Ui, theme: &Theme) {
    ui.horizontal_top(|ui| {
        // 좌: 제목 + 설명 (flex 1).
        let right_w = 96.0; // "Add script" 버튼 대략 폭 예약 (secondary sm + plus).
        let left_w = (ui.available_width() - right_w - theme.spacing_md.value()).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(left_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = ROW_LINE_GAP.value();
                ui.label(
                    egui::RichText::new("Scripts")
                        .size(theme.font_size_max.value())
                        .strong()
                        .color(theme.text_primary().to_egui()),
                );
                ui.set_max_width(theme.measure_md.value().min(left_w));
                ui.label(
                    egui::RichText::new(
                        "Register and manage Lua scripts you can run with a shortcut. \
                         Binding a trigger is done in Keybindings; each script is verified \
                         against the SHA recorded when it was added.",
                    )
                    .size(theme.font_size_term_sm.value())
                    .color(theme.text_muted().to_egui()),
                );
            },
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            Button::new("Add script")
                .variant(ButtonVariant::Secondary)
                .size(tasty_ui_widgets::ControlSize::Sm)
                .leading_icon(&|ui, rect, c| icons::PLUS.image(rect.width(), c).paint_at(ui, rect))
                .show(ui, theme);
        });
    });
}

fn script_row(ui: &mut egui::Ui, theme: &Theme, s: &Seed) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        // 좌: script 글리프 16 · text-muted · margin-top 2.
        ui.vertical(|ui| {
            ui.add_space(ROW_LINE_GAP.value());
            kit::icon(
                ui,
                icons::SCRIPT,
                theme.icon_glyph_size_md.value(),
                theme.text_muted().to_egui(),
            );
        });
        // 우측 액션 클러스터(right-to-left) — 남는 폭을 채우고 우측 정렬.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            // rightmost 부터: trash → edit → keyboard → shortcut.
            IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .size(tasty_ui_widgets::ControlSize::Sm)
                .show(ui, theme, &|ui, rect, c| {
                    icons::TRASH.image(rect.width(), c).paint_at(ui, rect)
                });
            IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .size(tasty_ui_widgets::ControlSize::Sm)
                .show(ui, theme, &|ui, rect, c| {
                    icons::EDIT.image(rect.width(), c).paint_at(ui, rect)
                });
            IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .size(tasty_ui_widgets::ControlSize::Sm)
                .show(ui, theme, &|ui, rect, c| {
                    icons::KEYBOARD.image(rect.width(), c).paint_at(ui, rect)
                });
            if s.shortcut.is_empty() {
                ui.label(
                    egui::RichText::new("Unbound")
                        .size(theme.font_size_term_sm.value())
                        .italics()
                        .color(theme.text_disabled().to_egui()),
                );
            } else {
                // 이 클러스터는 RTL 이라 kbd 키캡이 역순으로 그려진다 → 파트를 미리
                // 뒤집어 넘겨 화면상 정순(Ctrl+Shift+J)이 되게 한다.
                kbd(ui, theme, &rtl_combo(s.shortcut));
            }
            // 남은 좌측 폭 = 중앙 컬럼(name/path/help).
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = ROW_LINE_GAP.value();
                // row1 — name + changed 배지.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ui.label(
                        egui::RichText::new(s.name)
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(theme.text_primary().to_egui()),
                    );
                    if s.changed {
                        changed_badge(ui, theme);
                    }
                });
                // row2 — 경로(dir muted + file secondary).
                script_path(ui, theme, s.dir, s.file);
                // row3 — changed help.
                if s.changed {
                    ui.label(
                        egui::RichText::new(
                            "File changed since registration — you'll be asked to confirm on next run.",
                        )
                        .size(theme.font_size_caption.value())
                        .color(theme.accent_warning().to_egui()),
                    );
                }
            });
        });
    });
    // 행 하단 1px separator.
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

/// 경로 — dir(text-muted) + file(text-secondary), mono `font-size-term-sm`(12).
fn script_path(ui: &mut egui::Ui, theme: &Theme, dir: &str, file: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(
            egui::RichText::new(dir)
                .size(theme.font_size_term_sm.value())
                .monospace()
                .color(theme.text_muted().to_egui()),
        );
        ui.label(
            egui::RichText::new(file)
                .size(theme.font_size_term_sm.value())
                .monospace()
                .color(theme.text_secondary().to_egui()),
        );
    });
}

/// changed 배지 — warn 글리프 12 + "changed", mono micro(10), accent-warning
/// color-mix(40% border / 12% bg).
fn changed_badge(ui: &mut egui::Ui, theme: &Theme) {
    let warn = theme.accent_warning().to_egui();
    let micro = theme.font_size_micro.value();
    let glyph = theme.icon_glyph_size_xs.value(); // 12
    let galley = ui.painter().layout_no_wrap(
        "changed".to_owned(),
        egui::FontId::monospace(micro),
        egui::Color32::PLACEHOLDER,
    );
    let gap = theme.spacing_xs.value(); // 4 — 글리프↔라벨
    let pad_x = theme.spacing_sm.value(); // 8 좌우 패딩(디자인 padding 0 space-sm)
    let h = 16.0; // jsx height 16 (배지 고정)
    let w = pad_x * 2.0 + glyph + gap + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let radius = theme.corner_radius_sm.value();
    ui.painter()
        .rect_filled(rect, radius, warn.gamma_multiply(0.12));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), warn.gamma_multiply(0.4)),
        egui::StrokeKind::Inside,
    );
    let gy = egui::Rect::from_min_size(
        egui::pos2(rect.left() + pad_x, rect.center().y - glyph * 0.5),
        egui::vec2(glyph, glyph),
    );
    icons::ALERT_TRIANGLE.image(glyph, warn).paint_at(ui, gy);
    let pos = egui::pos2(
        rect.left() + pad_x + glyph + gap,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter().galley(pos, galley, warn);
}

fn empty_state(ui: &mut egui::Ui, theme: &Theme) {
    ui.vertical_centered(|ui| {
        ui.add_space(theme.spacing_xl.value());
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        kit::icon(ui, icons::SCRIPT, EMPTY_GLYPH, theme.text_muted().to_egui());
        ui.label(
            egui::RichText::new("No scripts registered")
                .size(theme.font_size_max.value())
                .color(theme.text_secondary().to_egui()),
        );
        ui.set_max_width(theme.measure_sm.value());
        ui.label(
            egui::RichText::new(
                "Click Add script to register a Lua script and bind it to a shortcut.",
            )
            .size(theme.font_size_term_sm.value())
            .color(theme.text_muted().to_egui()),
        );
        ui.add_space(theme.spacing_xl.value());
    });
}

fn meta_note(ui: &mut egui::Ui, theme: &Theme) {
    spec::meta(
        ui,
        theme,
        &[
            ("frame", "≤560 · bg-panel · 1px border-strong · shadow"),
            (
                "header",
                "title font-size-max semibold + muted desc · Add script (secondary sm)",
            ),
            (
                "row",
                "glyph 16 · name 13/600 · mono path (middle-elided) · Kbd/Unbound · bind·edit·trash",
            ),
            (
                "changed",
                "accent-warning badge + help line (TOFU re-confirm)",
            ),
            ("divider", "1px separator per row"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "accent-warning",
                "changed",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new("text-disabled", "Unbound", theme.text_disabled().to_egui()),
            TokenChip::new("separator", "row divider", theme.separator.to_egui()),
        ],
    );
    spec::note(
        ui,
        theme,
        "Mirrors the body ScriptManager (Settings › Misc › Scripts). Binding is owned by \
         Keybindings — this surface only shows the bound key and links into it. A row is \
         marked changed when the on-disk SHA differs from the hash recorded at registration.",
    );
}
