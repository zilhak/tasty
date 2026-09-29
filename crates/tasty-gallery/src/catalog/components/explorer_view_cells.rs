//! 탐색기의 그리드·목록·상세 보기 예제. 목록은 tree_row, 상세는 공용 Table을 사용한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Table, TableAlign, TableColumn, TableColumnWidth, TableSortDir, tree_row};

use crate::catalog::icons::{FILE, FOLDER, IMAGE, MockGlyph};
use crate::catalog::spec::{StageVariant, TokenChip, body_column, cluster, meta, note, stage};

/// 셀 폭.
const CELL_W: LogicalPx = LogicalPx(80.0);

/// 샘플 항목의 종류. 이미지는 accent-info 로 칠한다(본체 explorer 와 같은 규칙).
#[derive(Clone, Copy)]
enum Kind {
    Folder,
    File,
    Image,
}

impl Kind {
    fn glyph(self) -> MockGlyph {
        match self {
            Kind::Folder => FOLDER,
            Kind::File => FILE,
            Kind::Image => IMAGE,
        }
    }

    /// 이미지만 accent-info, 나머지는 `muted`(호출부가 정한 기본 글리프 색)를 쓴다.
    fn tint(self, theme: &Theme, muted: egui::Color32) -> egui::Color32 {
        match self {
            Kind::Image => egui::Color32::from(theme.accent_info()),
            Kind::Folder | Kind::File => muted,
        }
    }
}

/// 시안이 정적으로 보여 주는 셀 상태. 선택은 클릭으로 바뀌므로 여기 두지 않는다.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    None,
    Hover,
    Cut,
}

#[derive(Clone, Copy)]
struct Entry {
    kind: Kind,
    name: &'static str,
    mark: Mark,
}

const fn entry(kind: Kind, name: &'static str, mark: Mark) -> Entry {
    Entry { kind, name, mark }
}

/// 시안 `ExpGridMini`. 선택 초기값은 diagram.png(GRID_SEL).
const GRID: &[Entry] = &[
    entry(Kind::Folder, "mockup-exports", Mark::None),
    // 긴 이름 샘플 — 3줄 wrap + '…' 말줄임 시연.
    entry(Kind::File, "rust-toolchain.toml", Mark::None),
    entry(Kind::Image, "diagram.png", Mark::None),
    entry(Kind::File, "notes.md", Mark::Hover),
    entry(Kind::File, "THIRD_PARTY_LICENSES.md", Mark::None),
    entry(Kind::Folder, "node_modules", Mark::None),
];

/// 시안 `ExpListMini`. 선택 초기값은 diagram.png(LIST_SEL).
const LIST: &[Entry] = &[
    entry(Kind::Folder, "mockup-exports", Mark::None),
    entry(Kind::File, "report.pdf", Mark::None),
    entry(Kind::Image, "diagram.png", Mark::None),
    entry(Kind::File, "notes.md", Mark::Hover),
    entry(Kind::File, "build.sh", Mark::None),
    entry(Kind::File, "archive.zip", Mark::Cut),
];

struct DetailRow {
    kind: Kind,
    name: &'static str,
    size: &'static str,
    modified: &'static str,
    kind_label: &'static str,
}

/// 시안 View modes 의 Detail 열. 선택 초기값은 diagram.png(DETAIL_SEL).
const DETAIL: &[DetailRow] = &[
    DetailRow {
        kind: Kind::Folder,
        name: "mockup-exports",
        size: "—",
        modified: "06-20 14:30",
        kind_label: "Folder",
    },
    DetailRow {
        kind: Kind::File,
        name: "report.pdf",
        size: "2.4 MB",
        modified: "06-24 09:12",
        kind_label: "PDF",
    },
    DetailRow {
        kind: Kind::Image,
        name: "diagram.png",
        size: "488 KB",
        modified: "06-26 18:05",
        kind_label: "PNG",
    },
];

thread_local! {
    static GRID_SEL: RefCell<usize> = const { RefCell::new(2) };
    static LIST_SEL: RefCell<usize> = const { RefCell::new(2) };
    static DETAIL_SEL: RefCell<usize> = const { RefCell::new(2) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
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

    // 잘라내기 대기는 전경만 흐리게 하고 선택·호버 배경은 유지한다.
    cluster(ui, theme, "cut (50% opacity until paste)", |ui| {
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing =
                        egui::vec2(theme.spacing_md.value(), theme.spacing_md.value());
                    grid_cell(ui, theme, &GRID[0], true, true);
                    grid_cell(ui, theme, &GRID[2], false, true);
                });
            });
    });

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
                // cluster 의 가로 배치를 물려받으면 행 scope 가 옆으로 붙는다. 세로 배치 안에 둔다.
                ui.vertical(|ui| {
                    LIST_SEL.with(|s| {
                        let mut sel = s.borrow_mut();
                        for (i, e) in LIST.iter().enumerate() {
                            let clicked = ui
                                .scope(|ui| {
                                    if e.mark == Mark::Cut {
                                        ui.multiply_opacity(theme.cut_pending_opacity());
                                    }
                                    if e.mark == Mark::Hover {
                                        static_row_hover(ui, theme);
                                    }
                                    let kind = e.kind;
                                    tree_row(
                                        ui,
                                        theme,
                                        0,
                                        false,
                                        false,
                                        Some(&|ui, rect, c| {
                                            kind.glyph()
                                                .image(rect.height(), kind.tint(theme, c))
                                                .paint_at(ui, rect)
                                        }),
                                        e.name,
                                        None,
                                        i == *sel,
                                    )
                                    .clicked()
                                })
                                .inner;
                            if clicked {
                                *sel = i;
                            }
                        }
                    });
                });
            });
    });

    cluster(
        ui,
        theme,
        "detail — sortable columns (Table reuse)",
        |ui| {
            let columns = vec![
                TableColumn {
                    title: "Name",
                    width: TableColumnWidth::Remainder {
                        at_least: LogicalPx(140.0),
                        clip: true,
                    },
                    align: TableAlign::Left,
                    sort_id: Some(0_usize),
                },
                // design DetailRow gridTemplateColumns: 1fr 80px 132px 92px.
                TableColumn {
                    title: "Size",
                    width: TableColumnWidth::Initial {
                        initial: LogicalPx(80.0),
                        at_least: LogicalPx(64.0),
                    },
                    align: TableAlign::Right,
                    sort_id: Some(1_usize),
                },
                TableColumn {
                    title: "Modified",
                    width: TableColumnWidth::Initial {
                        initial: LogicalPx(132.0),
                        at_least: LogicalPx(108.0),
                    },
                    align: TableAlign::Left,
                    sort_id: Some(2_usize),
                },
                TableColumn {
                    title: "Type",
                    width: TableColumnWidth::Initial {
                        initial: LogicalPx(92.0),
                        at_least: LogicalPx(72.0),
                    },
                    align: TableAlign::Left,
                    sort_id: Some(3_usize),
                },
            ];

            // 페이지 본문은 가로 폭 제한이 없어 Remainder 열이 남은 폭을 모두 차지한다.
            // 설명과 같은 본문 컬럼 폭의 세로 배치 안에 두어 네 열이 본문 안에 들어오게 한다.
            body_column(ui, |ui| {
                DETAIL_SEL.with(|s| {
                    let mut sel = s.borrow_mut();
                    let selected = *sel;
                    let out = Table::new(columns)
                        .active_sort(0_usize, TableSortDir::Asc)
                        .header_fill(egui::Color32::from(theme.table_header_bg()))
                        .selectable(true)
                        .max_scroll_height(theme.overlay_top_offset.value() * 2.0)
                        .id_salt("explorer_detail_demo")
                        .show(
                            ui,
                            theme,
                            DETAIL,
                            |row: &DetailRow| {
                                DETAIL.iter().position(|r| r.name == row.name) == Some(selected)
                            },
                            |ui, th, row, col| match col {
                                0 => {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                                        let sz = th.icon_glyph_size_md.value();
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(sz, sz),
                                            egui::Sense::hover(),
                                        );
                                        row.kind
                                            .glyph()
                                            .image(
                                                sz,
                                                row.kind
                                                    .tint(th, egui::Color32::from(th.text_muted())),
                                            )
                                            .paint_at(ui, rect);
                                        ui.label(
                                            egui::RichText::new(row.name)
                                                .size(th.font_size_body.value())
                                                .color(egui::Color32::from(th.text_primary())),
                                        );
                                    });
                                }
                                1 => {
                                    ui.add_space(th.spacing_sm.value());
                                    ui.label(
                                        egui::RichText::new(row.size)
                                            .font(egui::FontId::monospace(
                                                th.font_size_caption.value(),
                                            ))
                                            .color(egui::Color32::from(th.text_muted())),
                                    );
                                }
                                2 => {
                                    ui.label(
                                        egui::RichText::new(row.modified)
                                            .font(egui::FontId::monospace(
                                                th.font_size_caption.value(),
                                            ))
                                            .color(egui::Color32::from(th.text_muted())),
                                    );
                                }
                                _ => {
                                    ui.label(
                                        egui::RichText::new(row.kind_label)
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
            });
        },
    );

    meta(
        ui,
        theme,
        &[
            ("grid cell", "glyph 16 + 3-line label (…) · fixed height"),
            ("list row", "22 · tree-row-height (file tree row)"),
            (
                "detail row / header",
                "28 · table-cell-height (shared Table)",
            ),
            (
                "detail cells",
                "Name flex · Size/Date mono 11 · Size padR 8",
            ),
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
        "The grid draws glyphs and labels; image textures are not loaded in this example. List and detail views use the shared tree_row and Table widgets. Cut-pending cells dim the icon and label to 50% while preserving the selection or hover background. No explorer-specific row height: the list reuses the tree row (22), and the detail header and body rows reuse the shared Table height (28).",
    );
}

/// 그리드 셀을 그린다. 클릭하면 true를 반환하며 cut 상태는 전경만 흐리게 한다.
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

    if selected {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            egui::Color32::from(theme.surface_active()),
        );
    } else if resp.hovered() || e.mark == Mark::Hover {
        p.rect_filled(
            rect,
            theme.corner_radius.value(),
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }

    let fg_dim = |c: egui::Color32| {
        if cut {
            c.gamma_multiply(theme.cut_pending_opacity())
        } else {
            c
        }
    };
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(
            rect.center().x,
            rect.top() + theme.spacing_sm.value() + glyph / 2.0,
        ),
        egui::vec2(glyph, glyph),
    );
    e.kind
        .glyph()
        .image(
            glyph,
            fg_dim(e.kind.tint(theme, egui::Color32::from(theme.text_muted()))),
        )
        .paint_at(ui, glyph_rect);

    let label_color = fg_dim(if selected {
        egui::Color32::from(theme.text_primary())
    } else {
        egui::Color32::from(theme.text_secondary())
    });
    let mut job = egui::text::LayoutJob {
        halign: egui::Align::Center,
        wrap: egui::text::TextWrapping {
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

/// 시안이 hover 상태로 보여 주는 목록 행의 배경. 다음 `tree_row` 자리에 먼저 칠한다.
/// 실제 포인터가 올라가 있으면 `tree_row` 가 같은 배경을 칠하므로 겹쳐 칠하지 않는다.
fn static_row_hover(ui: &mut egui::Ui, theme: &Theme) {
    let rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(ui.available_width(), theme.tree_row_height().value()),
    );
    if !ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            theme.tree_row_bg_hover().to_egui_premultiplied(),
        );
    }
}
