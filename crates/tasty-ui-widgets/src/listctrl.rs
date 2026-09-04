//! `ListCtrl` — 행 선택형 내비게이션 리스트 (디자인 `components/data/ListCtrl`).
//!
//! Table(다컬럼·정렬 데이터 그리드)과 달리 "하나 골라 진입하는(pick one to drill
//! into)" 풀폭 리스트다. 각 행: 주 라벨 + 선택적 보조 description + 선택적 leading
//! 아이콘 / trailing 슬롯(Tag/Badge — 예: "Active" 마커) + 기본으로 우측 drill-in
//! chevron(클릭 시 디테일 진입 신호). [`crate::DrillDown`] 과 짝지어 list → detail
//! content-swap 에 쓴다.
//!
//! 디자인 계약 (`ListCtrl.jsx`):
//! - 행 상태: default / hover(`overlay-hover`) / selected(`surface-active` + 2px
//!   accent 좌측 바 — sidebar/list idiom) / disabled(opacity, chevron 숨김).
//! - `divided`(기본 on): 마지막 행 제외 `separator` 헤어라인 + 해당 행 radius 0.
//! - 행 min-height 36(`--tasty-listctrl-row-min-height`) — description 있으면
//!   내용 높이(label + 1px + desc + 상하 패딩)만큼 늘어난다.
//! - label body(13) `label-fg` → hover/selected `label-fg-active`. desc caption(11)
//!   muted. 둘 다 넘치면 말줄임(ellipsis).
//! - trailing 슬롯과 chevron 은 우측 정렬, 간격 `space-sm`.
//! - 빈 목록: `space-lg` 패딩의 중앙 muted `empty_label` (비상호작용).
//!
//! 아이콘 시스템은 **호출측 소유** — [`IconPainter`] 주입(이 crate 는 본체 icons
//! 미의존). chevron 은 tree_row 관례대로 위젯이 painter 로 직접 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::icon_button::IconPainter;
use crate::tokens::STRUCT_GAP_1;

/// trailing 슬롯 renderer: 행 우측(chevron 왼쪽)에 Tag/Badge 등을 그린다.
/// 예: `|ui, theme| { tag(ui, theme, "Active", TagVariant::Success, true); }`.
pub type ListCtrlTrailing<'a> = &'a dyn Fn(&mut egui::Ui, &Theme);

/// ListCtrl 한 행. 디자인 `ListCtrlItem` (id 는 인덱스로 갈음 — 호출측이
/// `clicked` 인덱스로 자기 모델의 id 를 역참조한다).
pub struct ListCtrlItem<'a> {
    /// 주 라벨 (body 13).
    pub label: &'a str,
    /// 보조 줄 (muted, caption 11). 라벨 아래 1px 간격.
    pub description: Option<&'a str>,
    /// leading 글리프 (icon-size-md 16, muted).
    pub icon: Option<IconPainter<'a>>,
    /// chevron 앞 trailing 슬롯 (Tag/Badge 등).
    pub trailing: Option<ListCtrlTrailing<'a>>,
    /// 선택 불가·흐림 처리 (chevron 도 숨김).
    pub disabled: bool,
}

impl<'a> ListCtrlItem<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            description: None,
            icon: None,
            trailing: None,
            disabled: false,
        }
    }

    pub fn description(mut self, description: &'a str) -> Self {
        self.description = Some(description);
        self
    }

    pub fn icon(mut self, icon: IconPainter<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn trailing(mut self, trailing: ListCtrlTrailing<'a>) -> Self {
        self.trailing = Some(trailing);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// `ListCtrl::show` 결과.
pub struct ListCtrlOutput {
    /// 이번 프레임에 클릭된 행의 인덱스 (`items` 기준). disabled 행은 제외.
    pub clicked: Option<usize>,
}

/// ListCtrl 빌더. 프레젠테이션 설정만 담고, 상태(`items`/`selected`)는 `show` 인자.
pub struct ListCtrl<'a> {
    chevron: bool,
    divided: bool,
    empty_label: &'a str,
    /// 리스트 폭. `None` 이면 가용 폭(디자인 기본 — 풀폭).
    width: Option<f32>,
}

impl Default for ListCtrl<'_> {
    fn default() -> Self {
        Self {
            chevron: true,
            divided: true,
            empty_label: "",
            width: None,
        }
    }
}

impl<'a> ListCtrl<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// 우측 drill-in chevron 표시 여부. 기본 true.
    pub fn chevron(mut self, chevron: bool) -> Self {
        self.chevron = chevron;
        self
    }

    /// 행 사이 헤어라인. 기본 true.
    pub fn divided(mut self, divided: bool) -> Self {
        self.divided = divided;
        self
    }

    /// 빈 목록일 때 표시할 비상호작용 라벨 (호출측이 `t()` 로 넘긴다).
    pub fn empty_label(mut self, empty_label: &'a str) -> Self {
        self.empty_label = empty_label;
        self
    }

    /// 리스트 폭 고정. 미지정 시 가용 폭.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// 리스트를 그린다. `selected` 는 선택 행 인덱스 (호출측 소유 상태).
    pub fn show(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        items: &[ListCtrlItem<'_>],
        selected: Option<usize>,
    ) -> ListCtrlOutput {
        let width = self.width.unwrap_or_else(|| ui.available_width());

        if items.is_empty() {
            self.empty_state(ui, theme, width);
            return ListCtrlOutput { clicked: None };
        }

        let mut clicked = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, item) in items.iter().enumerate() {
                let is_last = i + 1 == items.len();
                if self.row(ui, theme, item, width, selected == Some(i), is_last) {
                    clicked = Some(i);
                }
            }
        });
        ListCtrlOutput { clicked }
    }

    /// 빈 목록 상태 — space-lg 패딩, 중앙, muted, body.
    fn empty_state(&self, ui: &mut egui::Ui, theme: &Theme, width: f32) {
        let pad = theme.spacing_lg.value();
        let galley = ui.painter().layout(
            self.empty_label.to_owned(),
            egui::FontId::proportional(theme.listctrl_font_size().value()),
            theme.text_muted().to_egui(),
            (width - pad * 2.0).max(0.0),
        );
        let h = galley.rect.height() + pad * 2.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
        let pos = egui::pos2(
            rect.center().x - galley.rect.width() * 0.5,
            rect.center().y - galley.rect.height() * 0.5,
        );
        ui.painter().galley(pos, galley, egui::Color32::PLACEHOLDER);
    }

    /// 행 하나. 클릭되면 true.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        ui: &mut egui::Ui,
        theme: &Theme,
        item: &ListCtrlItem<'_>,
        width: f32,
        is_selected: bool,
        is_last: bool,
    ) -> bool {
        let pad_x = theme.listctrl_row_padding_x().value();
        let pad_y = theme.listctrl_row_padding_y().value();
        let gap = theme.listctrl_row_gap().value();
        let label_font = egui::FontId::proportional(theme.listctrl_font_size().value());
        let desc_font = egui::FontId::proportional(theme.listctrl_desc_font_size().value());

        // 내용 높이 → 행 높이 (min-height 36, description 있으면 늘어남).
        let label_h = ui.fonts(|f| f.row_height(&label_font));
        let desc_h = item
            .description
            .map(|_| ui.fonts(|f| f.row_height(&desc_font)));
        let row_h = row_height(
            theme.listctrl_row_min_height(),
            LogicalPx(pad_y),
            LogicalPx(label_h),
            desc_h.map(LogicalPx),
            STRUCT_GAP_1,
        )
        .value();

        let sense = if item.disabled {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row_h), sense);
        let hovered = !item.disabled && resp.hovered();
        let radius = row_radius(theme.listctrl_radius(), self.divided, is_last).value();

        // 배경: selected(surface-active) > hover(overlay-hover).
        if is_selected {
            ui.painter()
                .rect_filled(rect, radius, theme.listctrl_row_bg_selected().to_egui());
            // 2px accent 좌측 바 (inset box-shadow 전사).
            let bar = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(
                    rect.left() + theme.listctrl_selected_bar_width().value(),
                    rect.bottom(),
                ),
            );
            ui.painter()
                .rect_filled(bar, 0.0, theme.listctrl_selected_bar().to_egui());
        } else if hovered {
            ui.painter().rect_filled(
                rect,
                radius,
                theme.listctrl_row_bg_hover().to_egui_premultiplied(),
            );
        }

        // divided: 마지막 행 제외 하단 헤어라인.
        if self.divided && !is_last {
            let y = rect.bottom() - theme.border_width.value() * 0.5;
            ui.painter().line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                egui::Stroke::new(
                    theme.border_width.value(),
                    theme.listctrl_divider().to_egui_premultiplied(),
                ),
            );
        }

        let dim = |c: egui::Color32| {
            if item.disabled {
                c.gamma_multiply(theme.opacity_disabled())
            } else {
                c
            }
        };

        let mut x = rect.left() + pad_x;

        // leading 아이콘 (icon-size-md, muted).
        if let Some(paint) = item.icon {
            let glyph = theme.icon_glyph_size_md.value();
            let irect = egui::Rect::from_center_size(
                egui::pos2(x + glyph * 0.5, rect.center().y),
                egui::vec2(glyph, glyph),
            );
            paint(ui, irect, dim(theme.listctrl_icon_fg().to_egui()));
            x += glyph + gap;
        }

        // 우측: chevron (가장 오른쪽) ← trailing 슬롯 순으로 배치.
        let mut right = rect.right() - pad_x;
        if self.chevron && !item.disabled {
            let glyph = theme.icon_glyph_size_sm.value();
            paint_chevron_right(
                ui,
                theme,
                egui::pos2(right - glyph * 0.5, rect.center().y),
                theme.listctrl_chevron_fg().to_egui(),
            );
            right -= glyph + theme.spacing_sm.value();
        }
        if let Some(trailing) = item.trailing {
            let trail_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left() + pad_x, rect.top()),
                egui::pos2(right, rect.bottom()),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(trail_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            if item.disabled {
                child.disable();
            }
            trailing(&mut child, theme);
            right = child.min_rect().left() - theme.spacing_sm.value();
        }

        // 텍스트 컬럼 (label 위 / desc 아래, 1px 간격, 말줄임).
        let text_w = (right - x - gap).max(0.0);
        let label_fg = if is_selected || hovered {
            theme.listctrl_label_fg_active().to_egui()
        } else {
            theme.listctrl_label_fg().to_egui()
        };
        let label_galley = truncated_galley(ui, item.label, label_font, dim(label_fg), text_w);
        let desc_galley = item.description.map(|d| {
            truncated_galley(
                ui,
                d,
                desc_font,
                dim(theme.listctrl_desc_fg().to_egui()),
                text_w,
            )
        });
        let content_h = label_galley.rect.height()
            + desc_galley
                .as_ref()
                .map(|g| g.rect.height() + STRUCT_GAP_1.value())
                .unwrap_or(0.0);
        let mut y = rect.center().y - content_h * 0.5;
        ui.painter().galley(
            egui::pos2(x, y),
            label_galley.clone(),
            egui::Color32::PLACEHOLDER,
        );
        y += label_galley.rect.height() + STRUCT_GAP_1.value();
        if let Some(g) = desc_galley {
            ui.painter()
                .galley(egui::pos2(x, y), g, egui::Color32::PLACEHOLDER);
        }

        !item.disabled && resp.clicked()
    }
}

/// 한 줄 말줄임 galley — 폭 초과 시 '…' 로 잘라낸다 (디자인 ellipsis 전사).
fn truncated_galley(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
    ui.fonts(|f| f.layout_job(job))
}

/// drill-in chevron(›) — tree_row 관례대로 painter 폴리라인으로 직접 그린다.
fn paint_chevron_right(ui: &egui::Ui, theme: &Theme, center: egui::Pos2, color: egui::Color32) {
    let s = 3.0;
    let pts = vec![
        egui::pos2(center.x - s * 0.6, center.y - s),
        egui::pos2(center.x + s * 0.6, center.y),
        egui::pos2(center.x - s * 0.6, center.y + s),
    ];
    ui.painter().add(egui::Shape::line(
        pts,
        egui::Stroke::new(theme.icon_stroke_width.value(), color),
    ));
}

/// 행 높이 — `max(row-min-height, label + (1px + desc) + 상하 패딩)`.
fn row_height(
    min_height: LogicalPx,
    pad_y: LogicalPx,
    label_h: LogicalPx,
    desc_h: Option<LogicalPx>,
    desc_gap: LogicalPx,
) -> LogicalPx {
    let content = label_h.value() + desc_h.map(|d| d.value() + desc_gap.value()).unwrap_or(0.0);
    LogicalPx((content + pad_y.value() * 2.0).max(min_height.value()))
}

/// 행 radius — divided 모드에선 헤어라인을 가진(마지막 아닌) 행만 radius 0
/// (디자인 CSS `.tasty-listctrl--divided .row:not(:last-child) { border-radius: 0 }`).
fn row_radius(radius: LogicalPx, divided: bool, is_last: bool) -> LogicalPx {
    if divided && !is_last {
        LogicalPx(0.0)
    } else {
        radius
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_height_desc_없으면_min_height() {
        // label 13 + pad 16 = 29 < 36 → min-height 로 클램프.
        let h = row_height(
            LogicalPx(36.0),
            LogicalPx(8.0),
            LogicalPx(13.0),
            None,
            LogicalPx(1.0),
        );
        assert_eq!(h.value(), 36.0);
    }

    #[test]
    fn row_height_desc_있으면_내용만큼_증가() {
        // 13 + 1 + 11 + 16 = 41 > 36.
        let h = row_height(
            LogicalPx(36.0),
            LogicalPx(8.0),
            LogicalPx(13.0),
            Some(LogicalPx(11.0)),
            LogicalPx(1.0),
        );
        assert_eq!(h.value(), 41.0);
    }

    #[test]
    fn row_radius_divided_는_마지막_행만_radius_유지() {
        let r = LogicalPx(2.0);
        assert_eq!(row_radius(r, true, false).value(), 0.0);
        assert_eq!(row_radius(r, true, true).value(), 2.0);
        assert_eq!(row_radius(r, false, false).value(), 2.0);
        assert_eq!(row_radius(r, false, true).value(), 2.0);
    }
}
