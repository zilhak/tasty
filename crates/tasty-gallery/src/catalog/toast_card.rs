//! Toast 카드 그리기 공통 헬퍼.
//!
//! `widgets/toast.rs` (단일 카드 데모) 와 `components/toast.rs` (스택 데모) 가
//! 각각 중복 정의하던 `ToastKind` / `accent_color` / 카드 chrome (bg rect + border +
//! accent bar + text galley) 을 한곳으로 통합한다.
//!
//! 색·폰트·치수 토큰은 모두 호출부가 `Theme` 에서 뽑아 그대로 넘기므로 시각 무변경.
//! 스택 데모는 alpha 를 미리 곱한 색을 넘기고, 단일 카드 데모는 alpha=1.0 (=곱 항등)
//! 으로 같은 헬퍼를 호출한다.
//!
//! 상수 (`PADDING_X` / `PADDING_Y` / `ACCENT_BAR_WIDTH`) 는 본체 `toast.rs` 와 동일.

use tasty_type_appearance::theme::Theme;

pub const PADDING_X: f32 = 12.0;
pub const PADDING_Y: f32 = 8.0;
pub const ACCENT_BAR_WIDTH: f32 = 4.0;

/// 본체 정본 `crates/tasty-model/src/toast_kind.rs::ToastKind` 와 **kind-for-kind
/// 동일**해야 한다 — 본체가 만들 수 없는 종류를 갤러리가 전시하면 demo=main 등가성이
/// 한 방향으로 깨진다(`docs/design/policies/gallery-completeness.md`). 정본을 그대로
/// import 하지 않는 이유는 그 크레이트가 termwiz/터미널 모델까지 끌고 오기 때문이고,
/// 본체 binary 의존 때문이 아니다.
#[derive(Clone, Copy, Debug)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

/// kind → accent 색. 본체 `toast.rs` 와 동일한 accent 매핑.
pub fn accent_color(kind: ToastKind, theme: &Theme) -> egui::Color32 {
    match kind {
        ToastKind::Info => theme.accent_primary().into(),
        ToastKind::Success => theme.accent_success().into(),
        ToastKind::Warning => theme.accent_warning().into(),
        ToastKind::Error => theme.accent_danger().into(),
    }
}

/// 카드 chrome 색 묶음 (호출부가 alpha 반영 후 최종 색을 채워 넘긴다).
#[derive(Clone, Copy)]
pub struct CardColors {
    /// 카드 배경.
    pub bg: egui::Color32,
    /// 카드 border.
    pub border: egui::Color32,
    /// 좌측 accent bar.
    pub accent: egui::Color32,
    /// galley fallback 텍스트 색 (galley 자체도 이 색으로 layout 됨).
    pub text: egui::Color32,
}

/// 토스트 카드 1장의 chrome 을 `rect` 안에 그린다.
///
/// 호출부가 (alpha 반영 후) 최종 색을 `CardColors` 로 넘긴다 — 단일 카드 데모는
/// alpha=1.0, 스택 데모는 `gamma_multiply(alpha)` 한 색을 그대로 전달. galley 의
/// 텍스트 색도 호출부가 결정해 layout 해 둔 것을 사용한다.
pub fn draw_card(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    colors: CardColors,
    galley: std::sync::Arc<egui::Galley>,
) {
    painter.rect_filled(rect, theme.corner_radius.value(), colors.bg);
    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), colors.border),
        egui::StrokeKind::Inside,
    );

    let bar_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.min.x + ACCENT_BAR_WIDTH, rect.max.y),
    );
    let bar_radius = egui::CornerRadius {
        nw: theme.corner_radius.value() as u8,
        sw: theme.corner_radius.value() as u8,
        ne: 0,
        se: 0,
    };
    painter.rect_filled(bar_rect, bar_radius, colors.accent);

    let text_pos = egui::pos2(
        rect.min.x + ACCENT_BAR_WIDTH + PADDING_X,
        rect.min.y + PADDING_Y,
    );
    painter.galley(text_pos, galley, colors.text);
}
