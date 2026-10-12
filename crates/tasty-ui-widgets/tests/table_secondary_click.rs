//! 행 선택 표의 우클릭이 `secondary_clicked_row` 로만 나오고 `clicked_row` 와 섞이지 않는지,
//! 글자 위·헤더·스크롤한 뒤의 우클릭이 어느 행을 가리키는지 검사한다. 보이는 행만 그리는 표와
//! 모든 행을 그리는 표(`virtual_rows` 를 켜지 않은 소비처의 기존 경로)를 함께 본다.
//! 소비처 구성은 탐색기 상세 보기(바깥 세로 ScrollArea + `virtual_rows`), 포트 표(`ports_table`:
//! 가로 스크롤 + 본문 높이 제한), 갤러리 표(본문 높이 제한)를 따른다.

use std::cell::RefCell;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Table, TableAlign, TableColumn, TableColumnWidth, TableOutput, ports_table,
};

const VIEW_H: f32 = 400.0;

/// 표를 놓는 방식. 각 값은 실제 소비처 하나의 구성을 따른다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// 탐색기 상세 보기: 바깥 세로 ScrollArea 안에서 보이는 행만 그린다.
    Virtual,
    /// 같은 바깥 ScrollArea 에서 모든 행을 그린다.
    Plain,
    /// 갤러리 표: 본문 높이를 제한해 표 안에서 세로 스크롤한다.
    InnerScroll,
    /// 포트 표: `ports_table`(가로 스크롤·헤더 배경) + 본문 높이 제한.
    Ports,
}

const NON_VIRTUAL: [Layout; 3] = [Layout::Plain, Layout::InnerScroll, Layout::Ports];
const ALL: [Layout; 4] = [
    Layout::Virtual,
    Layout::Plain,
    Layout::InnerScroll,
    Layout::Ports,
];

/// 스크롤 애니메이션이 진행되도록 프레임마다 시각을 1/60 초씩 올린다.
fn raw(ctx: &egui::Context, events: Vec<Event>) -> RawInput {
    RawInput {
        time: Some(ctx.input(|i| i.time) + 1.0 / 60.0),
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, VIEW_H))),
        focused: true,
        events,
        ..Default::default()
    }
}

fn button(p: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos: p,
        button,
        pressed,
        modifiers: Modifiers::default(),
    }
}

/// 한 프레임의 관측치: col 0 을 그린 행 번호와 라벨 rect, 표 상단 y.
struct Drawn {
    out: TableOutput<()>,
    rows: Vec<(usize, Rect)>,
    table_top: f32,
}

/// 한 프레임을 그린다. 행은 `0..n` 이고 `selected` 행을 선택 상태로 둔다.
fn frame(
    ctx: &egui::Context,
    theme: &Theme,
    layout: Layout,
    n: usize,
    scroll_to: Option<usize>,
    events: Vec<Event>,
) -> Drawn {
    let rows: Vec<usize> = (0..n).collect();
    let drawn = RefCell::new(Vec::new());
    let mut table_top = 0.0;
    let mut out = None;
    let mut cell = |ui: &mut egui::Ui, _th: &Theme, row: &usize, col: usize| {
        let resp = ui.label(format!("row {row}"));
        if col == 0 {
            drawn.borrow_mut().push((*row, resp.rect));
        }
    };
    // 포트 표의 가로 스크롤은 Exact·Flex 열만 쓴다.
    let columns = || {
        let first = match layout {
            Layout::Ports => TableColumnWidth::Flex {
                min_width: LogicalPx(140.0),
            },
            _ => TableColumnWidth::Remainder {
                at_least: LogicalPx(140.0),
                clip: true,
            },
        };
        vec![
            TableColumn {
                title: "Name",
                width: first,
                align: TableAlign::Left,
                sort_id: Some(()),
            },
            TableColumn {
                title: "Kind",
                width: TableColumnWidth::Exact(LogicalPx(92.0)),
                align: TableAlign::Right,
                sort_id: None,
            },
        ]
    };
    let max_h = LogicalPx(VIEW_H / 2.0);
    let _output = ctx.run(raw(ctx, events), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            table_top = ui.cursor().top();
            match layout {
                Layout::Virtual | Layout::Plain => {
                    let shown = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            Table::new(columns())
                                .selectable(true)
                                .virtual_rows(layout == Layout::Virtual)
                                .scroll_to_row(scroll_to)
                                .show(ui, theme, &rows, |r| *r == 1, &mut cell)
                        });
                    out = Some(shown.inner);
                }
                Layout::InnerScroll => {
                    out = Some(
                        Table::new(columns())
                            .selectable(true)
                            .max_scroll_height(max_h)
                            .scroll_to_row(scroll_to)
                            .show(ui, theme, &rows, |r| *r == 1, &mut cell),
                    );
                }
                Layout::Ports => {
                    out = Some(
                        ports_table(columns(), theme)
                            .max_scroll_height(max_h)
                            .scroll_to_row(scroll_to)
                            .show(ui, theme, &rows, |r| *r == 1, &mut cell),
                    );
                }
            }
        });
    });
    Drawn {
        out: out.expect("table drawn"),
        rows: drawn.into_inner(),
        table_top,
    }
}

/// 이 프레임에 보이는 영역 안에 중심이 있는 `row` 라벨의 rect.
fn on_screen(drawn: &Drawn, row: usize) -> Option<Rect> {
    drawn
        .rows
        .iter()
        .find(|(i, r)| *i == row && r.center().y > drawn.table_top && r.center().y < VIEW_H)
        .map(|(_, r)| *r)
}

/// `at` 에서 `btn` 을 누르고 뗀다. 이동·누름·뗌을 프레임마다 나눠 보낸다.
fn press_release(
    ctx: &egui::Context,
    theme: &Theme,
    layout: Layout,
    n: usize,
    at: Pos2,
    btn: PointerButton,
) -> TableOutput<()> {
    frame(ctx, theme, layout, n, None, vec![Event::PointerMoved(at)]);
    frame(ctx, theme, layout, n, None, vec![button(at, btn, true)]);
    frame(ctx, theme, layout, n, None, vec![button(at, btn, false)]).out
}

/// 행 글자 정중앙을 우클릭하면 그 행이 `secondary_clicked_row` 로만 나온다.
/// 행 선택 표는 본문 라벨의 텍스트 선택을 꺼 두므로 글자 위 우클릭도 행에 간다.
#[test]
fn secondary_click_on_cell_text_reports_only_that_row() {
    let theme = tasty_themes::mocha_fallback();
    for layout in ALL {
        let ctx = egui::Context::default();
        frame(&ctx, &theme, layout, 3, None, vec![]);
        let drawn = frame(&ctx, &theme, layout, 3, None, vec![]);
        for row in [0, 1, 2] {
            let at = on_screen(&drawn, row)
                .unwrap_or_else(|| panic!("{layout:?}: row {row} not on screen"))
                .center();
            let out = press_release(&ctx, &theme, layout, 3, at, PointerButton::Secondary);
            assert_eq!(
                out.secondary_clicked_row,
                Some(row),
                "{layout:?}: right-click on the text of row {row}"
            );
            assert_eq!(
                out.clicked_row, None,
                "{layout:?}: a right-click is not a row click"
            );
            assert_eq!(out.clicked_sort, None, "{layout:?}");
        }
    }
}

/// 좌클릭은 `clicked_row` 로만 나오고 `secondary_clicked_row` 를 남기지 않는다.
#[test]
fn primary_click_does_not_report_a_secondary_row() {
    let theme = tasty_themes::mocha_fallback();
    for layout in ALL {
        let ctx = egui::Context::default();
        frame(&ctx, &theme, layout, 3, None, vec![]);
        let drawn = frame(&ctx, &theme, layout, 3, None, vec![]);
        let at = on_screen(&drawn, 2).expect("row 2").center();
        let out = press_release(&ctx, &theme, layout, 3, at, PointerButton::Primary);
        assert_eq!(out.clicked_row, Some(2), "{layout:?}");
        assert_eq!(out.secondary_clicked_row, None, "{layout:?}");
    }
}

/// 헤더 우클릭은 행 우클릭도, 정렬 클릭도 아니다.
#[test]
fn secondary_click_on_the_header_is_not_a_row() {
    let theme = tasty_themes::mocha_fallback();
    let header_h = theme.table_cell_height().value();
    for layout in ALL {
        let ctx = egui::Context::default();
        frame(&ctx, &theme, layout, 3, None, vec![]);
        let drawn = frame(&ctx, &theme, layout, 3, None, vec![]);
        let row0 = on_screen(&drawn, 0).expect("row 0");
        let at = pos2(row0.left() + 5.0, drawn.table_top + header_h * 0.5);
        let out = press_release(&ctx, &theme, layout, 3, at, PointerButton::Secondary);
        assert_eq!(out.secondary_clicked_row, None, "{layout:?}");
        assert_eq!(out.clicked_row, None, "{layout:?}");
        assert_eq!(out.clicked_sort, None, "{layout:?}");
    }
}

/// 화면 밖 행으로 스크롤한 뒤의 우클릭도 그 행의 번호(`rows` 기준)를 돌려준다.
#[test]
fn secondary_click_after_scrolling_reports_the_rows_index() {
    let theme = tasty_themes::mocha_fallback();
    let n = 2_000;
    let target = 1_500;
    for layout in ALL {
        let ctx = egui::Context::default();
        frame(&ctx, &theme, layout, n, None, vec![]);
        frame(&ctx, &theme, layout, n, Some(target), vec![]);
        let mut drawn = frame(&ctx, &theme, layout, n, None, vec![]);
        for _ in 0..60 {
            drawn = frame(&ctx, &theme, layout, n, None, vec![]);
        }
        let rect = on_screen(&drawn, target)
            .unwrap_or_else(|| panic!("{layout:?}: row {target} not on screen"));
        let at = pos2(rect.left() + 5.0, rect.center().y);
        let out = press_release(&ctx, &theme, layout, n, at, PointerButton::Secondary);
        assert_eq!(out.secondary_clicked_row, Some(target), "{layout:?}");
        assert_eq!(out.clicked_row, None, "{layout:?}");
    }
}

/// `virtual_rows` 를 켜지 않은 소비처는 기존 경로 그대로 매 프레임 모든 행의 셀을 그린다.
/// 켜면 보이는 행만 그린다.
#[test]
fn only_virtual_tables_skip_rows_off_screen() {
    let theme = tasty_themes::mocha_fallback();
    let n = 500;
    for layout in NON_VIRTUAL {
        let ctx = egui::Context::default();
        frame(&ctx, &theme, layout, n, None, vec![]);
        let drawn = frame(&ctx, &theme, layout, n, None, vec![]);
        let mut seen: Vec<usize> = drawn.rows.iter().map(|(i, _)| *i).collect();
        seen.sort_unstable();
        assert_eq!(
            seen,
            (0..n).collect::<Vec<_>>(),
            "{layout:?}: every row is drawn once"
        );
    }
    let ctx = egui::Context::default();
    frame(&ctx, &theme, Layout::Virtual, n, None, vec![]);
    let drawn = frame(&ctx, &theme, Layout::Virtual, n, None, vec![]);
    let row_h = theme.table_cell_height().value();
    assert!(
        drawn.rows.len() <= (VIEW_H / row_h).ceil() as usize + 2,
        "virtual table drew {} rows",
        drawn.rows.len()
    );
}
