//! 선택 행 띠가 표 영역의 좌우 끝을 넘어 보이지 않는지, 선택 행에서 색을 지정하지 않은 라벨이
//! accent 가 아니라 text-primary 로 그려지는지 검사한다. 계약은 docs/architecture/ui-widgets-crate.md 를 따른다.

use std::cell::RefCell;

use egui::{Pos2, RawInput, Rect, vec2};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Table, TableAlign, TableColumn, TableColumnWidth};

const ROWS: &[&str] = &["alpha", "bravo", "charlie"];
const SELECTED: &str = "bravo";

fn raw() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        ..Default::default()
    }
}

fn columns() -> Vec<TableColumn<'static, ()>> {
    vec![
        TableColumn {
            title: "Name",
            width: TableColumnWidth::Exact(LogicalPx(120.0)),
            align: TableAlign::Left,
            sort_id: None,
        },
        TableColumn {
            title: "Kind",
            width: TableColumnWidth::Remainder {
                at_least: LogicalPx(80.0),
                clip: true,
            },
            align: TableAlign::Left,
            sort_id: None,
        },
    ]
}

/// 두 프레임을 그려 마지막 프레임의 도형, 표를 둔 영역, 선택 행 셀의 기본 글자색을 돌려준다.
fn render(theme: &Theme) -> (Vec<egui::epaint::ClippedShape>, Rect, Vec<egui::Color32>) {
    let ctx = egui::Context::default();
    tasty_egui_theme::apply_theme_to_egui(theme, &ctx);
    let area = RefCell::new(Rect::NOTHING);
    let selected_ink = RefCell::new(Vec::new());
    let mut shapes = Vec::new();
    for _ in 0..2 {
        selected_ink.borrow_mut().clear();
        let out = ctx.run(raw(), |c| {
            egui::CentralPanel::default().show(c, |ui| {
                *area.borrow_mut() = ui.available_rect_before_wrap();
                Table::new(columns()).selectable(true).show(
                    ui,
                    theme,
                    ROWS,
                    |row: &&str| *row == SELECTED,
                    |ui, _th, row: &&str, _col| {
                        if *row == SELECTED {
                            selected_ink.borrow_mut().push(ui.visuals().text_color());
                        }
                        ui.label(*row);
                    },
                );
            });
        });
        shapes = out.shapes;
    }
    (shapes, area.into_inner(), selected_ink.into_inner())
}

/// egui_extras 는 띠를 가로로 item_spacing.x 의 절반씩 넓힌다. 보이는 부분(띠 ∩ clip)은 표 영역 안이어야 한다.
#[test]
fn the_selected_band_stays_inside_the_table_area() {
    let theme = tasty_themes::mocha_fallback();
    let fill: egui::Color32 = theme.table_row_bg_selected().into();
    let (shapes, area, _) = render(&theme);
    let visible: Vec<Rect> = shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::epaint::Shape::Rect(r) if r.fill == fill => Some(r.rect.intersect(c.clip_rect)),
            _ => None,
        })
        .collect();
    assert!(!visible.is_empty(), "no selected band was painted");
    let left = visible
        .iter()
        .map(|r| r.left())
        .fold(f32::INFINITY, f32::min);
    let right = visible
        .iter()
        .map(|r| r.right())
        .fold(f32::NEG_INFINITY, f32::max);
    assert_eq!(
        (left, right),
        (area.left(), area.right()),
        "visible band {visible:?} vs table area {area:?}"
    );
}

#[test]
fn an_uncoloured_label_in_the_selected_row_uses_text_primary() {
    let theme = tasty_themes::mocha_fallback();
    let (_, _, ink) = render(&theme);
    let expected = theme.text_primary().to_egui();
    assert!(
        !ink.is_empty() && ink.iter().all(|c| *c == expected),
        "selected row ink {ink:?}, expected text-primary {expected:?}"
    );
}
