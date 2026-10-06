//! 탐색기의 파일 트리와 하단 즐겨찾기 예제. 두 영역은 따로 스크롤한다.
//! 즐겨찾기 높이 계산은 본체 explorer.rs::favorites_pin_height와 같은 식을 사용한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tree_row;

use crate::catalog::icons::{FOLDER, STAR, STAR_FILL};
use crate::catalog::spec::{StageVariant, TokenChip, body_column, cluster, meta, note, stage};

/// 시안 `TreeNode` 의 chevron 자리: 펼침 · 접힘 · 없음(leaf).
#[derive(Clone, Copy)]
pub(super) enum Fold {
    Open,
    Closed,
    Leaf,
}
use Fold::{Closed, Leaf, Open};

/// (label, depth, fold, active)
pub(super) type Node = (&'static str, u16, Fold, bool);

/// 짧은 트리(시안 `TREE_SHORT`), 스크롤 없이 상단 영역에 빈 공간을 남긴다.
pub(super) const TREE_SHORT: &[Node] = &[
    ("Home", 0, Open, false),
    ("Downloads", 1, Open, true),
    ("mockup-exports", 2, Leaf, false),
    ("Documents", 1, Closed, false),
    ("Projects", 1, Closed, false),
];

/// 긴 트리(시안 `TREE_LONG`), 상단 영역 스크롤을 유발한다.
const TREE_LONG: &[Node] = &[
    ("Home", 0, Open, false),
    ("Downloads", 1, Open, true),
    ("mockup-exports", 2, Leaf, false),
    ("invoices", 2, Leaf, false),
    ("Documents", 1, Open, false),
    ("contracts", 2, Leaf, false),
    ("notes", 2, Leaf, false),
    ("Projects", 1, Open, false),
    ("tasty", 2, Open, false),
    ("crates", 3, Leaf, false),
    ("docs", 3, Leaf, false),
    ("src", 3, Leaf, false),
    ("target", 3, Leaf, false),
    ("design-system", 2, Leaf, false),
    ("playground", 2, Leaf, false),
    ("Pictures", 1, Open, false),
    ("screenshots", 2, Leaf, false),
    ("wallpapers", 2, Leaf, false),
    ("Music", 1, Closed, false),
    ("Videos", 1, Closed, false),
    (".config", 1, Closed, false),
    (".cache", 1, Closed, false),
];

/// (label, active) — 소수 즐겨찾기(시안 `FAVS_DEFAULT`), 고정 영역 안에 전부 들어간다.
pub(super) const FAVS_FEW: &[(&str, bool)] = &[
    ("tasty", true),
    ("Documents", false),
    ("screenshots", false),
];

/// (label, active) — 다수 즐겨찾기(시안 `FAVS_MANY`, 10개), 고정 영역을 넘겨 자체 스크롤을 유발한다.
const FAVS_MANY: &[(&str, bool)] = &[
    ("tasty", true),
    ("crates", false),
    ("design-system", false),
    ("Documents", false),
    ("mockup-exports", false),
    ("screenshots", false),
    ("wallpapers", false),
    ("invoices", false),
    (".config", false),
    ("playground", false),
];

/// design ExpSidebar width 196.
pub(super) const SIDEBAR_W: LogicalPx = LogicalPx(196.0);
/// 시안 2-region Spec 네 예제의 body 높이(`ExpSidebar height={620}`). pin 240.
const SPLIT_BODY_H: LogicalPx = LogicalPx(620.0);
/// 시안 Favorites populated/empty Spec의 body 높이(`ExpSidebar height={300}`). pin 120.
const FAVORITES_BODY_H: LogicalPx = LogicalPx(300.0);
/// 시안 pin 비교 줄의 40% 구간 예제 body 높이(`[620, 560, 420, 300]`의 560). pin 224.
const PIN_STRIP_UPPER_H: LogicalPx = LogicalPx(560.0);
/// 시안 pin 비교 줄의 40% 구간 예제 body 높이(`[620, 560, 420, 300]`의 420). pin 168.
const PIN_STRIP_LOWER_H: LogicalPx = LogicalPx(420.0);
/// 시안 2-region Spec 아래 pin 비교 줄의 body 높이 순서. pin 240·224·168·120.
const PIN_STRIP_BODY_H: [LogicalPx; 4] = [
    SPLIT_BODY_H,
    PIN_STRIP_UPPER_H,
    PIN_STRIP_LOWER_H,
    FAVORITES_BODY_H,
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    cluster(
        ui,
        theme,
        "files — long tree scrolls, favorites stays pinned",
        |ui| {
            stage(ui, theme, StageVariant::Tight, |ui| {
                panel(ui, theme, SPLIT_BODY_H, |ui| {
                    two_region(ui, theme, "a", SPLIT_BODY_H, TREE_LONG, FAVS_FEW);
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
                panel(ui, theme, SPLIT_BODY_H, |ui| {
                    two_region(ui, theme, "b", SPLIT_BODY_H, TREE_SHORT, FAVS_FEW);
                });
            });
        },
    );

    cluster(ui, theme, "favorites — empty state", |ui| {
        stage(ui, theme, StageVariant::Tight, |ui| {
            panel(ui, theme, SPLIT_BODY_H, |ui| {
                two_region(ui, theme, "c", SPLIT_BODY_H, TREE_LONG, &[]);
            });
        });
    });

    cluster(
        ui,
        theme,
        "favorites — own scroll (many favorites)",
        |ui| {
            stage(ui, theme, StageVariant::Tight, |ui| {
                panel(ui, theme, SPLIT_BODY_H, |ui| {
                    two_region(ui, theme, "d", SPLIT_BODY_H, TREE_LONG, FAVS_MANY);
                });
            });
        },
    );

    // 시안 비교 줄: 긴 트리와 기본 즐겨찾기로 body 높이마다 pin 높이를 보여 준다.
    // 네 예제가 창 끝을 넘지 않고 줄을 바꾸도록 본문 컬럼 폭 안에 둔다.
    body_column(ui, |ui| {
        cluster(ui, theme, "pin height by body height", |ui| {
            // 시안처럼 높이가 다른 예제를 위쪽에 맞춘다. 크기를 먼저 알려야 폭을 넘는 예제가
            // 다음 줄로 간다.
            let wrap_top = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
            ui.with_layout(wrap_top, |ui| {
                for (i, body_h) in PIN_STRIP_BODY_H.into_iter().enumerate() {
                    let size = egui::vec2(SIDEBAR_W.value(), body_h.value());
                    ui.allocate_ui_with_layout(
                        size,
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                            ui.label(
                                egui::RichText::new(pin_strip_label(body_h))
                                    .font(egui::FontId::monospace(theme.font_size_micro.value()))
                                    .color(egui::Color32::from(theme.text_muted())),
                            );
                            stage(ui, theme, StageVariant::Tight, |ui| {
                                panel(ui, theme, body_h, |ui| {
                                    two_region(
                                        ui,
                                        theme,
                                        &format!("pin{i}"),
                                        body_h,
                                        TREE_LONG,
                                        FAVS_FEW,
                                    );
                                });
                            });
                        },
                    );
                }
            });
        });
    });

    cluster(ui, theme, "with favorites", |ui| {
        stage(ui, theme, StageVariant::Tight, |ui| {
            panel(ui, theme, FAVORITES_BODY_H, |ui| {
                two_region(ui, theme, "e", FAVORITES_BODY_H, TREE_SHORT, FAVS_FEW);
            });
        });
    });

    cluster(ui, theme, "empty — caption persists", |ui| {
        stage(ui, theme, StageVariant::Tight, |ui| {
            panel(ui, theme, FAVORITES_BODY_H, |ui| {
                two_region(ui, theme, "f", FAVORITES_BODY_H, TREE_SHORT, &[]);
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("width", "196 (design ExpSidebar)"),
            (
                "example body",
                "620 → pin 240 · populated/empty 300 → pin 120",
            ),
            ("pin strip", "body 620·560·420·300 → pin 240·224·168·120"),
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
            TokenChip::new(
                "explorer-split-border",
                "fixed boundary line",
                theme.explorer_split_border().to_egui_premultiplied(),
            ),
            TokenChip::new(
                "text-placeholder",
                "empty hint",
                egui::Color32::from(theme.text_placeholder()),
            ),
            TokenChip::without_color("explorer-favorites-pin-min-height", "lower clamp"),
            TokenChip::without_color("explorer-favorites-pin-threshold", "small-surface switch"),
            TokenChip::without_color("explorer-favorites-pin-height", "pinned region height"),
            TokenChip::without_color("explorer-sidebar-width", "196 column"),
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

/// 데모 사이드바 컨테이너 — 배경 + 보더 + 고정 폭/높이(`body_h`).
fn panel(
    ui: &mut egui::Ui,
    theme: &Theme,
    body_h: LogicalPx,
    contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(egui::Color32::from(theme.bg_sidebar()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ))
        .show(ui, |ui| {
            ui.set_width(SIDEBAR_W.value());
            ui.set_height(body_h.value());
            ui.spacing_mut().item_spacing.y = 0.0;
            contents(ui);
        });
}

/// 2-region 분할 — 본체 `explorer.rs::sidebar()` 구조 전사: 상단 Files(flex+스크롤),
/// 고정 좌표 구분선, 하단 Favorites(고정 높이+스크롤).
pub(super) fn two_region(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    body_h: LogicalPx,
    tree: &[Node],
    favs: &[(&str, bool)],
) {
    // 예제마다 같은 라벨을 쓰므로 위젯 ID의 범위를 나눈다.
    ui.push_id(id_salt, |ui| {
        two_region_inner(ui, theme, body_h, tree, favs)
    });
}

fn two_region_inner(
    ui: &mut egui::Ui,
    theme: &Theme,
    body_h: LogicalPx,
    tree: &[Node],
    favs: &[(&str, bool)],
) {
    let fav_h = favorites_pin_height(body_h);
    let files_h = (body_h - fav_h - theme.border_width).max(LogicalPx(0.0));

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
                    for (i, &(label, depth, fold, active)) in tree.iter().enumerate() {
                        ui.push_id(i, |ui| {
                            tree_row(
                                ui,
                                theme,
                                depth,
                                !matches!(fold, Leaf),
                                matches!(fold, Open),
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
                                active,
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

/// design `FAV_PIN` — `favPinHeight`가 쓰는 기본 높이·문턱·비율·하한.
const PIN_BASE: LogicalPx = LogicalPx(240.0);
const PIN_THRESHOLD: LogicalPx = LogicalPx(600.0);
const PIN_RATIO: f32 = 0.4;
const PIN_MIN: LogicalPx = LogicalPx(120.0);

/// 시안 비교 줄 라벨: `body {h} → pin {pin}` 뒤에 계산 구간을 붙인다.
fn pin_strip_label(body_h: LogicalPx) -> String {
    let pin = favorites_pin_height(body_h);
    let kind = if body_h >= PIN_THRESHOLD {
        format!(" ({} flat)", PIN_BASE.value())
    } else if pin <= PIN_MIN {
        " (min clamp)".to_string()
    } else {
        " (40%)".to_string()
    };
    format!("body {} → pin {}{kind}", body_h.value(), pin.value())
}

/// design `favPinHeight` 전사 — 본체 `explorer.rs::favorites_pin_height` 와 동일 공식.
fn favorites_pin_height(body_h: LogicalPx) -> LogicalPx {
    if body_h <= LogicalPx(0.0) || body_h >= PIN_THRESHOLD {
        return PIN_BASE;
    }
    (LogicalPx((body_h * PIN_RATIO / 4.0).value().round()) * 4.0).max(PIN_MIN)
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
            theme.separator.to_egui_premultiplied(),
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
