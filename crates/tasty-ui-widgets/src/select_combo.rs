//! 검색칸처럼 공용 Select 가 갖지 못한 목록을 쓰는 egui ComboBox 를 Select 와 같은 테두리로 그린다.

use tasty_type_appearance::theme::Theme;

/// `add` 안의 egui ComboBox 트리거를 Select 테두리로 그린다. 닫힘 `select-border`, 호버·누름
/// `border-strong`, 목록이 열림 `select-border-focus` 다. 펼친 목록은 메뉴 팝오버 틀이다.
///
/// egui 의 ComboBox 목록은 부모 `Ui` 의 위젯 스타일을 물려받지 않으므로(목록은 별도 Area 다)
/// 여기서 바꾼 위젯 테두리는 트리거에만 닿는다.
pub fn with_select_combo_frame<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let prev = ui.style().clone();
    let stroke = |c: tasty_type_appearance::color::HexColor| {
        egui::Stroke::new(theme.border_width.value(), c.to_egui())
    };
    let widgets = &mut ui.style_mut().visuals.widgets;
    widgets.inactive.bg_stroke = stroke(theme.select_border());
    widgets.hovered.bg_stroke = stroke(theme.border_strong());
    widgets.active.bg_stroke = stroke(theme.border_strong());
    widgets.open.bg_stroke = stroke(theme.select_border_focus());
    let out = tasty_egui_theme::with_popover_frame(ui, theme, add);
    ui.set_style(prev);
    out
}
