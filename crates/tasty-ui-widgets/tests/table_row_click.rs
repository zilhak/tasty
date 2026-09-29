//! 행 선택 표에서 글자 위 클릭도 행 클릭으로 처리하는지 검사한다. 높이를 지정하지 않은 표의
//! 헤더와 본문 행이 `table_cell_height`인지, 선택·hover 행 배경이 `table_row_bg_selected`·
//! `table_row_bg_hover`인지, `header_pad_right`가 오른쪽 정렬 열 제목을 안쪽으로 미는지도 검사한다.
//! 본문 텍스트 선택과 헤더 정렬 클릭의 구분은 docs/architecture/ui-widgets-crate.md를 따른다.

use std::cell::RefCell;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Table, TableAlign, TableColumn, TableColumnWidth, TableOutput};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Col {
    Name,
    Kind,
}

struct Row {
    name: &'static str,
    kind: &'static str,
}

const ROWS: &[Row] = &[
    Row {
        name: "alpha.txt",
        kind: "Text",
    },
    Row {
        name: "bravo.rs",
        kind: "Rust",
    },
    Row {
        name: "charlie.md",
        kind: "Markdown",
    },
];

/// 프레임 관측치: 셀 라벨 rect(col 0), 표 상단 y, 셀 서브트리의 라벨 선택성.
#[derive(Default)]
struct Probe {
    name_rects: Vec<Rect>,
    table_top: f32,
    cell_selectable_labels: bool,
}

fn raw(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        focused: true,
        events,
        ..Default::default()
    }
}

fn ptr_move(p: Pos2) -> Event {
    Event::PointerMoved(p)
}

fn ptr_btn(p: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    }
}

/// 한 프레임 구동. 표를 그리고 `TableOutput` 과 관측치를 돌려준다.
fn frame(
    ctx: &egui::Context,
    theme: &Theme,
    selectable: bool,
    events: Vec<Event>,
) -> (TableOutput<Col>, Probe) {
    let probe = RefCell::new(Probe::default());
    let mut out: Option<TableOutput<Col>> = None;

    let _out = ctx.run(raw(events), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            probe.borrow_mut().table_top = ui.cursor().top();
            let columns = vec![
                TableColumn {
                    title: "Name",
                    width: TableColumnWidth::Remainder {
                        at_least: LogicalPx(140.0),
                        clip: true,
                    },
                    align: TableAlign::Left,
                    sort_id: Some(Col::Name),
                },
                TableColumn {
                    title: "Kind",
                    width: TableColumnWidth::Initial {
                        initial: LogicalPx(92.0),
                        at_least: LogicalPx(72.0),
                    },
                    align: TableAlign::Left,
                    sort_id: Some(Col::Kind),
                },
            ];
            out = Some(Table::new(columns).selectable(selectable).show(
                ui,
                theme,
                ROWS,
                |_row: &Row| false,
                |ui, _th, row: &Row, col| {
                    let mut p = probe.borrow_mut();
                    p.cell_selectable_labels = ui.style().interaction.selectable_labels;
                    drop(p);
                    let resp = ui.label(if col == 0 { row.name } else { row.kind });
                    if col == 0 {
                        probe.borrow_mut().name_rects.push(resp.rect);
                    }
                },
            ));
        });
    });

    (out.expect("table drawn"), probe.into_inner())
}

/// 헤더 행 중앙 좌표. 헤더 높이는 `Table` 기본값(`table_cell_height`).
fn header_pos(theme: &Theme, probe: &Probe) -> Pos2 {
    let header_h = theme.table_cell_height().value();
    pos2(
        probe.name_rects[0].left() + 5.0,
        probe.table_top + header_h * 0.5,
    )
}

/// 셀 텍스트(파일 이름 글자) 정중앙을 좌클릭하면 그 행이 `clicked_row` 로 나와야 한다.
#[test]
fn selectable_row_click_lands_on_cell_text() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();

    // 레이아웃 프레임 — 셀 라벨 rect 확보.
    let (_, probe) = frame(&ctx, &theme, true, vec![]);
    assert_eq!(probe.name_rects.len(), ROWS.len(), "행마다 이름 라벨 1개");
    let target = probe.name_rects[1].center();
    assert!(
        probe.name_rects[1].width() > 1.0,
        "이름 라벨이 실제 폭을 가져야 클릭 대상이 된다"
    );

    // hover → press → release.
    let (_, _) = frame(&ctx, &theme, true, vec![ptr_move(target)]);
    let (_, _) = frame(&ctx, &theme, true, vec![ptr_btn(target, true)]);
    let (out, _) = frame(&ctx, &theme, true, vec![ptr_btn(target, false)]);

    assert_eq!(
        out.clicked_row,
        Some(1),
        "셀 텍스트 위 클릭이 행 클릭으로 처리돼야 한다"
    );
}

/// `selectable(true)` 셀 서브트리에서는 라벨 텍스트 선택이 꺼져 있어야 한다
/// (I-beam 커서 / 드래그 하이라이트의 출처).
#[test]
fn selectable_disables_cell_label_text_selection() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();

    let (_, on) = frame(&ctx, &theme, true, vec![]);
    assert!(
        !on.cell_selectable_labels,
        "행 선택 모드 셀에서는 selectable_labels 가 꺼져야 한다"
    );

    let (_, off) = frame(&ctx, &theme, false, vec![]);
    assert!(
        off.cell_selectable_labels,
        "비선택 표는 egui 기본값 유지 — 셀 텍스트를 드래그 선택할 수 있어야 한다"
    );
}

/// 헤더 정렬 클릭 회귀 — 헤더 라벨은 명시 `Sense::click()` 이라 셀 정책과 무관하다.
#[test]
fn selectable_keeps_header_sort_click() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();

    let (_, probe) = frame(&ctx, &theme, true, vec![]);
    let target = header_pos(&theme, &probe);

    let (_, _) = frame(&ctx, &theme, true, vec![ptr_move(target)]);
    let (_, _) = frame(&ctx, &theme, true, vec![ptr_btn(target, true)]);
    let (out, _) = frame(&ctx, &theme, true, vec![ptr_btn(target, false)]);

    assert_eq!(
        out.clicked_sort,
        Some(Col::Name),
        "헤더 제목 클릭은 정렬 토글로 그대로 동작해야 한다"
    );
    assert_eq!(out.clicked_row, None, "헤더 클릭은 행 클릭이 아니다");
}

/// 높이를 지정하지 않은 표는 헤더와 본문 행 모두 `table_cell_height`를 쓴다.
#[test]
fn unsized_rows_and_header_use_table_cell_height() {
    let base = tasty_themes::mocha_fallback();
    for zoom in [0.85, 1.0, 1.2] {
        let theme = Theme::with_colors_and_zoom(base.extract_colors(), false, zoom);
        let ctx = egui::Context::default();
        // 첫 프레임은 글꼴 준비와 열 폭 계산에 쓰고 두 번째 프레임을 잰다.
        frame(&ctx, &theme, true, vec![]);
        let (_, probe) = frame(&ctx, &theme, true, vec![]);
        let cell_h = theme.table_cell_height().value();
        let pitch = probe.name_rects[1].center().y - probe.name_rects[0].center().y;
        assert!(
            (pitch - cell_h).abs() <= 0.5,
            "ui_zoom {zoom}: row pitch {pitch} vs table_cell_height {cell_h}"
        );
        // 헤더 아래에 첫 행이 온다. 첫 행 라벨 중심 = 표 상단 + 헤더 + 행 절반.
        let first = probe.name_rects[0].center().y - probe.table_top;
        assert!(
            (first - (cell_h + cell_h * 0.5)).abs() <= 1.0,
            "ui_zoom {zoom}: first row centre {first} vs header {cell_h} + half row"
        );
    }
}

/// 선택 행의 배경은 `table_row_bg_selected` 로 칠하고 행 높이만큼 덮는다.
#[test]
fn selected_row_fill_is_table_row_bg_selected() {
    let theme = tasty_themes::mocha_fallback();
    let fill: egui::Color32 = theme.table_row_bg_selected().into();
    let ctx = egui::Context::default();
    let text_selection_fill = ctx.style().visuals.selection.bg_fill;
    let cell_selection_fills = RefCell::new(Vec::new());
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = ctx.run(raw(vec![]), |c| {
            egui::CentralPanel::default().show(c, |ui| {
                let columns = vec![TableColumn {
                    title: "Name",
                    width: TableColumnWidth::Remainder {
                        at_least: LogicalPx(140.0),
                        clip: true,
                    },
                    align: TableAlign::Left,
                    sort_id: None::<Col>,
                }];
                Table::new(columns).selectable(true).show(
                    ui,
                    &theme,
                    ROWS,
                    |row: &Row| row.name == "bravo.rs",
                    |ui, _th, row: &Row, _col| {
                        cell_selection_fills
                            .borrow_mut()
                            .push(ui.visuals().selection.bg_fill);
                        ui.label(row.name);
                    },
                );
            });
        });
        shapes = out.shapes;
    }
    let bands: Vec<Rect> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
            _ => None,
        })
        .collect();
    assert_eq!(bands.len(), 1, "one selected row band: {bands:?}");
    assert!(
        (bands[0].height() - theme.table_cell_height().value()).abs() <= 0.5,
        "selected band height {}",
        bands[0].height()
    );
    assert!(
        !cell_selection_fills.borrow().is_empty()
            && cell_selection_fills
                .borrow()
                .iter()
                .all(|c| *c == text_selection_fill),
        "cell text selection keeps the egui selection fill"
    );
}

#[test]
fn hovered_row_fill_is_table_row_bg_hover() {
    let theme = tasty_themes::mocha_fallback();
    let fill = theme.table_row_bg_hover().to_egui_premultiplied();
    let cell_h = theme.table_cell_height().value();
    let ctx = egui::Context::default();
    let widget_hover_fill = ctx.style().visuals.widgets.hovered.bg_fill;
    let table_top = RefCell::new(0.0_f32);
    let cell_hover_fills = RefCell::new(Vec::new());
    let mut shapes = Vec::new();
    // 첫 프레임에서 표 위치를 얻고, 이후 세 번째 행(charlie.md) 위에 포인터를 둔다.
    for i in 0..4 {
        let events = if i == 0 {
            vec![]
        } else {
            vec![ptr_move(pos2(40.0, *table_top.borrow() + cell_h * 3.5))]
        };
        let out = ctx.run(raw(events), |c| {
            egui::CentralPanel::default().show(c, |ui| {
                *table_top.borrow_mut() = ui.cursor().top();
                let columns = vec![TableColumn {
                    title: "Name",
                    width: TableColumnWidth::Remainder {
                        at_least: LogicalPx(140.0),
                        clip: true,
                    },
                    align: TableAlign::Left,
                    sort_id: None::<Col>,
                }];
                Table::new(columns).selectable(true).show(
                    ui,
                    &theme,
                    ROWS,
                    |_row: &Row| false,
                    |ui, _th, row: &Row, _col| {
                        cell_hover_fills
                            .borrow_mut()
                            .push(ui.visuals().widgets.hovered.bg_fill);
                        ui.label(row.name);
                    },
                );
            });
        });
        shapes = out.shapes;
    }
    let bands: Vec<Rect> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect),
            _ => None,
        })
        .collect();
    assert_eq!(bands.len(), 1, "one hovered row band: {bands:?}");
    let top = *table_top.borrow();
    assert!(
        (bands[0].top() - (top + cell_h * 3.0)).abs() <= 0.5
            && (bands[0].height() - cell_h).abs() <= 0.5,
        "hovered band {:?} for table top {top}",
        bands[0]
    );
    assert!(
        cell_hover_fills
            .borrow()
            .iter()
            .all(|c| *c == widget_hover_fill),
        "cell widgets keep the egui hover fill"
    );
}

/// 오른쪽 정렬 열 제목 "Kind" 글자 영역의 오른쪽 끝.
fn right_header_text_right(theme: &Theme, pad_right: f32) -> f32 {
    let ctx = egui::Context::default();
    let mut shapes = Vec::new();
    for _ in 0..2 {
        let out = ctx.run(raw(vec![]), |c| {
            egui::CentralPanel::default().show(c, |ui| {
                let columns = vec![
                    TableColumn {
                        title: "Name",
                        width: TableColumnWidth::Remainder {
                            at_least: LogicalPx(140.0),
                            clip: true,
                        },
                        align: TableAlign::Left,
                        sort_id: None::<Col>,
                    },
                    TableColumn {
                        title: "Kind",
                        width: TableColumnWidth::Initial {
                            initial: LogicalPx(92.0),
                            at_least: LogicalPx(72.0),
                        },
                        align: TableAlign::Right,
                        sort_id: None,
                    },
                ];
                Table::new(columns).header_pad_right(pad_right).show(
                    ui,
                    theme,
                    ROWS,
                    |_row: &Row| false,
                    |ui, _th, row: &Row, col| {
                        ui.label(if col == 0 { row.name } else { row.kind });
                    },
                );
            });
        });
        shapes = out.shapes;
    }
    shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::epaint::Shape::Text(t) if t.galley.text() == "Kind" => {
                Some(t.visual_bounding_rect().right())
            }
            _ => None,
        })
        .expect("right-aligned header text drawn")
}

#[test]
fn header_pad_right_moves_right_aligned_titles_in() {
    let theme = tasty_themes::mocha_fallback();
    let pad = theme.spacing_sm.value();
    let flush = right_header_text_right(&theme, 0.0);
    let padded = right_header_text_right(&theme, pad);
    assert!(
        ((flush - padded) - pad).abs() <= 0.5,
        "right header title moves in by {pad}: flush {flush}, padded {padded}"
    );
}
