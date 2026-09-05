//! `explorer_view_cells` specimen — 디자인 T11 explorer view mode 셀 (design §3.2).
//!
//! grid / list / detail 세 뷰의 셀을 나란히 전시한다.
//! - **grid (신규)**: 배경 박스 없이 글리프(16) + 3줄 wrap 라벨(폭 기준, 넘치면 '…'). 고정 3줄 높이,
//!   라벨 top 정렬. 선택 셀 = surface-active 배경만(보더 없음) + 라벨 text-primary, 비선택 라벨 text-secondary.
//! - **list (재사용)**: `tree_row()` depth=0 (단일 컬럼 icon+label).
//! - **detail (재사용 + 정렬 헤더)**: 공용 `Table` 위젯에 Name/Size/Modified/Type 컬럼.
//!   정렬 컬럼 헤더 인디케이터는 Table 이 제공 → explorer 는 컬럼 구성만 신규.
//!
//! 색·치수·폰트는 전부 `Theme` 토큰. i18n 키 후보(본체):
//! `explorer.column.{name,size,modified,type}`.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Table, TableAlign, TableColumn, TableColumnWidth, TableSortDir, tree_row};

use crate::catalog::icons::{FILE, FOLDER, MockGlyph};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

// ── grid 셀 치수 (4px 그리드) ──
/// 셀 폭.
const CELL_W: LogicalPx = LogicalPx(80.0);

#[derive(Clone, Copy)]
struct Entry {
    glyph: MockGlyph,
    name: &'static str,
    /// 실 explorer Entry 와의 필드 parity 용 — 렌더는 glyph 가 폴더 여부를 이미
    /// 인코드하므로 specimen 에선 읽지 않는다.
    #[allow(dead_code)]
    dir: bool,
}

const GRID: &[Entry] = &[
    Entry {
        glyph: FOLDER,
        name: "src",
        dir: true,
    },
    Entry {
        glyph: FOLDER,
        name: "assets",
        dir: true,
    },
    Entry {
        glyph: FILE,
        name: "photo.png",
        dir: false,
    },
    Entry {
        glyph: FILE,
        name: "README.md",
        dir: false,
    },
    // 긴 이름 샘플 — 3줄 wrap + '…' 말줄임 시연 (design ExpGridMini).
    Entry {
        glyph: FILE,
        name: "rust-toolchain.toml",
        dir: false,
    },
    Entry {
        glyph: FILE,
        name: "THIRD_PARTY_LICENSES.md",
        dir: false,
    },
];

struct DetailRow {
    glyph: MockGlyph,
    name: &'static str,
    size: &'static str,
    modified: &'static str,
    kind: &'static str,
}

thread_local! {
    static GRID_SEL: RefCell<usize> = const { RefCell::new(2) };
    static LIST_SEL: RefCell<usize> = const { RefCell::new(0) };
    static DETAIL_SEL: RefCell<usize> = const { RefCell::new(3) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // ── grid (신규 셀) ──
    stage(ui, theme, StageVariant::Tight, |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(theme.spacing_md.value(), theme.spacing_md.value());
                    GRID_SEL.with(|s| {
                        let mut sel = s.borrow_mut();
                        for (i, e) in GRID.iter().enumerate() {
                            if grid_cell(ui, theme, e, i == *sel, false) {
                                *sel = i;
                            }
                        }
                    });
                });
            });
    });

    // ── cut state (잘라내기 대기 = 전경 50% opacity) ──
    // design cell-state matrix "cut (50% opacity) until paste". 전경(아이콘+라벨)만
    // opacity-cut(=opacity-disabled, 0.5) 로 디밍하고 선택/hover 배경은 유지한다.
    cluster(ui, theme, "cut (50% opacity until paste)", |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(theme.spacing_md.value(), theme.spacing_md.value());
                    // 첫 셀은 cut+selected (선택 배경 유지 + 전경 디밍), 둘째는 cut only.
                    grid_cell(ui, theme, &GRID[0], true, true);
                    grid_cell(ui, theme, &GRID[2], false, true);
                });
            });
    });

    // ── list (tree_row 재사용, depth 0) ──
    cluster(ui, theme, "list — single column (tree_row reuse)", |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                egui::Color32::from(theme.border_default()),
            ))
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(theme.spacing_xs.value() as i8))
            .show(ui, |ui| {
                ui.set_width(theme.measure_sm.value());
                ui.spacing_mut().item_spacing.y = 0.0;
                LIST_SEL.with(|s| {
                    let mut sel = s.borrow_mut();
                    for (i, e) in GRID.iter().enumerate() {
                        let g = e.glyph;
                        let r = tree_row(
                            ui,
                            theme,
                            0,
                            false,
                            false,
                            Some(&|ui, rect, c| g.image(rect.height(), c).paint_at(ui, rect)),
                            e.name,
                            None,
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

    // ── detail (Table 재사용 + 정렬 컬럼 헤더) ──
    let rows = [
        DetailRow {
            glyph: FOLDER,
            name: "src",
            size: "—",
            modified: "2026-06-20",
            kind: "Folder",
        },
        DetailRow {
            glyph: FOLDER,
            name: "assets",
            size: "—",
            modified: "2026-06-18",
            kind: "Folder",
        },
        DetailRow {
            glyph: FILE,
            name: "README.md",
            size: "4.7 KB",
            modified: "2026-06-27",
            kind: "Markdown",
        },
        DetailRow {
            glyph: FILE,
            name: "photo.png",
            size: "1.2 MB",
            modified: "2026-06-28",
            kind: "PNG image",
        },
    ];

    cluster(
        ui,
        theme,
        "detail — sortable columns (Table reuse)",
        |ui| {
            let columns = vec![
                TableColumn {
                    title: "Name",
                    width: TableColumnWidth::Remainder {
                        at_least: 140.0,
                        clip: true,
                    },
                    align: TableAlign::Left,
                    sort_id: Some(0_usize),
                },
                // design DetailRow gridTemplateColumns: 1fr 80px 132px 92px.
                TableColumn {
                    title: "Size",
                    width: TableColumnWidth::Initial {
                        initial: 80.0,
                        at_least: 64.0,
                    },
                    align: TableAlign::Right,
                    sort_id: Some(1_usize),
                },
                TableColumn {
                    title: "Modified",
                    width: TableColumnWidth::Initial {
                        initial: 132.0,
                        at_least: 108.0,
                    },
                    align: TableAlign::Left,
                    sort_id: Some(2_usize),
                },
                TableColumn {
                    title: "Type",
                    width: TableColumnWidth::Initial {
                        initial: 92.0,
                        at_least: 72.0,
                    },
                    align: TableAlign::Left,
                    sort_id: Some(3_usize),
                },
            ];

            DETAIL_SEL.with(|s| {
                let mut sel = s.borrow_mut();
                let selected = *sel;
                let out = Table::new(columns)
                    .active_sort(0_usize, TableSortDir::Asc)
                    .header_fill(egui::Color32::from(theme.bg_sidebar()))
                    .selectable(true)
                    .max_scroll_height(theme.overlay_top_offset.value() * 2.0)
                    .id_salt("explorer_detail_demo")
                    .show(
                        ui,
                        theme,
                        &rows,
                        |row: &DetailRow| {
                            rows.iter().position(|r| r.name == row.name) == Some(selected)
                        },
                        |ui, th, row, col| match col {
                            0 => {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                                    let g = row.glyph;
                                    let sz = th.icon_glyph_size_md.value();
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(sz, sz),
                                        egui::Sense::hover(),
                                    );
                                    // design glyph 색: 폴더/파일 text-muted.
                                    g.image(sz, egui::Color32::from(th.text_muted()))
                                        .paint_at(ui, rect);
                                    ui.label(
                                        egui::RichText::new(row.name)
                                            .size(th.font_size_body.value())
                                            .color(egui::Color32::from(th.text_primary())),
                                    );
                                });
                            }
                            // Size — mono·11 우측 정렬 + 8px 우측 패딩(design paddingRight 8).
                            1 => {
                                ui.add_space(th.spacing_sm.value());
                                ui.label(
                                    egui::RichText::new(row.size)
                                        .font(egui::FontId::monospace(th.font_size_caption.value()))
                                        .color(egui::Color32::from(th.text_muted())),
                                );
                            }
                            // Date — mono·11.
                            2 => {
                                ui.label(
                                    egui::RichText::new(row.modified)
                                        .font(egui::FontId::monospace(th.font_size_caption.value()))
                                        .color(egui::Color32::from(th.text_muted())),
                                );
                            }
                            // Type — caption(11).
                            _ => {
                                ui.label(
                                    egui::RichText::new(row.kind)
                                        .size(th.font_size_caption.value())
                                        .color(egui::Color32::from(th.text_muted())),
                                );
                            }
                        },
                    );
                if let Some(i) = out.clicked_row {
                    *sel = i;
                }
            });
        },
    );

    meta(
        ui,
        theme,
        &[
            ("grid cell", "glyph 16 + 3-line label (…) · fixed height"),
            ("list row", "22 control-height-tree (tree_row)"),
            ("detail row", "Name flex · Size/Date mono 11 · Size padR 8"),
            ("selected", "surface-active (no border)"),
            ("glyph", "folder/file text-muted · image accent-info"),
            ("cut", "foreground 50% opacity until paste"),
            ("sort", "header indicator (accent-primary)"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "icon box",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "surface-active",
                "selected",
                egui::Color32::from(theme.surface_active()),
            ),
            TokenChip::new(
                "accent-primary",
                "sel border / sort",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::new(
                "text-muted",
                "detail meta",
                egui::Color32::from(theme.text_muted()),
            ),
        ],
    );

    note(
        ui,
        theme,
        "Only the grid cell is new — list rows reuse tree_row (depth 0), and detail reuses \
         the shared Table whose header already paints the sort indicator. Image thumbnails \
         fall back to the file glyph until real textures land in the body stage. Cut-pending \
         cells dim only the foreground (icon + label) to opacity-cut (0.5), keeping the \
         selection/hover background intact so a cut+selected cell still reads as selected.",
    );
}

/// grid 셀 한 개. 클릭되면 `true`. 배경 박스 없이 글리프(16) + 3줄 wrap 라벨(top 정렬),
/// 선택 시 surface-active 배경만(추가 보더 없음 — design GridCell). `cut` 이면 전경
/// (아이콘+라벨)을 opacity-cut(50%) 로 디밍(배경은 유지) — design cell-state matrix.
fn grid_cell(ui: &mut egui::Ui, theme: &Theme, e: &Entry, selected: bool, cut: bool) -> bool {
    let glyph = theme.icon_glyph_size_md.value(); // design glyph 16
    // 라벨: caption(11), line_h ≈ round(11 × 1.3)=14. 고정 3줄 예약 → 그리드 행 정렬 균일.
    let label_font = theme.font_size_caption.value();
    let label_line_h = (label_font * 1.3).round();
    let label_h = label_line_h * 3.0;
    let cell_h = theme.spacing_sm.value()
        + glyph
        + theme.spacing_xs.value()
        + label_h
        + theme.spacing_sm.value();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(CELL_W.value(), cell_h), egui::Sense::click());
    let p = ui.painter_at(rect);

    // 선택 = surface-active 배경만(추가 accent 보더 없음). hover = overlay-hover.
    if selected {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            egui::Color32::from(theme.surface_active()),
        );
    } else if resp.hovered() {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }

    // cut-pending 셀은 전경만 opacity-cut(50%) 로 디밍.
    let fg_dim = |c: egui::Color32| {
        if cut {
            c.gamma_multiply(theme.opacity_cut())
        } else {
            c
        }
    };
    // 아이콘: 박스 없이 상단 중앙 확대 글리프, 폴더/파일 text-muted (design glyphColor).
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x,
            rect.top() + theme.spacing_sm.value() + glyph / 2.0,
        ),
        egui::vec2(glyph, glyph),
    );
    e.glyph
        .image(glyph, fg_dim(egui::Color32::from(theme.text_muted())))
        .paint_at(ui, glyph_rect);

    // 라벨: 폭 기준 wrap, 최대 3줄, 넘치면 마지막 줄 '…'. 블록 top 정렬.
    // 선택 시 text-primary, 비선택 시 text-secondary (design GridCell). cut 디밍은 유지.
    let label_color = fg_dim(if selected {
        egui::Color32::from(theme.text_primary())
    } else {
        egui::Color32::from(theme.text_secondary())
    });
    let mut job = egui::text::LayoutJob {
        halign: egui::Align::Center,
        wrap: egui::text::TextWrapping {
            // 좌우 패딩 spacing_xs(4) 씩 제외한 내부 폭 (design padding "8px 4px").
            max_width: (CELL_W - theme.spacing_xs.scaled(2.0)).value(),
            max_rows: 3,
            overflow_character: Some('…'),
            ..Default::default()
        },
        ..Default::default()
    };
    job.append(
        e.name,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(label_font),
            color: label_color,
            line_height: Some(label_line_h),
            ..Default::default()
        },
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    p.galley(
        egui::pos2(
            rect.center().x,
            glyph_rect.bottom() + theme.spacing_xs.value(),
        ),
        galley,
        label_color,
    );

    resp.clicked()
}
