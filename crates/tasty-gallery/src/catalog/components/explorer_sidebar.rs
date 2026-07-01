//! `explorer_sidebar` specimen — 디자인 T11 explorer 좌측 사이드바 (design `ExpSidebar`
//! / `SideHead` / `TreeNode` / `FavoritesEmpty`).
//!
//! - **Files 섹션(위)**: 디렉토리 트리(`tree_row` 재사용). active 노드 = surface-active +
//!   text-primary, 폴더 아이콘 text-muted.
//! - **구분선**: 트리 ↔ 즐겨찾기 사이 1px separator.
//! - **Favorites 섹션(아래)**: 캡션은 항상 표시. populated = 채운 별(accent-warning) 행,
//!   empty = 흐린 별 + "No favorites yet" + 힌트(text-placeholder).
//!
//! 색·치수·폰트는 전부 `Theme` 토큰. 본체 `explorer.rs` 의 `sidebar`/`tree_node`/
//! `favorite_row`/`favorites_empty` 와 동일 형상.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::tree_row;

use crate::catalog::icons::{FOLDER, STAR, STAR_FILL};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

/// (label, depth, active)
const TREE: &[(&str, u16, bool)] = &[
    ("Home", 0, false),
    ("Downloads", 1, true),
    ("figma-exports", 2, false),
    ("Documents", 1, false),
    ("Projects", 1, false),
];

/// (label, active)
const FAVS: &[(&str, bool)] = &[
    ("tasty", true),
    ("Documents", false),
    ("screenshots", false),
];

/// design ExpSidebar width 196.
const SIDEBAR_W: f32 = 196.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // ── populated (Files 트리 + 구분선 + 즐겨찾기) ──
    stage(ui, theme, StageVariant::Tight, |ui| {
        panel(ui, theme, |ui| {
            files_and_tree(ui, theme);
            section_separator(ui, theme);
            caption(ui, theme, "Favorites");
            for (n, a) in FAVS {
                fav_row(ui, theme, n, *a);
            }
        });
    });

    // ── empty state (즐겨찾기 0개) ──
    cluster(ui, theme, "favorites — empty state", |ui| {
        panel(ui, theme, |ui| {
            files_and_tree(ui, theme);
            section_separator(ui, theme);
            caption(ui, theme, "Favorites");
            favorites_empty(ui, theme);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("width", "196 (design ExpSidebar)"),
            ("order", "Files (tree) → separator → Favorites"),
            ("tree active", "surface-active + text-primary"),
            ("fav star", "starFill · accent-warning"),
            ("empty", "faint star + caption + hint"),
        ],
        &[
            TokenChip::new(
                "surface-active",
                "active row",
                egui::Color32::from(theme.surface_active()),
            ),
            TokenChip::new(
                "accent-warning",
                "filled star",
                egui::Color32::from(theme.accent_warning()),
            ),
            TokenChip::new("separator", "section rule", theme.separator.into()),
            TokenChip::new(
                "text-placeholder",
                "empty hint",
                egui::Color32::from(theme.text_placeholder()),
            ),
        ],
    );

    note(
        ui,
        theme,
        "The Favorites caption stays even at zero favorites so the feature is discoverable — a \
         faint star, \"No favorites yet\", and a right-click hint fill the slot (empty variant). \
         Files (tree) sits above the separator, Favorites below. Mirrors the main app's sidebar.",
    );
}

fn panel(ui: &mut egui::Ui, theme: &Theme, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.bg_sidebar()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.separator),
        ))
        .inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: theme.spacing_xs.value() as i8,
            bottom: theme.spacing_sm.value() as i8,
        })
        .show(ui, |ui| {
            ui.set_width(SIDEBAR_W);
            ui.spacing_mut().item_spacing.y = 0.0;
            contents(ui);
        });
}

fn files_and_tree(ui: &mut egui::Ui, theme: &Theme) {
    caption(ui, theme, "Files");
    for (label, depth, active) in TREE {
        let leaf = *depth >= 2;
        tree_row(
            ui,
            theme,
            *depth,
            !leaf,
            *depth == 1,
            Some(&|ui, rect, _c| {
                FOLDER
                    .image(rect.height(), egui::Color32::from(theme.text_muted()))
                    .paint_at(ui, rect)
            }),
            label,
            None,
            *active,
            true,
        );
    }
}

fn fav_row(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) {
    let star_color = egui::Color32::from(theme.accent_warning());
    tree_row(
        ui,
        theme,
        0,
        false,
        false,
        Some(&|ui, rect, _c| {
            STAR_FILL
                .image(rect.height(), star_color)
                .paint_at(ui, rect)
        }),
        label,
        None,
        active,
        true,
    );
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(theme.spacing_xs.value());
    ui.horizontal(|ui| {
        ui.add_space(theme.spacing_sm.value());
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .font(egui::FontId::monospace(theme.font_size_micro.value()))
                .color(egui::Color32::from(theme.text_muted())),
        );
    });
    ui.add_space(theme.spacing_xs.value());
}

fn section_separator(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(theme.spacing_sm.value());
    let (sep, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        sep.x_range(),
        sep.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.separator),
        ),
    );
}

fn favorites_empty(ui: &mut egui::Ui, theme: &Theme) {
    let inset = theme.spacing_sm.value();
    ui.add_space(theme.spacing_xs.value());
    ui.horizontal(|ui| {
        ui.add_space(inset);
        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
        let sz = theme.icon_glyph_size_sm.value();
        let (r, _) = ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
        STAR.image(
            sz,
            egui::Color32::from(theme.text_muted()).gamma_multiply(0.55),
        )
        .paint_at(ui, r);
        ui.label(
            egui::RichText::new("No favorites yet")
                .size(theme.font_size_caption.value())
                .color(egui::Color32::from(theme.text_muted())),
        );
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_space(inset);
        ui.spacing_mut().item_spacing.x = 0.0;
        let micro = theme.font_size_caption.value();
        ui.label(
            egui::RichText::new("Right-click a folder → ")
                .size(micro)
                .color(egui::Color32::from(theme.text_placeholder())),
        );
        ui.label(
            egui::RichText::new("Add to favorites")
                .size(micro)
                .color(egui::Color32::from(theme.text_muted())),
        );
        ui.label(
            egui::RichText::new(".")
                .size(micro)
                .color(egui::Color32::from(theme.text_placeholder())),
        );
    });
    ui.add_space(inset);
}
