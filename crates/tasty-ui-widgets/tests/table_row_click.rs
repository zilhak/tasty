//! `Table` 행 클릭 계약 회귀 테스트.
//!
//! 재현 대상 버그: `selectable(true)` 표에서 셀 텍스트(`ui.label`) 위를 클릭하면
//! 행이 선택되지 않았다. egui 기본 `interaction.selectable_labels = true` 가
//! 라벨에 `Sense::click_and_drag()` 를 붙이는데, 이 라벨은 셀 `Ui` 의 sense 보다
//! 나중에 등록되므로 hit-test 동률에서 앞선다 → `tr.response()` 가 클릭을 못 받고
//! `clicked_row` 가 `None` 으로 떨어진다(글자 위 hover 커서도 I-beam 이 됐다).
//!
//! 근거·트레이드오프: `docs/adr/0069-table-row-click-over-cell-text-selection.md`.

use std::cell::RefCell;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
use tasty_type_appearance::theme::Theme;
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
                        at_least: 140.0,
                        clip: true,
                    },
                    align: TableAlign::Left,
                    sort_id: Some(Col::Name),
                },
                TableColumn {
                    title: "Kind",
                    width: TableColumnWidth::Initial {
                        initial: 92.0,
                        at_least: 72.0,
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

/// 헤더 행 중앙 좌표. 헤더 높이는 `Table` 기본값(`table_font_size + 10`).
fn header_pos(theme: &Theme, probe: &Probe) -> Pos2 {
    let header_h = theme.table_font_size().value() + 10.0;
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
        "셀 텍스트 위 클릭이 행 클릭으로 잡혀야 한다 \
         (버그: 라벨이 hit-test 를 가져가 clicked_row 가 None 이었다)"
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
