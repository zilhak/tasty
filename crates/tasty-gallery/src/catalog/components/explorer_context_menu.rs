//! 공용 메뉴 항목을 조합한 탐색기 우클릭 메뉴 예제.
//! 빈 영역·파일·폴더·다중 선택에 따른 네 가지 구성을 보여준다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MenuItemVariant, menu_item, menu_separator};

use crate::catalog::icons::{COPY, EDIT, FILE_PLUS, FOLDER_PLUS, MockGlyph, STAR, TRASH};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};
use crate::i18n::{t, t_count, t_fmt};

/// 메뉴 한 줄.
pub(super) enum Mi {
    /// (leading glyph, label, danger 여부)
    Item(Option<MockGlyph>, &'static str, bool),
    /// 문구를 조합한 행. `tooltip` 이 있으면 꺼진 행이고 이유를 툴팁으로 보인다.
    Row(String, Option<String>),
    Sep,
}

fn empty_menu() -> Vec<Mi> {
    vec![
        Mi::Item(Some(FOLDER_PLUS), t("explorer.command.new_folder"), false),
        Mi::Item(Some(FILE_PLUS), t("explorer.command.new_file"), false),
        Mi::Sep,
        Mi::Item(Some(COPY), "Copy path", false),
        Mi::Item(Some(STAR), "Add to favorites", false),
        Mi::Sep,
        Mi::Item(None, "Paste", false),
    ]
}

/// 빈 영역 메뉴의 Undo · Redo 묶음. 맨 위 단계만 보이고, 되돌릴 수 없게 된 단계는 꺼진 행이다.
fn history_menu() -> Vec<Mi> {
    let undo = |op: &str| t_fmt("explorer.menu.undo", op);
    vec![
        Mi::Item(Some(COPY), "Copy path", false),
        Mi::Sep,
        Mi::Row(undo(&t_count("explorer.menu.op_move", 3, &["3"])), None),
        Mi::Row(
            t_fmt(
                "explorer.menu.redo",
                &t_count("explorer.menu.op_copy", 2, &["2"]),
            ),
            None,
        ),
        Mi::Sep,
        Mi::Row(
            undo(&t_count("explorer.menu.op_copy", 2, &["2"])),
            Some(t_fmt("explorer.menu.undo_stale", t("explorer.result.gone"))),
        ),
        Mi::Sep,
        Mi::Item(None, t("explorer.context_menu.properties"), false),
    ]
}

fn file_menu() -> Vec<Mi> {
    vec![
        Mi::Item(Some(COPY), "Copy path", false),
        Mi::Item(Some(COPY), "Copy", false),
        Mi::Item(None, "Cut", false),
        Mi::Sep,
        Mi::Item(Some(TRASH), "Delete", true),
        Mi::Item(Some(EDIT), "Rename", false),
    ]
}

fn folder_menu() -> Vec<Mi> {
    vec![
        Mi::Item(Some(COPY), "Copy path", false),
        Mi::Item(Some(STAR), "Add to favorites", false),
        Mi::Item(Some(COPY), "Copy", false),
        Mi::Item(None, "Cut", false),
        Mi::Item(None, "Paste (into)", false),
        Mi::Sep,
        Mi::Item(Some(FOLDER_PLUS), t("explorer.command.new_folder"), false),
        Mi::Item(Some(FILE_PLUS), t("explorer.command.new_file"), false),
        Mi::Item(Some(TRASH), "Delete", true),
        Mi::Item(Some(EDIT), "Rename", false),
        Mi::Item(None, "Open in system", false),
    ]
}

fn multi_menu() -> Vec<Mi> {
    vec![
        Mi::Item(Some(COPY), "Copy path (newline-sep)", false),
        Mi::Item(Some(COPY), "Copy", false),
        Mi::Item(None, "Cut", false),
        Mi::Sep,
        Mi::Item(Some(TRASH), "Delete", true),
    ]
}

/// (caption, 항목 빌더) — 한 컨텍스트 메뉴 variant.
type Variant = (&'static str, fn() -> Vec<Mi>);

/// 시안 메뉴 한 장의 바깥 폭(`width: 220`, border-box).
const MENU_OUTER_W: LogicalPx = LogicalPx(220.0);
/// 시안 무대의 메뉴 사이 간격(`gap: 18`). 대응 토큰이 없는 전시용 값이다.
const MENU_GAP: LogicalPx = LogicalPx(18.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // 시안 메뉴 폭 220 은 테두리·안쪽 여백을 포함한 바깥 폭이다.
    let menu_w =
        MENU_OUTER_W.value() - theme.spacing_xs.value() * 2.0 - theme.border_width.value() * 2.0;
    let variants: [Variant; 4] = [
        ("empty area → cwd", empty_menu),
        ("file (single)", file_menu),
        ("folder (single)", folder_menu),
        ("multi-select", multi_menu),
    ];

    stage(ui, theme, StageVariant::Wrap, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(MENU_GAP.value(), theme.spacing_lg.value());
        for (caption, build) in variants {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                ui.label(
                    egui::RichText::new(caption.to_uppercase())
                        .size(theme.font_size_micro.value())
                        .color(egui::Color32::from(theme.text_muted())),
                );
                render_menu(ui, theme, menu_w, &build());
            });
        }
    });
    cluster(ui, theme, "empty area — undo / redo", |ui| {
        render_menu(ui, theme, menu_w, &history_menu());
    });

    meta(
        ui,
        theme,
        &[
            ("container", "surface-raised · 1px border-strong"),
            ("item", "menu_item (28 control-height)"),
            ("separator", "menu_separator (1px)"),
            ("danger", "Delete = accent-danger label"),
            ("targets", "empty · file · folder · multi"),
            (
                "stale step",
                "disabled row · tooltip “Can't undo: {reason}” (none on Windows)",
            ),
            (
                "undo / redo",
                "empty area · before Properties · latest step of 10, no list",
            ),
            (
                "create",
                "empty area: New folder · New file first · folder: in the file-ops group · file / multi: none · remote: hidden",
            ),
        ],
        &[
            TokenChip::new("menu-bg", "menu fill", egui::Color32::from(theme.menu_bg())),
            TokenChip::new(
                "border-strong",
                "menu edge",
                egui::Color32::from(theme.border_strong()),
            ),
            TokenChip::new(
                "accent-danger",
                "delete label",
                egui::Color32::from(theme.accent_danger()),
            ),
            TokenChip::without_color("shadow-popover", "float"),
            TokenChip::without_color("menu-border", "1px edge"),
        ],
    );

    note(
        ui,
        theme,
        "Target rule (body): a right-clicked item inside the selection acts on the whole \
         selection; outside it, the selection resets to that item; on the background, the \
         cwd. New folder and New file create in the cwd from the background and inside the folder \
         from a folder row; the target is fixed when the command starts. Add-to-favorites shows only on a single folder or the background; Open in \
         system only on a folder.",
    );
}

pub(super) fn render_menu(ui: &mut egui::Ui, theme: &Theme, width: f32, items: &[Mi]) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.surface_raised()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_strong()),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_xs.value() as i8))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            for it in items {
                match it {
                    Mi::Sep => menu_separator(ui, theme),
                    Mi::Row(label, tooltip) => {
                        let row = menu_item(
                            ui,
                            theme,
                            None,
                            label,
                            None,
                            MenuItemVariant::Normal,
                            false,
                            tooltip.is_none(),
                        );
                        if let Some(why) = tooltip {
                            row.on_disabled_hover_text(why);
                        }
                    }
                    Mi::Item(glyph, label, danger) => {
                        let variant = if *danger {
                            MenuItemVariant::Danger
                        } else {
                            MenuItemVariant::Normal
                        };
                        match glyph {
                            Some(g) => {
                                let g = *g;
                                menu_item(
                                    ui,
                                    theme,
                                    Some(&|ui, rect, c| {
                                        g.image(rect.height(), c).paint_at(ui, rect)
                                    }),
                                    label,
                                    None,
                                    variant,
                                    false,
                                    true,
                                );
                            }
                            None => {
                                menu_item(ui, theme, None, label, None, variant, false, true);
                            }
                        }
                    }
                }
            }
        });
}
