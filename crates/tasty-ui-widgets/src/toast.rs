//! 본체와 갤러리가 공유하는 토스트 카드 그리기와 페이드 계산.
//! 수명 관리·중복 합치기·범위 조회는 호출자가 맡고 계산된 불투명도와 영역을 전달한다.

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

/// 그릴 토스트의 데이터. 불투명도는 호출자가 미리 계산한다.
#[derive(Clone, Debug)]
pub struct ToastEntryView {
    pub kind: ToastKind,
    pub message: String,
    /// [0.0, 1.0] — 0 이면 스킵.
    pub alpha: f32,
}

/// 생성 순서(ID 오름차순)의 토스트 목록. 역순으로 그려 최신 항목을 오른쪽 아래에 놓는다.
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

/// 생성 후 경과 시간과 수명으로 페이드 불투명도를 계산한다.
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

/// 호출자가 선택한 레이어의 painter에 토스트를 그린다. 사용자 입력이나 동작은 처리하지 않는다.
pub fn draw_toast_scopes(painter: &egui::Painter, props: &ToastViewProps<'_>) {
    let th = props.theme;
    let ctx = painter.ctx().clone();

    for scope in props.scopes {
        let scope_rect = scope.scope_rect;
        // 토스트가 이웃 영역을 덮지 않도록 스코프 경계로 자른다.
        let painter = painter.with_clip_rect(scope_rect);
        let mut cursor_y = scope_rect.max.y - SCOPE_MARGIN;

        // 새것부터 그리며 위로 올라간다 (id 오름차순으로 받았으므로 reverse).
        for entry in scope.entries.iter().rev() {
            let alpha = entry.alpha;
            if alpha <= 0.0 {
                continue;
            }

            // 좁은 영역에서도 왼쪽 여백을 넘지 않도록 카드 폭을 제한한다.
            let inner_limit = (scope_rect.width() - SCOPE_MARGIN * 2.0).max(MIN_TOAST_INNER_WIDTH);
            let max_width = (scope_rect.width() * 0.8)
                .max(TOAST_MIN_MAX_WIDTH)
                .min(inner_limit);
            let (galley, size) = layout_card(&ctx, th, entry.message.clone(), max_width);
            let (toast_w, toast_h) = (size.x, size.y);

            let max_x = scope_rect.max.x - SCOPE_MARGIN;
            let bottom_y = cursor_y;
            let top_y = bottom_y - toast_h;
            // 위쪽 경계를 넘는 카드부터는 그리지 않는다.
            if top_y < scope_rect.min.y {
                break;
            }
            let left_x = max_x - toast_w;

            let rect =
                egui::Rect::from_min_max(egui::pos2(left_x, top_y), egui::pos2(max_x, bottom_y));

            draw_card(
                &painter,
                th,
                rect,
                card_colors(th, entry.kind, alpha),
                galley,
            );

            cursor_y = top_y - TOAST_GAP;
        }
    }
}

/// 배경·테두리·본문색은 alpha를 적용한 뒤 egui 색으로 변환한다. 강조색은 변환 후 곱한다.
/// 변환 뒤 Color32::gamma_multiply를 적용하면 연산 공간이 달라 결과가 달라질 수 있다.
pub fn card_colors(theme: &Theme, kind: ToastKind, alpha: f32) -> CardColors {
    CardColors {
        bg: theme.surface_raised().gamma_multiply(alpha).into(),
        // toast 보더 — canonical `toast-border`.
        border: theme.toast_border().gamma_multiply(alpha).into(),
        accent: accent_color(kind, theme).gamma_multiply(alpha),
        text: theme.text_primary().gamma_multiply(alpha).into(),
    }
}

/// 카드 폭 상한에 맞춰 본문과 크기를 계산한다. 상한은 호출자가 정한다.
/// 본문은 고정색으로 배치해 페이드하지 않고 배경·테두리·강조 막대만 페이드한다.
pub fn layout_card(
    ctx: &egui::Context,
    theme: &Theme,
    message: String,
    max_width: f32,
) -> (std::sync::Arc<egui::Galley>, egui::Vec2) {
    let font = egui::FontId::proportional(theme.font_size_body.value());
    // wrap_width 음수 방지(스코프 클램프로 max_width 가 작아질 때).
    let wrap_width = (max_width - PADDING_X * 2.0 - ACCENT_BAR_WIDTH).max(1.0);
    let galley = ctx.fonts(|f| f.layout(message, font, theme.text_primary().into(), wrap_width));
    let toast_w = (galley.size().x + PADDING_X * 2.0 + ACCENT_BAR_WIDTH).min(max_width);
    let toast_h = galley.size().y + PADDING_Y * 2.0;
    (galley, egui::vec2(toast_w, toast_h))
}

/// 스코프 없이 단일 카드를 그린다. 본체 스택과 같은 크기·색 계산을 사용한다.
pub fn draw_single_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    kind: ToastKind,
    message: &str,
    alpha: f32,
) -> egui::Response {
    let (galley, size) = layout_card(
        ui.ctx(),
        theme,
        message.to_string(),
        theme.toast_max_width.value(),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    draw_card(
        &painter,
        theme,
        rect,
        card_colors(theme, kind, alpha),
        galley,
    );
    response
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

/// 지정된 사각형 안에 카드의 배경·테두리·강조 막대와 본문을 그린다.
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
