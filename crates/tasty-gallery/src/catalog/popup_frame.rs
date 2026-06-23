//! Popup frame (제목바 + 콘텐츠 영역) 그리기 공통 헬퍼.
//!
//! `convert` / `approval` / `file_handler_picker` / `widgets::dialog` 가 각각
//! 인라인 중복하던 popup frame chrome — `surface0` 배경 + `surface2` border +
//! `surface1` 제목바 (28px) + 제목 텍스트 + 콘텐츠 child Ui — 을 한곳으로 통합한다.
//!
//! 28px 가운데 제목바 이디엄은 디자인 canon 이므로 값/구성은 그대로 두고 중복만 제거.
//! 색·폰트·치수는 모두 `Theme` 토큰을 사용하므로 시각 무변경 dedup.
//!
//! 콘텐츠 영역 inset 은 호출부마다 달랐으므로 `ContentInset` 으로 명시 전달한다:
//! - `INSET` (approval / dialog): 좌우 8px, 상단 +4px 추가.
//! - `FLUSH` (convert / file_handler_picker): 좌우 0, 상단 추가 0.

use tasty_type_appearance::theme::Theme;

/// 본체 popup 상수 — 제목바 높이.
pub const TITLE_BAR_HEIGHT: f32 = 28.0;
/// 본체 popup 상수 — 콘텐츠 상/하 여백.
pub const CONTENT_MARGIN: f32 = 4.0;

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

/// `total_h` 로 높이를 직접 받아 popup frame 을 그린다 (높이 계산은 호출부 책임).
///
/// `paint` 는 콘텐츠 영역에 묶인 child Ui 를 받는다.
pub fn draw(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    width: f32,
    total_h: f32,
    inset: ContentInset,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let (frame_rect, _) = ui.allocate_exact_size(egui::vec2(width, total_h), egui::Sense::hover());
    let painter = ui.painter_at(frame_rect);

    let bg: egui::Color32 = theme.surface0.into();
    let title_bg: egui::Color32 = theme.surface1.into();
    let border: egui::Color32 = theme.surface2.into();
    let text_color: egui::Color32 = theme.text.into();

    painter.rect_filled(frame_rect, theme.corner_radius.value(), bg);
    painter.rect_stroke(
        frame_rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), border),
        egui::StrokeKind::Inside,
    );

    let title_rect = egui::Rect::from_min_size(
        frame_rect.min,
        egui::vec2(frame_rect.width(), TITLE_BAR_HEIGHT),
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
    painter.text(
        egui::pos2(title_rect.min.x + 8.0, title_rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(theme.font_size_body.value()),
        text_color,
    );

    let content_top = title_rect.bottom() + CONTENT_MARGIN;
    let content_rect = egui::Rect::from_min_max(
        egui::pos2(
            frame_rect.min.x + inset.horizontal,
            content_top + inset.top_extra,
        ),
        egui::pos2(
            frame_rect.max.x - inset.horizontal,
            frame_rect.max.y - CONTENT_MARGIN,
        ),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    paint(&mut child);
}
