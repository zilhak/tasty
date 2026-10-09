//! `virtual_rows` 표가 보이는 행과 앞의 고정 행만 그리는지, `scroll_to_row` 로 화면 밖 행에
//! 가는지, 안 그린 행의 자리를 비워 마지막 행까지 스크롤되는지, 클릭한 행 번호가 `rows`
//! 기준인지 검사한다. 탐색기 상세 보기처럼 표를 바깥 세로 ScrollArea 안에 둔다.

use std::cell::RefCell;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Table, TableAlign, TableColumn, TableColumnWidth, TableOutput};

const ROWS: usize = 10_000;
const VIEW_H: f32 = 400.0;

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

fn click(p: Pos2) -> Vec<Event> {
    let btn = |pressed| Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::default(),
    };
    vec![Event::PointerMoved(p), btn(true), btn(false)]
}

/// 한 프레임을 그린다. col 0 을 그린 행 번호와 그 rect 를 돌려준다.
fn frame(
    ctx: &egui::Context,
    theme: &Theme,
    virtual_rows: Option<usize>,
    scroll_to: Option<usize>,
    events: Vec<Event>,
) -> (TableOutput<()>, Vec<(usize, Rect)>) {
    let rows: Vec<usize> = (0..ROWS).collect();
    let drawn = RefCell::new(Vec::new());
    let mut out = None;
    let _output = ctx.run(raw(ctx, events), |c| {
        egui::CentralPanel::default().show(c, |ui| {
            let shown = egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let columns = vec![TableColumn {
                        title: "Name",
                        width: TableColumnWidth::Remainder {
                            at_least: LogicalPx(140.0),
                            clip: true,
                        },
                        align: TableAlign::Left,
                        sort_id: None,
                    }];
                    let mut table = Table::new(columns)
                        .selectable(true)
                        .scroll_to_row(scroll_to);
                    if let Some(pinned) = virtual_rows {
                        table = table.virtual_rows(pinned);
                    }
                    table.show(
                        ui,
                        theme,
                        &rows,
                        |_| false,
                        |ui, _th, row: &usize, _col| {
                            let resp = ui.label(format!("row {row}"));
                            drawn.borrow_mut().push((*row, resp.rect));
                        },
                    )
                });
            out = Some(shown.inner);
        });
    });
    (out.expect("table drawn"), drawn.into_inner())
}

/// `row` 로 스크롤을 요청하고 애니메이션이 끝날 만큼 프레임을 돌린 뒤 그린 행을 돌려준다.
fn settle_at(ctx: &egui::Context, theme: &Theme, pinned: usize, row: usize) -> Vec<(usize, Rect)> {
    frame(ctx, theme, Some(pinned), Some(row), vec![]);
    let mut drawn = Vec::new();
    for _ in 0..60 {
        drawn = frame(ctx, theme, Some(pinned), None, vec![]).1;
    }
    drawn
}

fn on_screen(drawn: &[(usize, Rect)], row: usize) -> Option<Rect> {
    drawn
        .iter()
        .find(|(i, r)| *i == row && r.center().y > 0.0 && r.center().y < VIEW_H)
        .map(|(_, r)| *r)
}

#[test]
fn only_visible_and_pinned_rows_are_drawn() {
    let theme = tasty_themes::mocha_fallback();
    let row_h = theme.table_cell_height().value();
    let ctx = egui::Context::default();
    frame(&ctx, &theme, Some(1), None, vec![]);
    let (_, drawn) = frame(&ctx, &theme, Some(1), None, vec![]);
    let visible = (VIEW_H / row_h).ceil() as usize;
    assert!(drawn.iter().any(|(i, _)| *i == 0));
    assert!(
        drawn.len() <= visible + 4,
        "{} rows drawn for about {visible} visible",
        drawn.len()
    );

    let (_, every) = frame(&ctx, &theme, None, None, vec![]);
    assert_eq!(every.len(), ROWS, "a plain table still draws every row");
}

#[test]
fn a_far_row_is_reached_with_the_pinned_rows_still_drawn_and_clicks_report_its_index() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    frame(&ctx, &theme, Some(2), None, vec![]);
    let drawn = settle_at(&ctx, &theme, 2, 7_000);
    let rows: Vec<usize> = drawn.iter().map(|(i, _)| *i).collect();
    assert!(
        rows.contains(&0) && rows.contains(&1),
        "pinned rows: {rows:?}"
    );
    assert!(rows.len() < 60, "{} rows drawn", rows.len());
    let rect =
        on_screen(&drawn, 7_000).unwrap_or_else(|| panic!("row 7000 not on screen: {rows:?}"));

    let (out, _) = frame(
        &ctx,
        &theme,
        Some(2),
        None,
        click(pos2(rect.left() + 5.0, rect.center().y)),
    );
    assert_eq!(out.clicked_row, Some(7_000));
}

#[test]
fn the_skipped_rows_keep_their_space_down_to_the_last_row() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    frame(&ctx, &theme, Some(1), None, vec![]);
    let drawn = settle_at(&ctx, &theme, 1, ROWS - 1);
    let last = on_screen(&drawn, ROWS - 1).expect("last row on screen");
    let before = on_screen(&drawn, ROWS - 2).expect("the row before it on screen");
    let row_h = theme.table_cell_height().value();
    assert!((last.top() - before.top() - row_h).abs() < 0.5);
}
