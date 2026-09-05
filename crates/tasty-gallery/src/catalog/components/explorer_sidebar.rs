//! `explorer_sidebar` specimen — 디자인 T11 explorer 좌측 사이드바 (design `ExpSidebar`
//! / `SideHead` / `TreeNode` / `FavoritesEmpty`), 즐겨찾기 하단 고정(pin) 레이아웃 포함.
//!
//! - **Files 섹션(상단)**: flex(남는 공간 전부) + 자체 스크롤(`tree_row` 재사용). active
//!   노드 = surface-active + text-primary, 폴더 아이콘 text-muted.
//! - **경계**: 고정 좌표의 1px separator — 트리 길이와 무관, 하단 Favorites 영역의
//!   상단에 항상 고정된다(트리가 짧아도 보더가 콘텐츠를 따라 올라가지 않는다).
//! - **Favorites 섹션(하단)**: 계산된 고정 높이 + 자체 스크롤. 캡션은 항상 표시.
//!   populated = 채운 별(accent-warning) 행, empty = 흐린 별 + "No favorites yet" + 힌트.
//!
//! 고정 높이 계산은 design `favPinHeight` 를 그대로 전사한다: 사이드바 본문 높이가
//! 600px 이상이면 240 고정, 미만이면 (본문×0.4)를 4px 그리드로 스냅한 값과 120 중
//! 큰 값. 본체 구현은 `src/adapters/ui/surface/explorer.rs::favorites_pin_height` —
//! gallery crate 는 본체를 참조할 수 없어 동일 공식을 specimen 전용으로 복제한다.
//!
//! 색·치수·폰트는 전부 `Theme` 토큰. 본체 `explorer.rs` 의 `sidebar`/`tree_node`/
//! `favorite_row`/`favorites_empty` 와 동일 형상.

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
/// 데모 컨테이너의 사이드바 본문 높이 — 600 미만이라 비율(40%) 분기를 재현하고,
/// 긴 트리/많은 즐겨찾기 각각의 스크롤도 자연히 유발한다(§ 아래 4케이스 참고).
const DEMO_BODY_H: LogicalPx = LogicalPx(340.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // ── (a) Files 길어서 스크롤, Favorites 는 하단에 고정 유지 ──
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

    // ── (b) Files 짧아서 상단에 빈 공간만 남는다(패딩/센터링 없음) ──
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

    // ── (c) Favorites empty state ──
    cluster(ui, theme, "favorites — empty state", |ui| {
        stage(ui, theme, StageVariant::Tight, |ui| {
            panel(ui, theme, |ui| {
                two_region(ui, theme, "c", TREE_SHORT, &[]);
            });
        });
    });

    // ── (d) Favorites 자체 스크롤(10개) — Files 스크롤과 완전 독립 ──
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
    // 4 개 variant(a/b/c/d)가 같은 라벨(TREE_SHORT/TREE_LONG/FAVS_*)을 재사용하므로,
    // variant 전체를 고유 id 스코프로 감싸 auto-id 충돌을 막는다.
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
    // `LogicalPx` 에는 `round` 가 없다 — 4px 그리드로 맞추는 이 한 자리에서만 벗긴다.
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
