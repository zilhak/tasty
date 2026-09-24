//! 탐색기의 파일 트리와 하단 즐겨찾기 예제. 두 영역은 따로 스크롤한다.
//! 즐겨찾기 높이 계산은 본체 explorer.rs::favorites_pin_height와 같은 식을 사용한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tree_row;

use crate::catalog::icons::{FOLDER, STAR, STAR_FILL};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

/// (label, depth, active) — 짧은 트리, 스크롤 없이 상단 영역에 빈 공간을 남긴다.
const TREE_SHORT: &[(&str, u16, bool)] = &[
    ("Home", 0, false),
    ("Downloads", 1, true),
    ("Documents", 1, false),
];

/// (label, depth, active) — 긴 트리, 상단 영역 스크롤을 유발한다.
const TREE_LONG: &[(&str, u16, bool)] = &[
    ("Home", 0, false),
    ("Downloads", 1, true),
    ("figma-exports", 2, false),
    ("screenshots", 2, false),
    ("archive", 2, false),
    ("Documents", 1, false),
    ("Projects", 1, false),
    ("tasty", 2, false),
    ("crates", 3, false),
    ("src", 3, false),
    ("docs", 3, false),
    ("Pictures", 1, false),
    ("Music", 1, false),
    ("Videos", 1, false),
    ("Desktop", 1, false),
];

/// (label, active) — 소수 즐겨찾기, 고정 영역 안에 전부 들어간다.
const FAVS_FEW: &[(&str, bool)] = &[
    ("tasty", true),
    ("Documents", false),
    ("screenshots", false),
];

/// (label, active) — 다수 즐겨찾기(10개), 고정 영역을 넘겨 자체 스크롤을 유발한다.
const FAVS_MANY: &[(&str, bool)] = &[
    ("tasty", true),
    ("Documents", false),
    ("screenshots", false),
    ("figma-exports", false),
    ("Downloads", false),
    ("Projects", false),
    ("archive", false),
    ("Pictures", false),
    ("Music", false),
    ("Desktop", false),
];

/// design ExpSidebar width 196.
const SIDEBAR_W: LogicalPx = LogicalPx(196.0);
/// 높이 600 미만의 비율 계산과 두 영역의 개별 스크롤을 보여주는 예제 크기.
const DEMO_BODY_H: LogicalPx = LogicalPx(340.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    cluster(
        ui,
        theme,
        "files — long tree scrolls, favorites stays pinned",
        |ui| {
            stage(ui, theme, StageVariant::Tight, |ui| {
                panel(ui, theme, |ui| {
                    two_region(ui, theme, "a", TREE_LONG, FAVS_FEW);
                });
            });
        },
    );

    cluster(
        ui,
        theme,
        "files — short tree leaves blank space above pin",
        |ui| {
            stage(ui, theme, StageVariant::Tight, |ui| {
                panel(ui, theme, |ui| {
                    two_region(ui, theme, "b", TREE_SHORT, FAVS_FEW);
                });
            });
        },
    );

    cluster(ui, theme, "favorites — empty state", |ui| {
        stage(ui, theme, StageVariant::Tight, |ui| {
            panel(ui, theme, |ui| {
                two_region(ui, theme, "c", TREE_SHORT, &[]);
            });
        });
    });

    cluster(
        ui,
        theme,
        "favorites — own scroll (many favorites)",
        |ui| {
            stage(ui, theme, StageVariant::Tight, |ui| {
                panel(ui, theme, |ui| {
                    two_region(ui, theme, "d", TREE_SHORT, FAVS_MANY);
                });
            });
        },
    );

    meta(
        ui,
        theme,
        &[
            ("width", "196 (design ExpSidebar)"),
            (
                "split",
                "Files flex(top) → fixed border → Favorites pinned(bottom)",
            ),
            (
                "pin height",
                "body>=600 → 240 fixed, else round(body×0.4/4)×4, min 120",
            ),
            (
                "pin transition",
                "hard switch at 600 threshold — no interpolation",
            ),
            (
                "scroll",
                "Files/Favorites independent ScrollArea, id_salt 분리",
            ),
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
            TokenChip::new("separator", "split border", theme.separator.into()),
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
        "Favorites is pinned to a computed fixed height at the sidebar bottom so it stays \
         visible regardless of how long the Files tree grows — the two regions scroll \
         independently. Below the 600px body-height threshold the pin height shrinks to 40% \
         of the body (4px-grid snapped, 120px floor) instead of the 240px default; the switch \
         is a hard cutover, not an interpolation. The split border sits at a fixed coordinate \
         above the Favorites region — a short tree leaves blank background above it rather \
         than pushing it down or centering the tree.",
    );
}

/// 데모 사이드바 컨테이너 — 배경 + 보더 + 고정 폭/높이(`DEMO_BODY_H`).
fn panel(ui: &mut egui::Ui, theme: &Theme, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.bg_sidebar()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.separator),
        ))
        .show(ui, |ui| {
            ui.set_width(SIDEBAR_W.value());
            ui.set_height(DEMO_BODY_H.value());
            ui.spacing_mut().item_spacing.y = 0.0;
            contents(ui);
        });
}

/// 2-region 분할 — 본체 `explorer.rs::sidebar()` 구조 전사: 상단 Files(flex+스크롤),
/// 고정 좌표 구분선, 하단 Favorites(고정 높이+스크롤).
fn two_region(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    tree: &[(&str, u16, bool)],
    favs: &[(&str, bool)],
) {
    // 예제마다 같은 라벨을 쓰므로 위젯 ID의 범위를 나눈다.
    ui.push_id(id_salt, |ui| two_region_inner(ui, theme, tree, favs));
}

fn two_region_inner(
    ui: &mut egui::Ui,
    theme: &Theme,
    tree: &[(&str, u16, bool)],
    favs: &[(&str, bool)],
) {
    let fav_h = favorites_pin_height(DEMO_BODY_H);
    let files_h = (DEMO_BODY_H - fav_h - theme.border_width).max(LogicalPx(0.0));

    ui.allocate_ui_with_layout(
        egui::vec2(SIDEBAR_W.value(), files_h.value()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            caption(ui, theme, "Files");
            egui::ScrollArea::vertical()
                .id_salt("files")
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    for (i, (label, depth, active)) in tree.iter().enumerate() {
                        ui.push_id(i, |ui| {
                            let leaf = *depth >= 2;
                            tree_row(
                                ui,
                                theme,
                                *depth,
                                !leaf,
                                *depth == 1,
                                Some(&|ui, rect, _c| {
                                    FOLDER
                                        .image(
                                            rect.height(),
                                            egui::Color32::from(theme.text_muted()),
                                        )
                                        .paint_at(ui, rect)
                                }),
                                label,
                                None,
                                *active,
                                true,
                            )
                        });
                    }
                });
        },
    );

    section_separator(ui, theme);

    ui.allocate_ui_with_layout(
        egui::vec2(SIDEBAR_W.value(), fav_h.value()),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            caption(ui, theme, "Favorites");
            egui::ScrollArea::vertical()
                .id_salt("favorites")
                .auto_shrink([false, false])
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    if favs.is_empty() {
                        favorites_empty(ui, theme);
                    } else {
                        for (i, (label, active)) in favs.iter().enumerate() {
                            ui.push_id(i, |ui| fav_row(ui, theme, label, *active));
                        }
                    }
                });
        },
    );
}

/// design `favPinHeight` 전사 — 본체 `explorer.rs::favorites_pin_height` 와 동일 공식.
fn favorites_pin_height(body_h: LogicalPx) -> LogicalPx {
    const BASE: LogicalPx = LogicalPx(240.0);
    const THRESHOLD: LogicalPx = LogicalPx(600.0);
    const RATIO: f32 = 0.4;
    const MIN: LogicalPx = LogicalPx(120.0);
    if body_h <= LogicalPx(0.0) || body_h >= THRESHOLD {
        return BASE;
    }
    (LogicalPx((body_h * RATIO / 4.0).value().round()) * 4.0).max(MIN)
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
        // 즐겨찾기 별 아이콘 톤. 대응 토큰 없음 — 본체와 같은 값을 여기 다시 적는다.
        const FAV_STAR_ICON_OPACITY: f32 = 0.55;
        STAR.image(
            sz,
            egui::Color32::from(theme.text_muted()).gamma_multiply(FAV_STAR_ICON_OPACITY),
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
