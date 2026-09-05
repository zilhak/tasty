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
use tasty_type_geometry::length::LogicalPx;

/// 본체 popup 상수 — 제목바 높이.
pub const TITLE_BAR_HEIGHT: LogicalPx = LogicalPx(28.0);
/// 본체 popup 상수 — 콘텐츠 상/하 여백.
///
/// 본체(`adapters::ui::popup::content_margin`)는 이 자리를 `Theme.spacing_xs` 에서
/// 읽는다. 여기서는 같은 값을 그 토큰의 정본(`semantic.space-xs` = `primitive.size-4`)
/// 에서 직접 가져온다 — 갤러리의 `Theme` 은 `with_colors` 로만 만들어져 zoom 재굽기를
/// 거치지 않으므로(스케일 세그는 egui `set_zoom_factor` 쪽이다) 두 경로의 값이 같고,
/// 리터럴 사본을 둘 이유가 없다.
pub const CONTENT_MARGIN: LogicalPx = tasty_design_tokens::generated::semantic::SPACE_XS;
/// 본체 popup 상수 — 타이틀바 우측 버튼 한 변.
pub const TITLE_BTN_SIZE: LogicalPx = LogicalPx(20.0);
/// 본체 popup 상수 — 타이틀바 우측 끝과 close 버튼 사이 여백.
pub const TITLE_BTN_EDGE_PAD: LogicalPx = LogicalPx(4.0);

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

/// 타이틀바 우측 버튼군을 그린다. 본체가 `ctx.layer_painter` 하나로 타이틀바 전체를
/// 그리므로(그 구간엔 `Ui` 가 없다) 두 글리프 모두 painter 직선이다 — 형상은
/// canonical `close`/`fit` 글리프와 같고, SVG `Image` 를 쓸 수 없을 뿐이다.
///
/// 반환값은 제목 텍스트가 침범하면 안 되는 **버튼군 좌변** — 제목 elide 가용 폭의
/// 기준이다(본체 `PopupState::title_buttons_left_x`).
pub fn draw_title_buttons(
    painter: &egui::Painter,
    theme: &Theme,
    title_rect: egui::Rect,
    buttons: TitleButtons,
) -> f32 {
    let fg: egui::Color32 = theme.text_muted().into();
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(
            title_rect.max.x - (TITLE_BTN_SIZE.scaled(0.5) + TITLE_BTN_EDGE_PAD).value(),
            title_rect.center().y,
        ),
        egui::Vec2::splat(TITLE_BTN_SIZE.value()),
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
        // close 왼쪽, 4px(space-xs) 간격.
        let rect = egui::Rect::from_center_size(
            egui::pos2(
                close_rect.center().x - (TITLE_BTN_SIZE + theme.spacing_xs).value(),
                close_rect.center().y,
            ),
            close_rect.size(),
        );
        // 디자인 `fit` — 24 viewBox 안 브래킷 사각형 18, 팔 길이 5.
        let g = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(rect.width() * 0.6));
        let arm = g.width() * (5.0 / 18.0);
        let stroke = egui::Stroke::new(theme.icon_stroke_width.value(), fg);
        for (corner, dx, dy) in [
            (g.left_top(), 1.0, 1.0),
            (g.right_top(), -1.0, 1.0),
            (g.left_bottom(), 1.0, -1.0),
            (g.right_bottom(), -1.0, -1.0),
        ] {
            painter.line_segment([corner, corner + egui::vec2(arm * dx, 0.0)], stroke);
            painter.line_segment([corner, corner + egui::vec2(0.0, arm * dy)], stroke);
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

/// `total_h` 로 높이를 직접 받아 popup frame 을 그린다 (높이 계산은 호출부 책임).
///
/// `paint` 는 콘텐츠 영역에 묶인 child Ui 를 받는다.
#[allow(clippy::too_many_arguments)] // reason: popup frame 은 chrome 파라미터가 본래 많다.
pub fn draw(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    width: LogicalPx,
    total_h: LogicalPx,
    inset: ContentInset,
    buttons: TitleButtons,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let (frame_rect, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), total_h.value()),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(frame_rect);

    let bg: egui::Color32 = theme.surface_raised().into();
    // 타이틀바 배경 채움 — 값-동일 surface_hover()(=surface1).
    let title_bg: egui::Color32 = theme.surface_hover().into();
    // divergence: popup 보더에 surface2 — border-role 전용 토큰 부재, 값-동일 surface_active().
    let border: egui::Color32 = theme.surface_active().into();
    let text_color: egui::Color32 = theme.text_primary().into();

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
    painter.text(
        egui::pos2(title_rect.min.x + 8.0, title_rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(theme.font_size_body.value()),
        text_color,
    );
    draw_title_buttons(&painter, theme, title_rect, buttons);

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
    // 콘텐츠는 항상 세로 스택이다(본체 popup 콘텐츠와 동일). `new_child` 는 부모 Ui 의
    // 레이아웃을 상속하므로, 호출부가 가로 컨텍스트(`cluster` 의 horizontal_wrapped)면
    // 세로 스택이 가로로 흐르고 세로 중앙 정렬까지 걸린다 — 레이아웃을 명시해 끊는다.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    paint(&mut child);
}
