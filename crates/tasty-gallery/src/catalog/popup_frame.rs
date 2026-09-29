//! 제목바와 콘텐츠 영역을 그리는 공용 팝업 프레임.
//! 콘텐츠 여백과 그림자는 호출자가 팝업 종류에 맞게 지정한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{elide_popup_title, paint_popup_title_glyph, popup_title_text_rect};

use crate::catalog::icons;

/// 본체 popup 상수 — 제목바 높이.
pub const TITLE_BAR_HEIGHT: LogicalPx = LogicalPx(28.0);
/// 본체 popup 콘텐츠의 위아래 여백. 본체 Theme.spacing_xs와 같은 토큰을 읽는다.
/// 갤러리는 Theme 치수에 배율을 곱하지 않고 egui 전역 배율을 사용한다.
pub const CONTENT_MARGIN: LogicalPx = tasty_design_tokens::generated::semantic::SPACE_XS;

/// 타이틀바 우측 버튼 세트. 본체 `PopupManager` 구성과 같다 — close(X) 는 타이틀바가
/// 있는 모든 popup 에, fullscreen 은 **전체화면 무대를 선언한 popup** 에만 붙는다
/// (`PopupDef.fullscreen_stage`).
#[derive(Clone, Copy, Default)]
pub struct TitleButtons {
    /// close 왼쪽의 전체화면 버튼(디자인 `fit` 글리프).
    pub fullscreen: bool,
    /// 타이틀바 우측 끝의 X.
    pub close: bool,
}

impl TitleButtons {
    /// 버튼 없음 — 이 헬퍼를 쓰는 기존 popup frame 데모의 기본값.
    pub const NONE: Self = Self {
        fullscreen: false,
        close: false,
    };
    /// X 만.
    pub const CLOSE: Self = Self {
        fullscreen: false,
        close: true,
    };
    /// 전체화면 + X — 무대를 선언한 popup(현재 `notifications`).
    pub const FULLSCREEN_AND_CLOSE: Self = Self {
        fullscreen: true,
        close: true,
    };
}

/// 타이틀바 제목을 본체와 같은 대칭 영역 계산으로 스트립 가운데에 그린다. 넘치면 본체와
/// 같이 말줄임한다.
pub fn draw_title_text(
    painter: &egui::Painter,
    theme: &Theme,
    title_rect: egui::Rect,
    buttons_left_x: f32,
    title: &str,
    font: egui::FontId,
) {
    let area = popup_title_text_rect(title_rect, buttons_left_x, theme.popup_title_text_gap());
    let shown = elide_popup_title(painter.ctx(), title, font.clone(), area.width());
    painter.with_clip_rect(area).text(
        area.center(),
        egui::Align2::CENTER_CENTER,
        shown,
        font,
        theme.text_primary().into(),
    );
}

/// 타이틀바 버튼을 painter로 그리고 버튼 영역의 왼쪽 끝을 반환한다. 본체 `PopupManager`와
/// 같은 토큰(`popup-title-btn-size`·`-btn-gap`·`-edge-inset`)으로 배치한다.
/// 제목은 이 경계를 넘지 않도록 줄여야 한다.
pub fn draw_title_buttons(
    ctx: &egui::Context,
    painter: &egui::Painter,
    theme: &Theme,
    title_rect: egui::Rect,
    buttons: TitleButtons,
) -> f32 {
    let fg: egui::Color32 = theme.text_muted().into();
    let size = theme.popup_title_btn_size();
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(
            title_rect.max.x - (size.scaled(0.5) + theme.popup_title_edge_inset()).value(),
            title_rect.center().y,
        ),
        egui::Vec2::splat(size.value()),
    );
    let mut left = title_rect.max.x;
    if buttons.close {
        let c = close_rect.center();
        let x = 5.0;
        let stroke = egui::Stroke::new(theme.icon_stroke_width.value(), fg);
        painter.line_segment([c - egui::vec2(x, x), c + egui::vec2(x, x)], stroke);
        painter.line_segment([c + egui::vec2(-x, x), c + egui::vec2(x, -x)], stroke);
        left = close_rect.min.x;
    }
    if buttons.fullscreen {
        // close 왼쪽, popup-title-btn-gap 간격. 글리프는 IconButton sm 과 같은 크기의 `fit`.
        let rect = egui::Rect::from_center_size(
            egui::pos2(
                close_rect.center().x - (size + theme.popup_title_btn_gap()).value(),
                close_rect.center().y,
            ),
            close_rect.size(),
        );
        let glyph = theme.icon_glyph_size_sm.value();
        let glyph_rect = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(glyph));
        if let Err(err) = paint_popup_title_glyph(ctx, painter, icons::FIT, glyph_rect, fg) {
            tracing::warn!("gallery popup fit glyph failed to load: {err}");
        }
        left = rect.min.x;
    }
    left
}

/// 콘텐츠 영역 inset (호출부별 차이를 명시).
#[derive(Clone, Copy)]
pub struct ContentInset {
    /// 좌우 가로 inset (px).
    pub horizontal: f32,
    /// 제목바 아래 콘텐츠 상단에 추가로 더하는 여백 (px).
    pub top_extra: f32,
}

impl ContentInset {
    /// approval / dialog 변형 — 좌우 8px, 상단 +4px.
    pub const INSET: Self = Self {
        horizontal: 8.0,
        top_extra: 4.0,
    };
    /// convert / file_handler_picker 변형 — 좌우 flush, 상단 추가 없음.
    pub const FLUSH: Self = Self {
        horizontal: 0.0,
        top_extra: 0.0,
    };
}

/// 호출자가 지정한 높이·여백·그림자로 프레임을 그린다.
/// paint는 콘텐츠 영역의 child Ui를 받는다. 알림창처럼 그림자가 없는 종류도 허용한다.
#[allow(clippy::too_many_arguments)] // reason: popup frame 은 chrome 파라미터가 본래 많다.
pub fn draw(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    width: LogicalPx,
    total_h: LogicalPx,
    inset: ContentInset,
    buttons: TitleButtons,
    shadow: Option<tasty_type_appearance::theme::ShadowToken>,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let (frame_rect, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), total_h.value()),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(frame_rect);

    let bg: egui::Color32 = theme.surface_raised().into();
    let title_bg: egui::Color32 = theme.surface_hover().into();
    let border: egui::Color32 = theme.border_frame().into();

    if let Some(shadow) = shadow {
        painter.add(
            shadow
                .to_egui()
                .as_shape(frame_rect, theme.corner_radius.value()),
        );
    }
    painter.rect_filled(frame_rect, theme.corner_radius.value(), bg);
    painter.rect_stroke(
        frame_rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), border),
        egui::StrokeKind::Inside,
    );

    let title_rect = egui::Rect::from_min_size(
        frame_rect.min,
        egui::vec2(frame_rect.width(), TITLE_BAR_HEIGHT.value()),
    );
    painter.rect_filled(
        title_rect,
        egui::CornerRadius {
            nw: theme.corner_radius.value() as u8,
            ne: theme.corner_radius.value() as u8,
            sw: 0,
            se: 0,
        },
        title_bg,
    );
    let buttons_left = draw_title_buttons(ui.ctx(), &painter, theme, title_rect, buttons);
    draw_title_text(
        &painter,
        theme,
        title_rect,
        buttons_left,
        title,
        egui::FontId::proportional(theme.font_size_body.value()),
    );

    let content_top = LogicalPx(title_rect.bottom()) + CONTENT_MARGIN;
    let content_rect = egui::Rect::from_min_max(
        egui::pos2(
            frame_rect.min.x + inset.horizontal,
            content_top.value() + inset.top_extra,
        ),
        egui::pos2(
            frame_rect.max.x - inset.horizontal,
            frame_rect.max.y - CONTENT_MARGIN.value(),
        ),
    );
    // 상위 가로 레이아웃을 상속하지 않도록 콘텐츠의 세로 방향을 명시한다.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    paint(&mut child);
}
