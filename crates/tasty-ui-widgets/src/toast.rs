//! 본체와 갤러리가 공유하는 토스트 카드 그리기와 페이드 계산.
//! 수명 관리·중복 합치기·범위 조회는 호출자가 맡고 계산된 불투명도와 영역을 전달한다.

use std::time::Duration;

use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;

use crate::chip::{KbdKey, kbd_parts_width, kbd_text_parts_painted};
use crate::tokens::{
    TOAST_GAP, TOAST_HINT_GAP as HINT_GAP, TOAST_MIN_INNER_WIDTH as MIN_TOAST_INNER_WIDTH,
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
    /// 알림을 낸 동작의 단축키 키캡(표시 문자열). 비어 있으면 hint를 그리지 않는다.
    /// 메뉴·마우스로 실행했고 binding이 있을 때만 호출자가 채운다.
    pub hint: Vec<String>,
    /// [0.0, 1.0] — 0 이면 스킵.
    pub alpha: f32,
}

/// 스택 맨 아래 카드를 스코프 하단에서 띄우는 기준.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastStackBottom {
    /// pane·surface처럼 화면 일부를 덮는 스코프. 가장자리 여백(space-md)만 둔다.
    #[default]
    ScopeMargin,
    /// 창 전체를 덮는 스코프. 창 하단에서 `toast-stack-offset-bottom`만큼 띄워 상태바 위에 쌓는다.
    Window,
}

/// 생성 순서(ID 오름차순)의 토스트 목록. 역순으로 그려 최신 항목을 오른쪽 아래에 놓는다.
#[derive(Clone, Debug)]
pub struct ToastScopeView {
    pub scope_rect: egui::Rect,
    pub bottom: ToastStackBottom,
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
        let bottom_offset = match scope.bottom {
            ToastStackBottom::ScopeMargin => SCOPE_MARGIN,
            ToastStackBottom::Window => th.toast_stack_offset_bottom().value(),
        };
        let mut cursor_y = scope_rect.max.y - bottom_offset;

        // 새것부터 그리며 위로 올라간다 (id 오름차순으로 받았으므로 reverse).
        for entry in scope.entries.iter().rev() {
            let alpha = entry.alpha;
            if alpha <= 0.0 {
                continue;
            }

            // 카드마다 내용 폭을 쓰고 상한은 `toast_max_width`다. 공유 스택 폭이 없어 카드가
            // 들고 나도 다른 카드가 다시 흐르지 않는다. 좁은 영역에서는 왼쪽 여백도 넘지 않는다.
            let inner_limit = (scope_rect.width() - SCOPE_MARGIN * 2.0).max(MIN_TOAST_INNER_WIDTH);
            let max_width = th.toast_max_width.value().min(inner_limit);
            let (galley, size) =
                layout_card(&ctx, th, entry.message.clone(), &entry.hint, max_width);
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
                &entry.hint,
                alpha,
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

/// hint 키캡이 차지하는 폭과 본문 사이 간격의 합. hint가 없으면 0이다.
fn hint_reserve(ctx: &egui::Context, theme: &Theme, hint: &[String]) -> f32 {
    if hint.is_empty() {
        return 0.0;
    }
    let keys: Vec<KbdKey<'_>> = hint.iter().map(|k| KbdKey::Text(k)).collect();
    kbd_parts_width(ctx, theme, &keys).value() + HINT_GAP
}

/// 카드 폭 상한에 맞춰 본문과 크기를 계산한다. 상한은 호출자가 정한다.
/// hint는 줄지 않으므로 본문 줄바꿈 폭에서 먼저 뺀다(본문이 먼저 줄바꿈된다).
/// 본문은 고정색으로 배치해 페이드하지 않고 배경·테두리·강조 막대만 페이드한다.
pub fn layout_card(
    ctx: &egui::Context,
    theme: &Theme,
    message: String,
    hint: &[String],
    max_width: f32,
) -> (std::sync::Arc<egui::Galley>, egui::Vec2) {
    let font = egui::FontId::proportional(theme.font_size_body.value());
    let accent_w = theme.toast_accent_width.value();
    let reserve = hint_reserve(ctx, theme, hint);
    // wrap_width 음수 방지(스코프 클램프로 max_width 가 작아질 때).
    let wrap_width = (max_width - PADDING_X * 2.0 - accent_w - reserve).max(1.0);
    let galley = ctx.fonts(|f| f.layout(message, font, theme.text_primary().into(), wrap_width));
    let toast_w = (galley.size().x + PADDING_X * 2.0 + accent_w + reserve).min(max_width);
    let hint_h = if hint.is_empty() {
        0.0
    } else {
        theme.kbd_size().value()
    };
    let toast_h = galley.size().y.max(hint_h) + PADDING_Y * 2.0;
    (galley, egui::vec2(toast_w, toast_h))
}

/// 스코프 없이 단일 카드를 그린다. 본체 스택과 같은 크기·색 계산을 사용한다.
pub fn draw_single_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    kind: ToastKind,
    message: &str,
    hint: &[String],
    alpha: f32,
) -> egui::Response {
    let (galley, size) = layout_card(
        ui.ctx(),
        theme,
        message.to_string(),
        hint,
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
        hint,
        alpha,
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

/// 지정된 사각형 안에 카드의 배경·테두리·강조 막대와 본문, hint 키캡을 그린다.
/// hint는 오른쪽 끝에서 본문 첫 줄의 세로 중심에 맞춘다. 키캡 색에도 `alpha`를 곱한다.
pub fn draw_card(
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    colors: CardColors,
    galley: std::sync::Arc<egui::Galley>,
    hint: &[String],
    alpha: f32,
) {
    painter.rect_filled(rect, theme.corner_radius.value(), colors.bg);
    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), colors.border),
        egui::StrokeKind::Inside,
    );

    let accent_w = theme.toast_accent_width.value();
    let bar_rect =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + accent_w, rect.max.y));
    let bar_radius = egui::CornerRadius {
        nw: theme.corner_radius.value() as u8,
        sw: theme.corner_radius.value() as u8,
        ne: 0,
        se: 0,
    };
    painter.rect_filled(bar_rect, bar_radius, colors.accent);

    let text_pos = egui::pos2(rect.min.x + accent_w + PADDING_X, rect.min.y + PADDING_Y);
    if !hint.is_empty() {
        let first_line_center = galley
            .rows
            .first()
            .map_or(galley.size().y * 0.5, |row| row.rect.center().y);
        let keys: Vec<&str> = hint.iter().map(String::as_str).collect();
        kbd_text_parts_painted(
            painter,
            theme,
            &keys,
            rect.max.x - PADDING_X,
            text_pos.y + first_line_center,
            alpha,
        );
    }
    painter.galley(text_pos, galley, colors.text);
}
