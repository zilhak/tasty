//! MenuItem · TreeRow primitive specimen — 디자인 gallery `components.html` 대조.
//! (Tab 은 Layouts 의 "Pane Tab Bar" specimen 으로 커버.)

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{menu_item, menu_separator, tree_row, MenuItemVariant};

use super::glyph;

use crate::catalog::specimen::caption;

thread_local! {
    static SEL: RefCell<usize> = const { RefCell::new(0) };
    static TREE_SEL: RefCell<usize> = const { RefCell::new(1) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);

    caption(ui, theme, "MenuItem — icon · label · shortcut · active · disabled · danger · separator");
    ui.vertical(|ui| {
        ui.set_max_width(260.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        SEL.with(|s| {
            let mut sel = s.borrow_mut();
            let items: [(glyph::MockGlyph, &str, &str); 3] = [
                (glyph::TERMINAL, "New terminal", "Alt+T"),
                (glyph::FOLDER, "Open folder…", ""),
                (glyph::SETTINGS, "Settings", "Ctrl+,"),
            ];
            for (i, (g, label, sc)) in items.iter().enumerate() {
                let gg = *g;
                let r = menu_item(
                    ui,
                    theme,
                    Some(&|ui, rect, c| gg.image(rect.height(), c).paint_at(ui, rect)),
                    label,
                    if sc.is_empty() { None } else { Some(sc) },
                    MenuItemVariant::Normal,
                    i == *sel,
                    true,
                );
                if r.clicked() {
                    *sel = i;
                }
            }
            // disabled — opacity 0.45, hover/클릭 비활성 (enabled=false).
            menu_item(
                ui,
                theme,
                Some(&|ui, rect, c| glyph::FILE.image(rect.height(), c).paint_at(ui, rect)),
                "Paste (nothing copied)",
                Some("Ctrl+V"),
                MenuItemVariant::Normal,
                false,
                false,
            );
            menu_separator(ui, theme);
            menu_item(
                ui,
                theme,
                Some(&|ui, rect, c| glyph::TRASH.image(rect.height(), c).paint_at(ui, rect)),
                "Delete profile",
                None,
                MenuItemVariant::Danger,
                false,
                true,
            );
        });
    });

    ui.add_space(12.0);
    caption(ui, theme, "TreeRow — depth · chevron · icon(selected accent) · meta");
    ui.vertical(|ui| {
        ui.set_max_width(260.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        TREE_SEL.with(|s| {
            let mut sel = s.borrow_mut();
            // (depth, has_children, open, glyph, label, meta)
            let rows: [(u16, bool, bool, glyph::MockGlyph, &str, &str); 4] = [
                (0, true, true, glyph::FOLDER, "Project A", "3"),
                (1, false, false, glyph::TERMINAL, "server", "48213"),
                (1, false, false, glyph::FILE, "dev", "48990"),
                (0, true, false, glyph::FOLDER, "Project B", "1"),
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
                    Some(meta),
                    i == *sel,
                    true,
                );
                if r.clicked() {
                    *sel = i;
                }
            }
        });
    });
}
