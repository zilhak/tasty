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
/// 본체 popup 상수 — 타이틀바 우측 버튼 한 변.
pub const TITLE_BTN_SIZE: f32 = 20.0;
/// 본체 popup 상수 — 타이틀바 우측 끝과 close 버튼 사이 여백.
pub const TITLE_BTN_EDGE_PAD: f32 = 4.0;

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
            title_rect.max.x - TITLE_BTN_SIZE * 0.5 - TITLE_BTN_EDGE_PAD,
            title_rect.center().y,
        ),
        egui::Vec2::splat(TITLE_BTN_SIZE),
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
                close_rect.center().x - TITLE_BTN_SIZE - theme.spacing_xs.value(),
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
    width: f32,
    total_h: f32,
    inset: ContentInset,
    buttons: TitleButtons,
    paint: impl FnOnce(&mut egui::Ui),
) {
    let (frame_rect, _) = ui.allocate_exact_size(egui::vec2(width, total_h), egui::Sense::hover());
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
    draw_title_buttons(&painter, theme, title_rect, buttons);

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

/// 모달 specimen 의 lift 그림자. `widgets::dialog` · `components::{file_picker,
/// remote_attach, transfer}` 넷이 각자 같은 값을 인라인 중복하던 것을 한곳으로 모은다.
///
/// **값은 그대로 둔다.** 이 값(offset 0/10 · blur 28 · black alpha 120)이
/// `Theme::shadow_popover()`(offset 0/8 · blur 24 · alpha 90 — 디자인이 승인한 단
/// 하나의 popover 그림자)와 다른 것은 **별개 판단 항목**이라 여기서 조용히 맞추지
/// 않는다. 본체 모달 렌더 경로에는 대응하는 그림자가 없어 "본체와 맞춘다" 로도
/// 결정되지 않는다. 이 헬퍼의 목적은 판단이 내려졌을 때 **고칠 자리를 한 곳으로**
/// 만들어 두는 것이다.
pub fn modal_shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: egui::Color32::from_black_alpha(120),
    }
}
