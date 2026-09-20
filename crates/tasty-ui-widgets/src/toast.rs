//! 토스트 카드 스택의 **순수 시각**.
//!
//! 본체(`ToastManager`)와 갤러리 specimen 이 같은 픽셀을 그리도록 그리기 본문을 여기
//! 하나로 둔다. 예전에는 같은 카드 chrome 이 본체와 갤러리에 각각 있었고, 값이 같아
//! 보여도 정의가 둘이면 갈릴 수 있었다.
//!
//! 이 모듈은 **시간도 상태도 안 본다.** 수명·coalesce·스코프 rect 조회는 부르는 쪽
//! (본체 `ToastManager`) 이 하고, 여기에는 이미 계산된 `alpha` 와 rect 만 들어온다.
//! 그래서 갤러리가 mock props 로 같은 함수를 그대로 부를 수 있다.
//!
//! 페이드 곡선([`fade_alpha`])만 예외적으로 여기 있다 — 곡선은 *시각* 이고, 그것을
//! 부르는 쪽이 시각과 갈리면 갤러리가 보여주는 fade 가 본체의 fade 가 아니게 된다.
//! 대신 인자는 `Duration` 둘이라 어떤 상태 타입도 안 본다.

use std::time::Duration;

use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;

use crate::tokens::{
    TOAST_ACCENT_BAR_WIDTH as ACCENT_BAR_WIDTH, TOAST_GAP,
    TOAST_MIN_INNER_WIDTH as MIN_TOAST_INNER_WIDTH, TOAST_MIN_MAX_WIDTH,
    TOAST_PADDING_X as PADDING_X, TOAST_PADDING_Y as PADDING_Y, TOAST_SCOPE_MARGIN as SCOPE_MARGIN,
};

/// 등장 페이드 시간(ms).
pub const FADE_IN_MS: f32 = 80.0;
/// 소멸 페이드 시간(ms). 부르는 쪽이 "언제까지 살려 둘지" 를 이 값으로 정하므로 공개한다.
pub const FADE_OUT_MS: f32 = 160.0;

/// 그릴 준비가 끝난 토스트 1 개의 시각 데이터.
///
/// `alpha` 는 부르는 쪽이 [`fade_alpha`] 로 미리 계산한다 — 이 모듈은 시간을 안 본다.
#[derive(Clone, Debug)]
pub struct ToastEntryView {
    pub kind: ToastKind,
    pub message: String,
    /// [0.0, 1.0] — 0 이면 스킵.
    pub alpha: f32,
}

/// 한 scope 의 토스트 그룹.
///
/// `entries` 는 *발사 순서* (id 오름차순) 로 정렬돼 있어야 한다 — 그리는 쪽이 그대로
/// 우측 하단부터 위로 쌓는다.
#[derive(Clone, Debug)]
pub struct ToastScopeView {
    pub scope_rect: egui::Rect,
    pub entries: Vec<ToastEntryView>,
}

/// 전체 scope 의 그룹 리스트 + theme.
pub struct ToastViewProps<'a> {
    pub theme: &'a Theme,
    pub scopes: &'a [ToastScopeView],
}

/// kind → 좌측 accent 바 색.
pub fn accent_color(kind: ToastKind, th: &Theme) -> egui::Color32 {
    match kind {
        ToastKind::Info => th.accent_primary().into(),
        ToastKind::Success => th.accent_success().into(),
        ToastKind::Warning => th.accent_warning().into(),
        ToastKind::Error => th.accent_danger().into(),
    }
}

/// 페이드 인/아웃 알파.
///
/// `age` 는 발사 시점으로부터 흐른 시간, `lifetime` 은 그 토스트의 수명이다. 상태
/// 타입을 안 받는 이유는 이 크레이트가 도메인 모델을 모르기 때문이다 — 부르는 쪽이
/// 자기 상태에서 두 `Duration` 을 뽑아 넘긴다.
pub fn fade_alpha(age: Duration, lifetime: Duration, reduced_motion: bool) -> f32 {
    let age_ms = age.as_secs_f32() * 1000.0;
    let life_ms = lifetime.as_secs_f32() * 1000.0;

    if reduced_motion {
        return if age_ms < life_ms { 1.0 } else { 0.0 };
    }

    if age_ms < FADE_IN_MS {
        (age_ms / FADE_IN_MS).clamp(0.0, 1.0)
    } else if age_ms < life_ms {
        1.0
    } else {
        let fade_out = (age_ms - life_ms) / FADE_OUT_MS;
        (1.0 - fade_out).clamp(0.0, 1.0)
    }
}

/// 토스트 스택을 `painter` 에 그린다.
///
/// `painter` 를 받는 이유는 **떠오르는 자리가 부르는 쪽마다 다르기 때문**이다 — 본체는
/// 다른 UI 위에 떠야 하므로 `LayerId(Order::Tooltip, …)` 의 layer painter 를 넘기고,
/// 갤러리 specimen 은 무대 frame 안에 그려야 하므로 `ui.painter_at(rect)` 를 넘긴다.
/// 폰트는 `painter.ctx()` 에서 가져오므로 따로 `Context` 를 받지 않는다.
///
/// 반환값 없음 — 토스트는 사용자 입력을 받지 않으며(auto-dismiss) action 도 없다.
pub fn draw_toast_scopes(painter: &egui::Painter, props: &ToastViewProps<'_>) {
    let th = props.theme;
    let ctx = painter.ctx().clone();

    for scope in props.scopes {
        let scope_rect = scope.scope_rect;
        // 스코프 경계로 클립 — 폭/세로 클램프 후에도 1px 단위로 새는 것을 막는
        // 안전망. 토스트는 자기 스코프 영역 안에 머물러야 한다(이웃 pane/탭바를
        // 덮지 않음).
        let painter = painter.with_clip_rect(scope_rect);
        let mut cursor_y = scope_rect.max.y - SCOPE_MARGIN;

        // 새것부터 그리며 위로 올라간다 (id 오름차순으로 받았으므로 reverse).
        for entry in scope.entries.iter().rev() {
            let alpha = entry.alpha;
            if alpha <= 0.0 {
                continue;
            }

            // 좁은 surface 에서 토스트가 좌측 경계를 넘지 않도록 max_width 를 surface
            // 안쪽 폭(width - 2*margin)으로 클램프한다. 정상 폭 surface 에서는
            // 0.8*width < width-2*margin (width>120) 이라 0.8 폭이 그대로 — 시각
            // 무변경이고, 좁은 surface 에서만 클램프가 발동한다.
            let inner_limit = (scope_rect.width() - SCOPE_MARGIN * 2.0).max(MIN_TOAST_INNER_WIDTH);
            let max_width = (scope_rect.width() * 0.8)
                .max(TOAST_MIN_MAX_WIDTH)
                .min(inner_limit);
            let font = egui::FontId::proportional(th.font_size_body.value());
            // wrap_width 음수 방지(클램프로 max_width 가 작아질 때).
            let wrap_width = (max_width - PADDING_X * 2.0 - ACCENT_BAR_WIDTH).max(1.0);

            // ★ layout 색에 alpha 를 곱하지 않는다 — 본체가 지금 그렇게 그린다.
            // `Fonts::layout` 에 색을 명시하면 그 색이 galley 에 박히고
            // `Painter::galley` 의 fallback 은 `Color32::PLACEHOLDER` 구간에만 쓰이므로
            // **본문 글자는 페이드하지 않고 카드 chrome 만 페이드한다.** 갤러리
            // specimen 은 여기서 갈려 있었다(글자까지 흐려졌다). 경계를 옮기면서
            // 시각을 바꾸지 않으려고 본체 쪽을 그대로 둔다 — 어느 쪽이 옳은지는
            // 디자인이 정할 값이다.
            let galley = ctx.fonts(|f| {
                f.layout(
                    entry.message.clone(),
                    font.clone(),
                    th.text_primary().into(),
                    wrap_width,
                )
            });

            let toast_w = (galley.size().x + PADDING_X * 2.0 + ACCENT_BAR_WIDTH).min(max_width);
            let toast_h = galley.size().y + PADDING_Y * 2.0;

            let max_x = scope_rect.max.x - SCOPE_MARGIN;
            let bottom_y = cursor_y;
            let top_y = bottom_y - toast_h;
            // 스택이 scope 상단을 넘으면 더 오래된(위쪽) 토스트는 그리지 않는다.
            // 새것부터 그리므로(reverse) break 가 곧 "넘치는 옛것 생략". 단일
            // 토스트가 scope 보다 높은 극단은 위의 clip 이 처리한다.
            if top_y < scope_rect.min.y {
                break;
            }
            let left_x = max_x - toast_w;

            let rect =
                egui::Rect::from_min_max(egui::pos2(left_x, top_y), egui::pos2(max_x, bottom_y));

            let bg = th.surface_raised().gamma_multiply(alpha);
            // toast 보더 — canonical `toast-border`.
            let border = th.toast_border().gamma_multiply(alpha);
            let accent = accent_color(entry.kind, th).gamma_multiply(alpha);

            draw_card(
                &painter,
                th,
                rect,
                CardColors {
                    bg: bg.into(),
                    border: border.into(),
                    accent,
                    text: th.text_primary().gamma_multiply(alpha).into(),
                },
                galley,
            );

            cursor_y = top_y - TOAST_GAP;
        }
    }
}

/// 카드 chrome 색 묶음 (부르는 쪽이 alpha 반영 후 최종 색을 채워 넘긴다).
#[derive(Clone, Copy)]
pub struct CardColors {
    /// 카드 배경.
    pub bg: egui::Color32,
    /// 카드 border.
    pub border: egui::Color32,
    /// 좌측 accent bar.
    pub accent: egui::Color32,
    /// galley 텍스트 색 (galley 자체도 이 색으로 layout 돼 있어야 한다).
    pub text: egui::Color32,
}

/// 토스트 카드 1 장의 chrome 을 `rect` 안에 그린다.
///
/// 스택을 안 쓰고 카드 한 장만 보여야 하는 자리(갤러리의 단일 카드 specimen 등)가
/// 있어 따로 공개한다.
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
