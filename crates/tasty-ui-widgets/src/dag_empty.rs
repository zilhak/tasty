//! Task DAG 빈 상태. 본체 DAG surface·DAG 목록 popup과 갤러리가 함께 호출한다.
//!
//! 디자인 `DagEmpty`: 글리프(text-disabled) → space-sm → 제목(body, text-secondary) → space-sm →
//! 안내(caption, text-muted, measure-sm 폭에서 줄바꿈)를 영역 가운데에 쌓는다. 안내의 백틱 구간
//! (CLI 명령)은 code run으로 그린다. 문구와 글리프는 호출부가 고른다.

use tasty_icons::Icon;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::ui_code::{paint_ui_copy, ui_copy_job, ui_copy_size};

/// 빈 상태 글리프 크기. 24px 아이콘 토큰이 아직 없어 시안 값을 그대로 둔다.
pub const DAG_EMPTY_ICON_SIZE: LogicalPx = LogicalPx(24.0);

/// 빈 상태 문구와 글리프.
pub struct DagEmptyView<'a> {
    pub icon: Icon,
    pub title: &'a str,
    pub hint: &'a str,
}

/// `rect` 가운데에 빈 상태를 그린다. 배경은 칠하지 않는다.
pub fn paint_dag_empty(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, view: &DagEmptyView<'_>) {
    let side = DAG_EMPTY_ICON_SIZE.value();
    let gap = theme.spacing_sm.value();
    let title_font = egui::FontId::proportional(theme.font_size_body.value());
    let title_h = ui.fonts(|f| f.row_height(&title_font));
    let measure = theme
        .measure_sm
        .value()
        .min(rect.width() - theme.spacing_xl.value() * 2.0)
        .max(theme.spacing_xl.value());
    let body = ui.fonts(|f| {
        f.layout_job(ui_copy_job(
            theme,
            view.hint,
            theme.font_size_caption,
            theme.text_muted().to_egui(),
            measure,
        ))
    });
    let body_w = ui_copy_size(theme, &body).x;

    let total = side + gap + title_h + gap + body.size().y;
    let mut y = rect.center().y - total / 2.0;
    view.icon
        .image(side, theme.text_disabled().to_egui())
        .paint_at(
            ui,
            egui::Rect::from_min_size(
                egui::pos2(rect.center().x - side / 2.0, y),
                egui::vec2(side, side),
            ),
        );
    y += side + gap;
    ui.painter().text(
        egui::pos2(rect.center().x, y),
        egui::Align2::CENTER_TOP,
        view.title,
        title_font,
        theme.text_secondary().to_egui(),
    );
    y += title_h + gap;
    paint_ui_copy(
        ui.painter(),
        theme,
        egui::pos2(rect.center().x - body_w / 2.0, y),
        body,
    );
}
