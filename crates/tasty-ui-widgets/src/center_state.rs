//! 목록 자리에 들어가는 빈·로딩·오류 중앙 블록(CenterState).
//! 글리프(로딩은 스피너)·제목·보조 줄로 이루어지며 받은 영역 안에서 세로 가운데에 놓인다.
//! 보조 줄이 없어도 캡션 한 줄 높이를 예약해 변형이 바뀌어도 글리프가 움직이지 않는다.
//! 값은 모두 `center_state_*` 컴포넌트 토큰에서 읽으므로 UI 배율을 따른다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::{Button, ButtonVariant, Spinner};

/// CenterState 변형. 오류만 글리프 색이 다르다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CenterStateVariant {
    Loading,
    Empty,
    Error,
}

/// 블록 아래에 붙는 선택 액션(Retry·Reconnect 등). 시안의 CenterState 에는 없는
/// 호스트 요소라 글리프 위치 계산에서 제외한다.
struct CenterStateAction<'a> {
    label: &'a str,
    icon: Option<tasty_icons::Icon>,
}

/// CenterState 빌더.
pub struct CenterState<'a> {
    variant: CenterStateVariant,
    glyph: Option<tasty_icons::Icon>,
    title: &'a str,
    sub: Option<&'a str>,
    action: Option<CenterStateAction<'a>>,
}

/// 그려진 요소의 위치와 액션 클릭 여부.
#[derive(Clone, Copy, Debug)]
pub struct CenterStateOutput {
    pub glyph: egui::Rect,
    pub title: egui::Rect,
    /// 보조 줄 슬롯. 보조 줄이 없어도 캡션 한 줄 높이를 차지한다.
    pub sub_slot: egui::Rect,
    pub action_clicked: bool,
}

impl<'a> CenterState<'a> {
    /// 스피너를 글리프 자리에 그린다.
    pub fn loading(title: &'a str) -> Self {
        Self::new(CenterStateVariant::Loading, None, title)
    }

    pub fn empty(glyph: tasty_icons::Icon, title: &'a str) -> Self {
        Self::new(CenterStateVariant::Empty, Some(glyph), title)
    }

    pub fn error(glyph: tasty_icons::Icon, title: &'a str) -> Self {
        Self::new(CenterStateVariant::Error, Some(glyph), title)
    }

    fn new(variant: CenterStateVariant, glyph: Option<tasty_icons::Icon>, title: &'a str) -> Self {
        Self {
            variant,
            glyph,
            title,
            sub: None,
            action: None,
        }
    }

    /// 보조 줄. `None` 이어도 슬롯은 예약된다.
    pub fn sub_line(mut self, sub: Option<&'a str>) -> Self {
        self.sub = sub;
        self
    }

    /// 보조 줄 아래의 Secondary 버튼.
    pub fn action(mut self, label: &'a str, icon: Option<tasty_icons::Icon>) -> Self {
        self.action = Some(CenterStateAction { label, icon });
        self
    }

    /// 가용 폭 전체와 `height`(없으면 블록 자연 높이)를 할당하고 그 안에 그린다.
    pub fn show(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        height: Option<LogicalPx>,
    ) -> CenterStateOutput {
        let width = LogicalPx(ui.available_width());
        let height = height.unwrap_or_else(|| {
            let texts = self.texts(ui, theme, width);
            texts.block_h(theme) + theme.spacing_md * 2.0
        });
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(width.value(), height.value()),
            egui::Sense::hover(),
        );
        self.show_in(ui, theme, rect)
    }

    /// 부모 레이아웃에 할당하지 않고 `region` 안에 그린다.
    pub fn show_in(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        region: egui::Rect,
    ) -> CenterStateOutput {
        let pad = theme.spacing_md;
        let texts = self.texts(ui, theme, LogicalPx(region.width()));
        let glyph_size = theme.center_state_glyph_size();
        let avail_h = LogicalPx(region.height()) - pad * 2.0;
        let top = LogicalPx(region.top())
            + pad
            + (avail_h - texts.block_h(theme)).max(LogicalPx(0.0)) * 0.5;
        let cx = region.center().x;

        let glyph = egui::Rect::from_min_size(
            egui::pos2(cx - (glyph_size * 0.5).value(), top.value()),
            egui::vec2(glyph_size.value(), glyph_size.value()),
        );
        let title_top = top + glyph_size + theme.center_state_gap();
        let title = texts
            .title
            .rect
            .translate(egui::vec2(cx, title_top.value()));
        let sub_top = LogicalPx(title.bottom()) + theme.center_state_line_gap();
        let sub_slot = egui::Rect::from_min_size(
            egui::pos2(cx - (texts.max_w * 0.5).value(), sub_top.value()),
            egui::vec2(texts.max_w.value(), texts.sub_h.value()),
        );

        let clip = region.intersect(ui.clip_rect());
        let painter = ui.painter_at(clip);
        let glyph_fg = match self.variant {
            CenterStateVariant::Error => theme.center_state_error_fg(),
            CenterStateVariant::Loading | CenterStateVariant::Empty => {
                theme.center_state_glyph_fg()
            }
        }
        .to_egui();
        match (self.variant, self.glyph) {
            (CenterStateVariant::Loading, _) | (_, None) => {
                let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(glyph));
                slot.set_clip_rect(clip);
                Spinner::new()
                    .size(glyph_size.value())
                    .color(glyph_fg)
                    .show(&mut slot, theme);
            }
            (_, Some(icon)) => {
                let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(glyph));
                slot.set_clip_rect(clip);
                icon.image(glyph_size.value(), glyph_fg)
                    .paint_at(&slot, glyph);
            }
        }
        painter.galley(
            egui::pos2(cx, title_top.value()),
            texts.title,
            theme.center_state_title_fg().to_egui(),
        );
        if let Some(sub) = texts.sub {
            painter.galley(
                egui::pos2(cx, sub_top.value()),
                sub,
                theme.center_state_sub_fg().to_egui(),
            );
        }

        let mut action_clicked = false;
        if let Some(action) = self.action {
            // 버튼 위 간격은 원격 연결 시안의 열 간격 space-sm 과 marginTop space-xs 합이다.
            let action_top = sub_slot.bottom() + (theme.spacing_sm + theme.spacing_xs).value();
            let mut col = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_max(
                        egui::pos2(region.left(), action_top),
                        egui::pos2(region.right(), region.bottom().max(action_top)),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            col.set_clip_rect(clip);
            let mut button = Button::new(action.label).variant(ButtonVariant::Secondary);
            let paint_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
                if let Some(icon) = action.icon {
                    icon.image(rect.height(), c).paint_at(ui, rect);
                }
            };
            if action.icon.is_some() {
                button = button.leading_icon(&paint_icon);
            }
            action_clicked = button.show(&mut col, theme).clicked();
        }

        CenterStateOutput {
            glyph,
            title,
            sub_slot,
            action_clicked,
        }
    }

    fn texts(&self, ui: &egui::Ui, theme: &Theme, region_w: LogicalPx) -> Texts {
        let max_w = theme
            .center_state_max_width()
            .min(region_w - theme.spacing_md * 2.0)
            .max(LogicalPx(0.0));
        let title_font = egui::FontId::proportional(theme.font_size_body.value());
        let sub_font = egui::FontId::proportional(theme.font_size_caption.value());
        let layout = |text: &str, font: egui::FontId| {
            let mut job = egui::text::LayoutJob::simple(
                text.to_owned(),
                font,
                egui::Color32::PLACEHOLDER,
                max_w.value(),
            );
            job.halign = egui::Align::Center;
            ui.fonts(|f| f.layout_job(job))
        };
        let title = layout(self.title, title_font);
        let sub = self
            .sub
            .filter(|s| !s.is_empty())
            .map(|s| layout(s, sub_font.clone()));
        // 예약 높이는 실제 보조 줄과 같은 레이아웃 경로로 잰 한 줄 높이다. `row_height` 와는
        // 소수점 아래가 달라 변형 사이에 글리프가 흔들린다.
        let line_h = LogicalPx(layout(" ", sub_font).rect.height());
        let sub_h = sub
            .as_ref()
            .map_or(line_h, |g| LogicalPx(g.rect.height()).max(line_h));
        Texts {
            title,
            sub,
            sub_h,
            max_w,
        }
    }
}

struct Texts {
    title: std::sync::Arc<egui::Galley>,
    sub: Option<std::sync::Arc<egui::Galley>>,
    sub_h: LogicalPx,
    max_w: LogicalPx,
}

impl Texts {
    /// 글리프부터 보조 줄 슬롯 끝까지의 높이. 액션은 포함하지 않는다.
    fn block_h(&self, theme: &Theme) -> LogicalPx {
        theme.center_state_glyph_size()
            + theme.center_state_gap()
            + LogicalPx(self.title.rect.height())
            + theme.center_state_line_gap()
            + self.sub_h
    }
}
