//! 워크스페이스 카테고리를 켰을 때 사이드바 우클릭 메뉴. 커서 아래 대상에 따라 네 구성이 된다.
//! 시안 `overlays-popups.jsx`의 "Sidebar context menu" Spec(`CatMenu`)을 공용 메뉴 항목으로 옮긴다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, menu_item, menu_separator};

use crate::catalog::icons::{CHEVRON_RIGHT, EDIT, MOVE, MockGlyph, PLUS, TRASH};
use crate::catalog::spec::{StageVariant, TokenChip, meta, note, stage, wrap_item};

/// 시안 메뉴 최소 폭(`minWidth: 176`, 테두리·안쪽 여백 포함)과 안쪽 여백 6.
const MENU_MIN_W: LogicalPx = LogicalPx(176.0);
const MENU_PAD: LogicalPx = LogicalPx(6.0);
/// 캡션과 메뉴 사이 간격 6, 메뉴 사이 간격 22(시안 stage gap).
const CAPTION_GAP: LogicalPx = LogicalPx(6.0);
const MENU_GAP: LogicalPx = LogicalPx(22.0);
/// 하위 메뉴 표시 chevron 크기(시안 `chevronRight size={13}`).
const SUBMENU_CHEVRON: LogicalPx = LogicalPx(13.0);

/// 메뉴 한 줄.
#[derive(Clone, Copy)]
enum Row {
    /// (glyph, label, danger, enabled)
    Item(MockGlyph, &'static str, bool, bool),
    /// 하위 메뉴를 여는 행. 열린 상태라 active 로 그린다.
    Submenu(MockGlyph, &'static str),
    Sep,
}

const BACKGROUND: &[Row] = &[Row::Item(PLUS, "New category", false, true)];
const CATEGORY: &[Row] = &[
    Row::Item(PLUS, "Add workspace", false, true),
    Row::Sep,
    Row::Item(EDIT, "Rename category", false, true),
    Row::Item(TRASH, "Delete category", true, true),
    Row::Sep,
    Row::Item(PLUS, "New category", false, true),
];
const RESERVED: &[Row] = &[
    Row::Item(PLUS, "Add workspace", false, true),
    Row::Sep,
    Row::Item(EDIT, "Rename category", false, false),
    Row::Item(TRASH, "Delete category", true, false),
    Row::Sep,
    Row::Item(PLUS, "New category", false, true),
];
const WORKSPACE: &[Row] = &[
    Row::Submenu(MOVE, "Move to category"),
    Row::Sep,
    Row::Item(PLUS, "New category", false, true),
];

fn menu(ui: &mut egui::Ui, theme: &Theme, rows: &[Row]) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.surface_raised()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_strong()),
        ))
        .corner_radius(theme.corner_radius.value())
        .shadow(theme.shadow_popover().to_egui())
        .inner_margin(egui::Margin::same(MENU_PAD.value() as i8))
        .show(ui, |ui| {
            let inner = MENU_MIN_W.value() - (MENU_PAD.value() + theme.border_width.value()) * 2.0;
            // 항목이 남은 폭을 다 차지하므로 최소 폭으로 고정한다. 시안 메뉴도 이 폭이다.
            ui.set_width(inner);
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in rows {
                match *row {
                    Row::Sep => menu_separator(ui, theme),
                    Row::Item(g, label, danger, enabled) => {
                        let variant = if danger {
                            MenuItemVariant::Danger
                        } else {
                            MenuItemVariant::Normal
                        };
                        menu_item(
                            ui,
                            theme,
                            Some(&|ui, rect, c| g.image(rect.height(), c).paint_at(ui, rect)),
                            label,
                            None,
                            variant,
                            false,
                            enabled,
                        );
                    }
                    Row::Submenu(g, label) => {
                        let resp = menu_item(
                            ui,
                            theme,
                            Some(&|ui, rect, c| g.image(rect.height(), c).paint_at(ui, rect)),
                            label,
                            None,
                            MenuItemVariant::Normal,
                            true,
                            true,
                        );
                        let s = SUBMENU_CHEVRON.value();
                        let r = egui::Rect::from_center_size(
                            egui::pos2(
                                resp.rect.right() - theme.spacing_sm.value() - s * 0.5,
                                resp.rect.center().y,
                            ),
                            egui::vec2(s, s),
                        );
                        CHEVRON_RIGHT
                            .image(s, egui::Color32::from(theme.text_muted()))
                            .paint_at(ui, r);
                    }
                }
            }
        });
}

/// Overlays › Workspace categories — 커서 아래 대상에 따라 바뀌는 사이드바 우클릭 메뉴.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(MENU_GAP.value(), MENU_GAP.value());
        // 시안 무대는 `alignItems: flex-start` 라 높이가 다른 메뉴도 위쪽을 맞춘다.
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
            |ui| {
                for (caption, rows) in [
                    ("target: background (cwd)", BACKGROUND),
                    ("target: category header", CATEGORY),
                    ("target: reserved (normal)", RESERVED),
                    ("target: workspace row", WORKSPACE),
                ] {
                    wrap_item(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = CAPTION_GAP.value();
                        ui.label(
                            egui::RichText::new(caption)
                                .monospace()
                                .size(theme.font_size_micro.value())
                                .color(egui::Color32::from(theme.text_muted())),
                        );
                        menu(ui, theme, rows);
                    });
                }
            },
        );
    });
    meta(
        ui,
        theme,
        &[
            ("trigger", "right-click, no dedicated button"),
            ("width", "176px min"),
            ("background", "New category"),
            ("category", "Add ws · Rename · Delete · New"),
            ("reserved", "Add ws · New (additive-only)"),
            ("workspace", "Move to category › · New"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "menu fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "border-strong",
                "edge",
                egui::Color32::from(theme.border_strong()),
            ),
            TokenChip::new(
                "accent-danger",
                "delete row",
                egui::Color32::from(theme.accent_danger()),
            ),
            TokenChip::without_color("shadow-popover", "lift"),
        ],
    );
    note(
        ui,
        theme,
        "\"Move to category\" opens a submenu of category targets (the drag-and-drop reorder is the primary path; this is the keyboard/menu fallback). New strings: workspace_category.add_workspace · collapse · expand. Existing: new_category · rename_category · delete_category · move_to_category.",
    );
}
