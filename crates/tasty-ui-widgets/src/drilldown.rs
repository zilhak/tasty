//! `DrillDown` — master→detail content-swap 레이아웃 (디자인
//! `components/navigation/DrillDown`).
//!
//! 경계 잡힌 콘텐츠 영역 안에서 풀폭 **리스트 뷰**와 풀폭 **디테일 뷰**를 전면
//! 교체(side-by-side 분할 아님)한다. 항목을 고르면 영역 전체가 그 디테일이 되고,
//! 상단 고정 **back bar**(ghost ← IconButton + 디테일 제목 + 우측 actions 슬롯)로
//! 리스트로 돌아온다. "풀폭 리스트 하나 → 항목 선택 → 디테일 → back" 모델 어디든
//! 사용 — 예: Settings › Keybindings › Preset ([`crate::ListCtrl`] 와 짝).
//!
//! 디자인 계약 (`DrillDown.jsx` / changelog `2026-07-09-settings-preset-drilldown`):
//! - **Controlled** — 어느 뷰를 보일지는 호출측이 `view` 로 소유한다.
//! - 전환은 **즉시**(0ms) — calm/0ms-terminal 시스템 준수. 디자인의 opt-in
//!   `animate`(reduced-motion 인지 cross-fade)는 장식이므로 전사하지 않는다.
//! - 컨테이너를 채운다(100% 높이). 디테일 본문은 내부 스크롤 — back bar 고정.
//! - back bar: 36px 밴드(`--tasty-drilldown-backbar-height`), padding 4/8,
//!   gap 8, 하단 `separator` 헤어라인. ← 는 ghost [`crate::IconButton`] sm +
//!   chevronLeft 글리프. 제목 body(13) `title-fg`(semibold 은 egui weight 한계로
//!   색 강조 관례 — `button.rs` 참조), 말줄임. actions 는 우측 정렬(gap space-sm).
//!
//! back 글리프는 tree_row 관례대로 painter 폴리라인으로 위젯이 직접 그린다
//! (이 crate 는 본체 icons 미의존).

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

    /// 레이아웃을 그린다. `view` 에 따라 `list` 또는 `detail` 본문 **하나만**
    /// 호출된다(content swap). `actions` 는 back bar 우측 슬롯 — 디테일 액션
    /// (예: "Apply")의 정위치 (모달 푸터의 Cancel/Save 와 분리).
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
                // 디테일 본문 — 내부 스크롤 (back bar 고정).
                egui::ScrollArea::vertical()
                    .id_salt(("tasty_drilldown_detail", self.id_salt))
                    .auto_shrink([false, false])
                    .max_height(body_height(LogicalPx(height), bar_h).value())
                    .show(ui, |ui| detail(ui, theme));
            } else {
                // 리스트 뷰 — 영역 전체 스크롤.
                egui::ScrollArea::vertical()
                    .id_salt(("tasty_drilldown_list", self.id_salt))
                    .auto_shrink([false, false])
                    .max_height(height)
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

        // 하단 헤어라인.
        let y = rect.bottom() - theme.border_width.value() * 0.5;
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(
                theme.border_width.value(),
                theme.drilldown_backbar_border().to_egui_premultiplied(),
            ),
        );

        let inner = rect.shrink2(egui::vec2(pad_x, pad_y));

        // ← ghost IconButton (sm, chevronLeft 글리프).
        let mut left_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let mut resp = IconButton::new().size(ControlSize::Sm).show(
            &mut left_ui,
            theme,
            &|ui, irect, color| {
                paint_chevron_left(ui, irect.center(), color);
            },
        );
        if !self.back_label.is_empty() {
            resp = resp.on_hover_text(self.back_label);
        }
        let title_left = resp.rect.right() + gap;

        // actions 슬롯 (우측 정렬, 항목 간 gap space-sm).
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

        // 제목 — body(13) title-fg, 말줄임.
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
fn paint_chevron_left(ui: &egui::Ui, center: egui::Pos2, color: egui::Color32) {
    let s = 3.5;
    let pts = vec![
        egui::pos2(center.x + s * 0.6, center.y - s),
        egui::pos2(center.x - s * 0.6, center.y),
        egui::pos2(center.x + s * 0.6, center.y + s),
    ];
    ui.painter()
        .add(egui::Shape::line(pts, egui::Stroke::new(1.5, color)));
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
