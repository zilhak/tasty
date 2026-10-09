//! Explorer 목록 맨 위에 새 항목 이름을 받는 인라인 입력과 그 아래 오류 상자.
//! Detail·List 는 행 높이(28) 한 줄, Grid 는 칸 글리프 아래 `field-width-md` 폭 입력을 칸 가운데에 둔다.
//! 입력 칸은 공용 Input 이다.
//! 본체와 갤러리가 같은 함수를 불러 같은 모양을 그린다.

use tasty_icons::Icon;
use tasty_type_appearance::theme::Theme;

use crate::input::Input;

/// 입력 줄을 그릴 보기 모드.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExplorerNameLayout {
    /// 줄 왼쪽에서 글리프까지 여백과 이름 열 폭. 호출자의 상세 표 행과 같은 값을 넘긴다.
    /// 입력 칸은 이름 열 안에만 둔다.
    Detail { inset: f32, name_width: f32 },
    /// 아래 `tree_row` 행들과 아이콘 가운데를 맞춘다.
    List,
    /// Grid 칸 크기와 글리프 자리 높이. 호출자의 Grid 칸과 같은 값을 넘긴다.
    Grid { cell: egui::Vec2, slot: f32 },
}

/// 편집 상태. 호출자가 소유하고 프레임마다 넘긴다.
#[derive(Clone, Debug)]
pub struct ExplorerNameEdit {
    pub buf: String,
    /// 처음 그릴 때 입력에 포커스를 주고 고를 글자 범위(문자 단위). 한 번 쓰면 비운다.
    pub initial_selection: Option<std::ops::Range<usize>>,
}

impl ExplorerNameEdit {
    /// 기본 이름을 넣고 처음 프레임에 `select` 범위를 고르게 한다.
    pub fn new(name: String, select: std::ops::Range<usize>) -> Self {
        Self {
            buf: name,
            initial_selection: Some(select),
        }
    }
}

/// 이번 프레임의 입력 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplorerNameEvent {
    None,
    /// Enter.
    Confirm,
    /// Esc.
    Cancel,
    /// 포커스를 잃었다. 호출자가 이름이 유효하면 확정하고 아니면 취소한다.
    Blur,
}

/// 편집 줄을 그린다. 차지한 줄 rect 와 입력 칸 rect, 이벤트를 돌려준다.
pub fn explorer_name_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    layout: ExplorerNameLayout,
    glyph: Icon,
    edit: &mut ExplorerNameEdit,
    invalid: bool,
) -> (egui::Rect, egui::Rect, ExplorerNameEvent) {
    let row_h = theme.table_cell_height().value();
    let glyph_size = theme.icon_glyph_size_md.value();
    let gap = theme.spacing_sm.value();
    let muted = theme.text_muted().to_egui();
    match layout {
        ExplorerNameLayout::Detail { .. } | ExplorerNameLayout::List => {
            let (row, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), row_h),
                egui::Sense::hover(),
            );
            let pad = theme.spacing_sm.value();
            let (radius, inset, field_right) = match layout {
                ExplorerNameLayout::Detail { inset, name_width } => {
                    (0.0, inset, row.left() + name_width)
                }
                _ => (
                    theme.corner_radius_sm.value(),
                    crate::tree_row::tree_row_icon_center(theme) - glyph_size * 0.5,
                    row.right(),
                ),
            };
            ui.painter()
                .rect_filled(row, radius, theme.surface_active().to_egui());
            let glyph_rect = egui::Rect::from_center_size(
                egui::pos2(row.left() + inset + glyph_size * 0.5, row.center().y),
                egui::vec2(glyph_size, glyph_size),
            );
            glyph.image(glyph_size, muted).paint_at(ui, glyph_rect);
            let field = egui::Rect::from_min_max(
                egui::pos2(glyph_rect.right() + gap, row.top()),
                egui::pos2(
                    (field_right - pad).max(glyph_rect.right() + gap),
                    row.bottom(),
                ),
            );
            let event = field_ui(ui, theme, field, edit, invalid);
            (row, field, event)
        }
        ExplorerNameLayout::Grid { cell, slot } => {
            let (rect, _) = ui.allocate_exact_size(cell, egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                theme.corner_radius.value(),
                theme.surface_active().to_egui(),
            );
            let pad = theme.spacing_sm.value();
            let glyph_rect = egui::Rect::from_center_size(
                egui::pos2(rect.center().x, rect.top() + pad + slot * 0.5),
                egui::vec2(glyph_size, glyph_size),
            );
            glyph.image(glyph_size, muted).paint_at(ui, glyph_rect);
            // 입력은 칸보다 넓어 양옆 칸 위로 나온다. 목록 가장자리의 칸이면 보이는 영역 안으로 민다.
            let width = theme.field_width_md.value();
            let field = egui::Rect::from_min_size(
                egui::pos2(
                    rect.center().x - width * 0.5,
                    rect.top() + pad + slot + theme.spacing_xs.value(),
                ),
                egui::vec2(width, theme.input_height().value()),
            );
            let field = field
                .translate(egui::Vec2::X * inside_shift(field.x_range(), ui.clip_rect().x_range()));
            let event = field_ui(ui, theme, field, edit, invalid);
            (rect, field, event)
        }
    }
}

/// `span` 을 `bounds` 안으로 옮기는 가로 거리. 들어가지 않으면 왼쪽 끝을 맞춘다.
fn inside_shift(span: egui::Rangef, bounds: egui::Rangef) -> f32 {
    if span.min < bounds.min {
        bounds.min - span.min
    } else if span.max > bounds.max {
        (bounds.max - span.max).max(bounds.min - span.min)
    } else {
        0.0
    }
}

fn field_ui(
    ui: &mut egui::Ui,
    theme: &Theme,
    field: egui::Rect,
    edit: &mut ExplorerNameEdit,
    invalid: bool,
) -> ExplorerNameEvent {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let resp =
        Input::new()
            .invalid(invalid)
            .width(field.width())
            .show(&mut child, theme, &mut edit.buf);
    if let Some(range) = edit.initial_selection.take() {
        resp.request_focus();
        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(range.start),
                    egui::text::CCursor::new(range.end),
                )));
            state.store(ui.ctx(), resp.id);
        }
        return ExplorerNameEvent::None;
    }
    if resp.lost_focus() {
        let (enter, esc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        return if enter {
            ExplorerNameEvent::Confirm
        } else if esc {
            ExplorerNameEvent::Cancel
        } else {
            ExplorerNameEvent::Blur
        };
    }
    ExplorerNameEvent::None
}

/// 입력 칸 바로 아래에 오류 상자를 띄운다. 아래 행을 덮으며 목록은 밀리지 않는다.
/// menu-bg 바탕, 1px `explorer-name-error-fg` 테두리, caption text-primary, 최대 폭 240.
pub fn explorer_name_error(ui: &egui::Ui, theme: &Theme, field: egui::Rect, text: &str) {
    let pad_x = theme.spacing_sm.value();
    let pad_y = theme.spacing_xs.value();
    let max_w = theme.explorer_name_error_max_width().value();
    let font = egui::FontId::proportional(theme.font_size_caption.value());
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        font.clone(),
        theme.text_primary().to_egui(),
        max_w - 2.0 * pad_x,
    );
    job.sections[0].format.line_height = Some(font.size * theme.line_height_ui);
    let galley = ui.painter().layout_job(job);
    let size = galley.size() + egui::vec2(2.0 * pad_x, 2.0 * pad_y);
    let min = egui::pos2(
        field.left(),
        field.bottom() + crate::tokens::STRUCT_GAP_2.value(),
    );
    let rect = egui::Rect::from_min_size(min, size);
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        ui.id().with("explorer_name_error"),
    ));
    let radius = theme.corner_radius.value();
    painter.add(theme.shadow_popover().to_egui().as_shape(rect, radius));
    painter.rect(
        rect,
        radius,
        theme.menu_bg().to_egui(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.explorer_name_error_fg().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        rect.min + egui::vec2(pad_x, pad_y),
        galley,
        theme.text_primary().to_egui(),
    );
}

#[cfg(test)]
mod tests;
