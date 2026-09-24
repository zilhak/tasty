//! 목록과 상세 내용을 같은 영역에서 교체한다. 표시 상태는 호출자가 소유한다.
//! 전환 애니메이션은 없으며 상세 화면은 뒤로 가기 줄을 고정하고 본문만 스크롤한다.
//! 뒤로 가기 아이콘은 직접 그리며 버튼 오른쪽에 호출자의 액션을 배치한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::control::ControlSize;
use crate::icon_button::IconButton;

/// 어느 뷰가 보이는지 — 호출측 소유 상태 (디자인 `view` prop).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DrillDownView {
    #[default]
    List,
    Detail,
}

impl DrillDownView {
    pub fn is_detail(self) -> bool {
        self == DrillDownView::Detail
    }
}

/// back bar 우측 actions 슬롯 renderer — 디테일 액션(예: "Apply" 버튼)의 정위치.
/// 예: `|ui, theme| { Button::new("Apply").show(ui, theme); }`.
pub type DrillDownActions<'a> = &'a dyn Fn(&mut egui::Ui, &Theme);

/// `DrillDown::show` 결과.
pub struct DrillDownOutput {
    /// back bar 의 ← 가 클릭됨 — 호출측이 `view` 를 `List` 로 되돌린다.
    pub back_clicked: bool,
}

/// DrillDown 빌더. 프레젠테이션 설정만 담고, 뷰 상태(`view`)는 호출측 소유.
pub struct DrillDown<'a> {
    id_salt: &'a str,
    view: DrillDownView,
    /// 디테일 제목 (back bar, 말줄임).
    title: &'a str,
    /// ← 버튼 tooltip (호출측이 `t()` 로 넘긴다). 빈 문자열이면 tooltip 없음.
    back_label: &'a str,
    /// 전체 높이. `None` 이면 가용 높이(디자인 기본 — 컨테이너 채움).
    height: Option<f32>,
    /// 전체 폭. `None` 이면 가용 폭.
    width: Option<f32>,
}

impl<'a> DrillDown<'a> {
    pub fn new(id_salt: &'a str) -> Self {
        Self {
            id_salt,
            view: DrillDownView::List,
            title: "",
            back_label: "",
            height: None,
            width: None,
        }
    }

    /// 현재 뷰 (호출측 소유 상태).
    pub fn view(mut self, view: DrillDownView) -> Self {
        self.view = view;
        self
    }

    /// 디테일 제목 — back bar 의 ← 옆에 표시.
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    /// ← 버튼 tooltip.
    pub fn back_label(mut self, back_label: &'a str) -> Self {
        self.back_label = back_label;
        self
    }

    /// 전체 높이 고정. 미지정 시 가용 높이.
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// 전체 폭 고정. 미지정 시 가용 폭.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// view에 해당하는 본문 하나만 호출한다. actions는 상세 화면의 뒤로 가기 줄 오른쪽에 놓인다.
    pub fn show(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        list: impl FnOnce(&mut egui::Ui, &Theme),
        detail: impl FnOnce(&mut egui::Ui, &Theme),
        actions: Option<DrillDownActions<'_>>,
    ) -> DrillDownOutput {
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let height = self.height.unwrap_or_else(|| ui.available_height());
        let mut back_clicked = false;

        ui.scope(|ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            if self.view.is_detail() {
                let bar_h = backbar_height(
                    theme.drilldown_backbar_height(),
                    LogicalPx(ControlSize::Sm.height(theme)),
                    theme.drilldown_backbar_padding_y(),
                );
                back_clicked = self.backbar(ui, theme, width, bar_h.value(), actions);
                egui::ScrollArea::vertical()
                    .id_salt(("tasty_drilldown_detail", self.id_salt))
                    .auto_shrink([false, false])
                    .max_height(body_height(LogicalPx(height), bar_h).value())
                    .drag_to_scroll(false)
                    .show(ui, |ui| detail(ui, theme));
            } else {
                egui::ScrollArea::vertical()
                    .id_salt(("tasty_drilldown_list", self.id_salt))
                    .auto_shrink([false, false])
                    .max_height(height)
                    .drag_to_scroll(false)
                    .show(ui, |ui| list(ui, theme));
            }
        });
        DrillDownOutput { back_clicked }
    }

    /// back bar 밴드. ← 클릭 시 true.
    fn backbar(
        &self,
        ui: &mut egui::Ui,
        theme: &Theme,
        width: f32,
        bar_h: f32,
        actions: Option<DrillDownActions<'_>>,
    ) -> bool {
        let pad_x = theme.drilldown_backbar_padding_x().value();
        let pad_y = theme.drilldown_backbar_padding_y().value();
        let gap = theme.drilldown_backbar_gap().value();

        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, bar_h), egui::Sense::hover());

        let y = rect.bottom() - theme.border_width.value() * 0.5;
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(
                theme.border_width.value(),
                theme.drilldown_backbar_border().to_egui_premultiplied(),
            ),
        );

        let inner = rect.shrink2(egui::vec2(pad_x, pad_y));

        let mut left_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let mut resp = IconButton::new().size(ControlSize::Sm).show(
            &mut left_ui,
            theme,
            &|ui, irect, color| {
                paint_chevron_left(ui, theme, irect.center(), color);
            },
        );
        if !self.back_label.is_empty() {
            resp = resp.on_hover_text(self.back_label);
        }
        let title_left = resp.rect.right() + gap;

        let mut title_right = inner.right();
        if let Some(actions) = actions {
            let mut right_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            right_ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            actions(&mut right_ui, theme);
            title_right = right_ui.min_rect().left() - gap;
        }

        let mut job = egui::text::LayoutJob::simple_singleline(
            self.title.to_owned(),
            egui::FontId::proportional(theme.drilldown_title_font_size().value()),
            theme.drilldown_title_fg().to_egui(),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width((title_right - title_left).max(0.0));
        let galley = ui.fonts(|f| f.layout_job(job));
        let pos = egui::pos2(title_left, rect.center().y - galley.rect.height() * 0.5);
        ui.painter().galley(pos, galley, egui::Color32::PLACEHOLDER);

        resp.clicked()
    }
}

/// back 글리프(‹) — painter 폴리라인 (tree_row chevron 관례).
fn paint_chevron_left(ui: &egui::Ui, theme: &Theme, center: egui::Pos2, color: egui::Color32) {
    let s = 3.5;
    let pts = vec![
        egui::pos2(center.x + s * 0.6, center.y - s),
        egui::pos2(center.x - s * 0.6, center.y),
        egui::pos2(center.x + s * 0.6, center.y + s),
    ];
    ui.painter().add(egui::Shape::line(
        pts,
        egui::Stroke::new(theme.icon_stroke_width.value(), color),
    ));
}

/// back bar 밴드 높이 — `max(backbar-height(36), ← 버튼 + 상하 패딩)`.
fn backbar_height(min_height: LogicalPx, button_h: LogicalPx, pad_y: LogicalPx) -> LogicalPx {
    LogicalPx((button_h.value() + pad_y.value() * 2.0).max(min_height.value()))
}

/// 디테일 본문 높이 — 전체에서 back bar 를 뺀 나머지.
fn body_height(total: LogicalPx, backbar: LogicalPx) -> LogicalPx {
    LogicalPx((total.value() - backbar.value()).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backbar_height_는_최소_36() {
        // sm 버튼 24 + 패딩 8 = 32 < 36 → 36 밴드.
        let h = backbar_height(LogicalPx(36.0), LogicalPx(24.0), LogicalPx(4.0));
        assert_eq!(h.value(), 36.0);
    }

    #[test]
    fn backbar_height_는_내용이_크면_늘어난다() {
        let h = backbar_height(LogicalPx(36.0), LogicalPx(32.0), LogicalPx(4.0));
        assert_eq!(h.value(), 40.0);
    }

    #[test]
    fn body_height_는_음수로_내려가지_않는다() {
        assert_eq!(
            body_height(LogicalPx(200.0), LogicalPx(36.0)).value(),
            164.0
        );
        assert_eq!(body_height(LogicalPx(20.0), LogicalPx(36.0)).value(), 0.0);
    }

    #[test]
    fn view_기본은_list() {
        assert_eq!(DrillDownView::default(), DrillDownView::List);
        assert!(!DrillDownView::List.is_detail());
        assert!(DrillDownView::Detail.is_detail());
    }
}
