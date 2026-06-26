//! TreeRow · MenuItem primitive specimens — 디자인(4) `components/nav/*` 카드.
//!
//! 디자인 nav 섹션은 Tab / TreeRow / MenuItem 세 Spec. Tab 은 `prim_tab`,
//! TreeRow·MenuItem 은 이 파일의 두 draw 함수가 담당한다. 두 컴포넌트는 풀블리드
//! 패널(tight) 안에 sidebar / raised 표면을 깔고 행을 쌓는다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{MenuItemVariant, menu_item, menu_separator, tree_row};

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

thread_local! {
    static SEL: RefCell<usize> = const { RefCell::new(0) };
    static TREE_SEL: RefCell<usize> = const { RefCell::new(2) };
}

/// MenuItem — icon · label · shortcut · active · disabled · danger · separator.
pub fn draw_menu_item(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        // 디자인: surface-raised border radius padding 6 width 280.
        egui::Frame::new()
            .fill(egui::Color32::from(theme.surface_raised()))
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                egui::Color32::from(theme.border_default()),
            ))
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
            .show(ui, |ui| {
                ui.set_width(theme.measure_sm.value() - theme.spacing_lg.value());
                ui.spacing_mut().item_spacing.y = 0.0;
                SEL.with(|s| {
                    let mut sel = s.borrow_mut();
                    let items: [(glyph::MockGlyph, &str, Option<&str>); 3] = [
                        (glyph::TERMINAL, "New tab", Some("Ctrl+T")),
                        (glyph::SPLIT, "Split pane", Some("Ctrl+D")),
                        (glyph::COPY, "Copy path", None),
                    ];
                    for (i, (g, label, sc)) in items.iter().enumerate() {
                        let gg = *g;
                        let r = menu_item(
                            ui,
                            theme,
                            Some(&|ui, rect, c| gg.image(rect.height(), c).paint_at(ui, rect)),
                            label,
                            *sc,
                            MenuItemVariant::Normal,
                            i == *sel,
                            true,
                        );
                        if r.clicked() {
                            *sel = i;
                        }
                    }
                    menu_separator(ui, theme);
                    menu_item(
                        ui,
                        theme,
                        Some(&|ui, rect, c| {
                            glyph::TRASH.image(rect.height(), c).paint_at(ui, rect)
                        }),
                        "Move to Trash",
                        None,
                        MenuItemVariant::Danger,
                        false,
                        true,
                    );
                });
            });
    });

    meta(
        ui,
        theme,
        &[
            ("height", "28 control-height"),
            ("active", "surface-active"),
            ("danger", "accent-danger label"),
        ],
        &[
            TokenChip::new(
                "surface-active",
                "active row",
                egui::Color32::from(theme.surface_active()),
            ),
            TokenChip::new(
                "accent-danger",
                "danger label",
                egui::Color32::from(theme.accent_danger()),
            ),
            TokenChip::new(
                "text-muted",
                "shortcut",
                egui::Color32::from(theme.text_muted()),
            ),
        ],
    );
}

/// TreeRow — depth · chevron · icon(selected accent) · meta.
pub fn draw_tree_row(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        // 디자인: bg-sidebar padding 6px 4px width 320.
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_sidebar()))
            .inner_margin(egui::Margin {
                left: theme.spacing_xs.value() as i8,
                right: theme.spacing_xs.value() as i8,
                top: theme.spacing_sm.value() as i8,
                bottom: theme.spacing_sm.value() as i8,
            })
            .show(ui, |ui| {
                ui.set_width(theme.measure_md.value());
                ui.spacing_mut().item_spacing.y = 0.0;
                TREE_SEL.with(|s| {
                    let mut sel = s.borrow_mut();
                    // (depth, has_children, open, glyph, label, meta)
                    let rows: [(u16, bool, bool, glyph::MockGlyph, &str, &str); 4] = [
                        (0, true, true, glyph::FOLDER, "tasty", "34"),
                        (1, true, false, glyph::FOLDER, "crates", "39"),
                        (1, false, false, glyph::FILE, "Cargo.toml", ""),
                        (1, false, false, glyph::FILE, "README.md", "4.7k"),
                    ];
                    for (i, (depth, hc, open, g, label, meta)) in rows.iter().enumerate() {
                        let gg = *g;
                        let r = tree_row(
                            ui,
                            theme,
                            *depth,
                            *hc,
                            *open,
                            Some(&|ui, rect, c| gg.image(rect.height(), c).paint_at(ui, rect)),
                            label,
                            (!meta.is_empty()).then_some(*meta),
                            i == *sel,
                            true,
                        );
                        if r.clicked() {
                            *sel = i;
                        }
                    }
                });
            });
    });

    meta(
        ui,
        theme,
        &[
            ("height", "22 control-height-tree"),
            ("indent", "space-md / level"),
            ("selected", "surface-active"),
        ],
        &[
            TokenChip::new(
                "surface-active",
                "selected row",
                egui::Color32::from(theme.surface_active()),
            ),
            TokenChip::new(
                "overlay-hover",
                "hover row",
                egui::Color32::from(theme.overlay_hover()),
            ),
            TokenChip::new(
                "text-muted",
                "meta value",
                egui::Color32::from(theme.text_muted()),
            ),
        ],
    );
}
