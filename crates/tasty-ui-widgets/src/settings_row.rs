//! 설정 서브탭의 행 격자. 라벨 열 · gap · 컨트롤과, 한 행에 딸린 caption 이나 경고 callout.
//!
//! 라벨 열은 서브탭에서 가장 긴 라벨에 맞추고 `settings-label-width` … `settings-label-max-width`
//! 로 clamp 한다. 열보다 긴 라벨은 열 안에서 줄을 바꾼다. 열 폭은 egui 가 자식 영역을 내용 폭으로
//! 줄이지 않도록 정확한 크기로 할당한다.

use std::sync::Arc;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::help_hint::HelpHint;
use crate::icon_button::IconPainter;
use crate::tooltip::TooltipPlacement;
use crate::warning_callout::warning_callout;

/// 행 바로 아래에 붙는 내용.
#[derive(Clone, Copy)]
enum Below<'a> {
    None,
    Caption(&'a str),
    Warning(&'a str, IconPainter<'a>),
}

/// 설정 한 행. 라벨 열 폭을 재는 데도 같은 값을 쓴다.
#[derive(Clone, Copy)]
pub struct SettingsRow<'a> {
    label: &'a str,
    hint: Option<&'a str>,
    below: Below<'a>,
}

impl<'a> SettingsRow<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            hint: None,
            below: Below::None,
        }
    }

    /// 라벨 뒤 물음표 도움말. 아래 caption 과 같은 내용을 두 번 쓰지 않는다.
    pub fn hint(mut self, text: &'a str) -> Self {
        self.hint = Some(text);
        self
    }

    /// 행 아래 muted 설명.
    pub fn caption(mut self, text: &'a str) -> Self {
        self.below = Below::Caption(text);
        self
    }

    /// 행 아래 항상 보이는 경고 callout.
    pub fn warning(mut self, text: &'a str, paint_icon: IconPainter<'a>) -> Self {
        self.below = Below::Warning(text, paint_icon);
        self
    }

    /// 줄을 바꾸지 않았을 때 라벨 칸이 차지하는 폭(도움말 아이콘 포함).
    pub fn natural_label_width(&self, ui: &egui::Ui, theme: &Theme) -> LogicalPx {
        let text = label_galley(ui, theme, self.label, f32::INFINITY).size().x;
        LogicalPx(text) + hint_width(theme, self.hint.is_some())
    }

    /// 행을 그린다. caption·callout 이 있으면 행과 `settings-row-caption-gap` 만큼 띄운다.
    pub fn show(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        label_col: LogicalPx,
        control: impl FnOnce(&mut egui::Ui),
    ) -> egui::Response {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = theme.settings_row_caption_gap().value();
            let row_h = theme.settings_row_min_height();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let label_h = label_cell_height(ui, theme, label_col, self.label, self.hint);
                ui.set_min_height(row_h.max(label_h).value());
                settings_label_cell(ui, theme, label_col, row_h, self.label, self.hint);
                ui.add_space(theme.settings_label_gap().value());
                control(ui);
            });
            match self.below {
                Below::None => {}
                Below::Caption(text) => settings_row_caption(ui, theme, text),
                Below::Warning(text, paint_icon) => {
                    ui.scope(|ui| {
                        ui.set_max_width(theme.measure_md.value());
                        warning_callout(ui, theme, text, paint_icon);
                    });
                }
            }
        })
        .response
    }
}

/// 서브탭의 라벨 열 폭. 가장 긴 라벨을 `settings-label-width` … `settings-label-max-width` 로 clamp 한다.
pub fn settings_label_column<'a>(
    ui: &egui::Ui,
    theme: &Theme,
    rows: impl IntoIterator<Item = &'a SettingsRow<'a>>,
) -> LogicalPx {
    let longest = rows
        .into_iter()
        .map(|row| row.natural_label_width(ui, theme))
        .fold(LogicalPx(0.0), LogicalPx::max);
    longest.clamp(
        theme.settings_label_width(),
        theme.settings_label_max_width(),
    )
}

/// 라벨 열 한 칸. 폭 `label_col` 을 그대로 차지하고, 라벨은 그 안에서 줄을 바꾸며 행 높이
/// `row_h` 안에서 세로 가운데에 놓인다. 행을 직접 짜는 화면(단축키 탭 등)도 이 칸을 쓴다.
pub fn settings_label_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    label_col: LogicalPx,
    row_h: LogicalPx,
    label: &str,
    hint: Option<&str>,
) -> egui::Response {
    let hint_w = hint_width(theme, hint.is_some());
    let galley = label_galley(ui, theme, label, (label_col - hint_w).value().max(1.0));
    let text_size = galley.size();
    let cell_h = row_h.value().max(text_size.y);
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(label_col.value(), cell_h), egui::Sense::hover());
    let text_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - text_size.y / 2.0),
        text_size,
    );
    let last_row = galley.rows.last().map(|r| r.rect);
    // `put` 은 부모 커서를 그 rect 뒤로 옮겨 열 폭을 되돌린다. 자리는 위에서 할당했으므로
    // 글자와 도움말은 커서와 무관한 자식 Ui 에 놓는다.
    let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    text_ui.add(egui::Label::new(galley));
    if let (Some(text), Some(last)) = (hint, last_row) {
        let glyph = theme.icon_glyph_size_sm.value();
        let x = text_rect.left() + last.right() + theme.help_hint_gap().value();
        let y = text_rect.top() + last.center().y - glyph / 2.0;
        let hint_rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(glyph, glyph));
        let mut hint_ui = ui.new_child(egui::UiBuilder::new().max_rect(hint_rect));
        HelpHint::new(text)
            .placement(TooltipPlacement::Bottom)
            .show(&mut hint_ui, theme);
    }
    resp
}

/// [`settings_label_cell`] 뒤 컨트롤까지의 간격을 `settings-label-gap` 으로 맞춘다. 부모 가로 줄의
/// `item_spacing.x` 가 칸 사이에 이미 들어가므로 그만큼 빼고 띄운다.
pub fn settings_label_gap(ui: &mut egui::Ui, theme: &Theme) {
    let auto = ui.spacing().item_spacing.x;
    ui.add_space((theme.settings_label_gap().value() - auto).max(0.0));
}

/// 한 행에 딸린 설명. 행 왼쪽 끝에서 시작하고 `measure-md` 폭에서 줄을 바꾼다.
/// 행과의 간격은 호출하는 쪽의 세로 간격이 정한다([`SettingsRow::caption`] 은 `settings-row-caption-gap`).
pub fn settings_row_caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(theme.measure_md.value());
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            )
            .wrap(),
        );
    });
}

fn hint_width(theme: &Theme, has_hint: bool) -> LogicalPx {
    if has_hint {
        theme.help_hint_gap() + theme.icon_glyph_size_sm
    } else {
        LogicalPx(0.0)
    }
}

fn label_cell_height(
    ui: &egui::Ui,
    theme: &Theme,
    label_col: LogicalPx,
    label: &str,
    hint: Option<&str>,
) -> LogicalPx {
    let wrap = (label_col - hint_width(theme, hint.is_some()))
        .value()
        .max(1.0);
    LogicalPx(label_galley(ui, theme, label, wrap).size().y)
}

fn label_galley(ui: &egui::Ui, theme: &Theme, text: &str, wrap_width: f32) -> Arc<egui::Galley> {
    let font = egui::FontId::proportional(theme.font_size_body.value());
    let color = theme.text_secondary().to_egui();
    ui.fonts(|f| f.layout(text.to_owned(), font, color, wrap_width))
}
