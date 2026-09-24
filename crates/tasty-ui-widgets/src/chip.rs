//! 상호작용 없는 태그·배지·키캡. Theme의 컴포넌트 토큰으로 크기와 색을 정한다.
//! 글꼴 굵기는 별도 계열을 등록하지 않아 재현하지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// Tag variant (디자인 `core/Tag`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TagVariant {
    /// 외곽선 chip (기본) — surface-raised + border-default + text-secondary.
    Default,
    Accent,
    Agent,
    /// 투명 배경의 정보 태그. git-viewer에서 사용한다.
    Info,
    Success,
    Warning,
    Danger,
    /// 원격 미러 워크스페이스용 채운 태그. 투명 배경의 Info와 구분한다.
    Remote,
}

/// Badge variant (디자인 `core/Badge`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BadgeVariant {
    /// 채움 danger (기본 — unread count).
    Danger,
    Primary,
    Agent,
    Success,
    Neutral,
}

fn mono(size: f32) -> egui::FontId {
    egui::FontId::monospace(size)
}

/// 상태 점 없는 태그의 폭을 계산한다. 그리기 전 가용 폭과 비교할 때 사용한다.
pub fn tag_width(ui: &egui::Ui, theme: &Theme, label: &str) -> f32 {
    let pad_x = theme.tag_padding_x().value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        mono(theme.tag_font_size().value()),
        egui::Color32::PLACEHOLDER,
    );
    galley.rect.width() + 2.0 * pad_x
}

/// Tag — 모노 라벨 chip. `dot` 이 true 면 선행 상태 점(현재 fg 색).
pub fn tag(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    variant: TagVariant,
    dot: bool,
) -> egui::Response {
    // 테두리의 공통 비율은 Theme에서 읽고 대응 토큰이 없는 두 비율만 아래에 둔다.
    const TAG_BORDER_OPACITY: f32 = 0.4;
    const TAG_REMOTE_FILL_OPACITY: f32 = 0.16;
    let (fill, border, fg) = match variant {
        TagVariant::Default => (
            theme.tag_bg().to_egui(),
            Some(theme.tag_border().to_egui()),
            theme.tag_fg().to_egui(),
        ),
        TagVariant::Accent => (
            theme.accent_primary().to_egui(),
            None,
            theme.text_on_accent().to_egui(),
        ),
        TagVariant::Agent => (
            theme.accent_agent().to_egui(),
            None,
            theme.text_on_accent().to_egui(),
        ),
        TagVariant::Info => (
            egui::Color32::TRANSPARENT,
            Some(
                theme
                    .accent_info()
                    .to_egui()
                    .gamma_multiply(TAG_BORDER_OPACITY),
            ),
            theme.accent_info().to_egui(),
        ),
        TagVariant::Remote => (
            theme
                .accent_remote()
                .to_egui()
                .gamma_multiply(TAG_REMOTE_FILL_OPACITY),
            Some(
                theme
                    .accent_remote()
                    .to_egui()
                    .gamma_multiply(theme.tint_border_alpha()),
            ),
            theme.accent_remote().to_egui(),
        ),
        TagVariant::Success => (
            egui::Color32::TRANSPARENT,
            Some(
                theme
                    .accent_success()
                    .to_egui()
                    .gamma_multiply(TAG_BORDER_OPACITY),
            ),
            theme.accent_success().to_egui(),
        ),
        TagVariant::Warning => (
            egui::Color32::TRANSPARENT,
            Some(
                theme
                    .accent_warning()
                    .to_egui()
                    .gamma_multiply(TAG_BORDER_OPACITY),
            ),
            theme.accent_warning().to_egui(),
        ),
        TagVariant::Danger => (
            egui::Color32::TRANSPARENT,
            Some(
                theme
                    .accent_danger()
                    .to_egui()
                    .gamma_multiply(TAG_BORDER_OPACITY),
            ),
            theme.accent_danger().to_egui(),
        ),
    };
    let radius = theme.tag_radius().value();
    let bw = theme.border_width.value();
    let pad_x = theme.tag_padding_x().value();
    let gap = theme.tag_gap().value();
    let dot_sz = theme.tag_dot_size().value();
    let tag_h = theme.tag_size().value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        mono(theme.tag_font_size().value()),
        egui::Color32::PLACEHOLDER,
    );
    let dot_w = if dot { dot_sz + gap } else { 0.0 };
    let w = galley.rect.width() + dot_w + 2.0 * pad_x;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, tag_h), egui::Sense::hover());
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, radius, fill);
    }
    if let Some(bc) = border {
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(bw, bc),
            egui::StrokeKind::Inside,
        );
    }
    let mut x = rect.left() + pad_x;
    if dot {
        let c = egui::pos2(x + dot_sz * 0.5, rect.center().y);
        ui.painter().circle_filled(c, dot_sz * 0.5, fg);
        x += dot_sz + gap;
    }
    let pos = egui::pos2(x, rect.center().y - galley.rect.height() * 0.5);
    ui.painter().galley(pos, galley, fg);
    resp
}

/// Badge — 채움 count/status pill (디자인 `core/Badge`).
pub fn badge(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    variant: BadgeVariant,
) -> egui::Response {
    let (fill, fg) = match variant {
        BadgeVariant::Danger => (
            theme.accent_danger().to_egui(),
            theme.text_on_accent().to_egui(),
        ),
        BadgeVariant::Primary => (
            theme.accent_primary().to_egui(),
            theme.text_on_accent().to_egui(),
        ),
        BadgeVariant::Agent => (
            theme.accent_agent().to_egui(),
            theme.text_on_accent().to_egui(),
        ),
        BadgeVariant::Success => (
            theme.accent_success().to_egui(),
            theme.text_on_accent().to_egui(),
        ),
        BadgeVariant::Neutral => (
            theme.surface_active().to_egui(),
            theme.text_primary().to_egui(),
        ),
    };
    let pad_x = theme.badge_padding_x().value();
    let badge_sz = theme.badge_size().value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        mono(theme.badge_font_size().value()),
        egui::Color32::PLACEHOLDER,
    );
    let w = (galley.rect.width() + 2.0 * pad_x).max(badge_sz);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, badge_sz), egui::Sense::hover());
    // radius-pill = 완전 둥금 (badge-radius 는 radius-sm 이나 구현은 pill idiom 유지).
    ui.painter().rect_filled(rect, badge_sz * 0.5, fill);
    let pos = rect.center() - galley.rect.size() * 0.5;
    ui.painter().galley(pos, galley, fg);
    resp
}

/// 상태 점의 영역을 할당한 뒤 공용 그리기 함수를 호출한다.
pub fn badge_dot(ui: &mut egui::Ui, theme: &Theme, variant: BadgeVariant) -> egui::Response {
    let dot_sz = theme.badge_dot_size().value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(dot_sz, dot_sz), egui::Sense::hover());
    paint_badge_dot(ui.painter(), theme, rect.center(), variant);
    resp
}

/// 이미 계산된 좌표에 상태 점을 그린다. 크기는 배율이 적용된 badge-dot-size를 사용한다.
pub fn paint_badge_dot(
    painter: &egui::Painter,
    theme: &Theme,
    center: egui::Pos2,
    variant: BadgeVariant,
) {
    let fill = match variant {
        BadgeVariant::Danger => theme.accent_danger().to_egui(),
        BadgeVariant::Primary => theme.accent_primary().to_egui(),
        BadgeVariant::Agent => theme.accent_agent().to_egui(),
        BadgeVariant::Success => theme.accent_success().to_egui(),
        BadgeVariant::Neutral => theme.surface_active().to_egui(),
    };
    painter.circle_filled(center, theme.badge_dot_size().value() * 0.5, fill);
}

/// 숫자 키캡의 영역을 할당한 뒤 공용 그리기 함수를 호출한다.
pub fn num_keycap(ui: &mut egui::Ui, theme: &Theme, digit: &str, active: bool) -> egui::Response {
    let side = theme.switch_overlay_size().value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    paint_num_keycap(ui.painter(), theme, rect.center(), digit, active, 1.0);
    resp
}

/// 이미 계산된 좌표에 숫자 키캡을 그린다.
/// alpha는 채움·테두리·글자에 함께 적용한다. 크기·색은 switch-overlay 토큰을,
/// 대응 값이 없는 반경·글꼴은 kbd 토큰을 사용한다.
pub fn paint_num_keycap(
    painter: &egui::Painter,
    theme: &Theme,
    center: egui::Pos2,
    digit: &str,
    active: bool,
    alpha: f32,
) {
    let (fill, border, fg) = if active {
        (
            theme.switch_overlay_active_bg().to_egui(),
            theme.switch_overlay_active_bg().to_egui(),
            theme.switch_overlay_active_fg().to_egui(),
        )
    } else {
        (
            theme.switch_overlay_bg().to_egui(),
            theme.switch_overlay_border().to_egui(),
            theme.switch_overlay_fg().to_egui(),
        )
    };
    let (fill, border, fg) = (
        fill.gamma_multiply(alpha),
        border.gamma_multiply(alpha),
        fg.gamma_multiply(alpha),
    );
    let side = theme.switch_overlay_size().value();
    let radius = theme.kbd_radius().value();
    let bw = theme.border_width.value();
    let bottom_border = theme.switch_overlay_shadow_depth().value();
    let rect = egui::Rect::from_center_size(center, egui::vec2(side, side));
    painter.rect_filled(rect, radius, fill);
    // 키캡 하단 보더 2px 강조 → 윗변 1px, 아랫변 2px 로 따로 그린다 (kbd 와 동일).
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, border),
        egui::StrokeKind::Inside,
    );
    painter.line_segment(
        [
            egui::pos2(rect.left() + radius, rect.bottom() - bw),
            egui::pos2(rect.right() - radius, rect.bottom() - bw),
        ],
        egui::Stroke::new(bottom_border, border),
    );
    let galley = painter.layout_no_wrap(
        digit.to_owned(),
        mono(theme.kbd_font_size().value()),
        egui::Color32::PLACEHOLDER,
    );
    let pos = rect.center() - galley.rect.size() * 0.5;
    painter.galley(pos, galley, fg);
}

/// Kbd — 키캡 시퀀스. `keys` 는 `"+"` 로 분할(예: `"Ctrl+K"`), 각 키를 키캡으로.
pub fn kbd(ui: &mut egui::Ui, theme: &Theme, keys: &str) {
    let parts: Vec<&str> = keys.split('+').collect();
    let owned: Vec<KbdKey<'_>> = parts.into_iter().map(KbdKey::Text).collect();
    kbd_parts(ui, theme, &owned);
}

/// 글자 폭에 패딩을 더하되 정사각 최소 크기를 유지한다.
fn cap_width(text_w: f32, pad_x: f32, kbd_h: f32) -> f32 {
    (text_w + 2.0 * pad_x).max(kbd_h)
}

/// 키캡과 사이의 + 라벨 폭을 그리는 순서로 반환한다. 항목 사이 간격은 제외한다.
fn kbd_item_widths(ctx: &egui::Context, theme: &Theme, keys: &[KbdKey<'_>]) -> Vec<f32> {
    let micro = theme.kbd_font_size().value();
    let pad_x = theme.kbd_padding_x().value();
    let kbd_h = theme.kbd_size().value();
    let measure = |text: &str| {
        ctx.fonts(|f| {
            f.layout_no_wrap(text.to_owned(), mono(micro), egui::Color32::PLACEHOLDER)
                .rect
                .width()
        })
    };
    let mut out = Vec::with_capacity(keys.len().saturating_mul(2).saturating_sub(1));
    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            out.push(measure("+"));
        }
        out.push(match key {
            KbdKey::Text(text) => cap_width(measure(text), pad_x, kbd_h),
            KbdKey::Icon(_) => kbd_h,
        });
    }
    out
}

/// 키캡을 그릴 때와 같은 식으로 항목 폭과 간격을 합산한다.
/// 그리기 전에 상태바·팔레트의 남은 공간을 계산할 때 사용한다.
pub fn kbd_parts_width(ctx: &egui::Context, theme: &Theme, keys: &[KbdKey<'_>]) -> LogicalPx {
    let items = kbd_item_widths(ctx, theme, keys);
    let gaps = items.len().saturating_sub(1) as f32;
    LogicalPx(items.iter().sum::<f32>() + theme.kbd_gap().value() * gaps)
}

/// [`kbd`] 가 차지할 폭 — `"+"` 로 분할한 뒤 [`kbd_parts_width`] 에 넘긴다.
pub fn kbd_width(ctx: &egui::Context, theme: &Theme, keys: &str) -> LogicalPx {
    let parts: Vec<KbdKey<'_>> = keys.split('+').map(KbdKey::Text).collect();
    kbd_parts_width(ctx, theme, &parts)
}

/// 키캡의 텍스트 또는 아이콘. 폰트에 없는 보조 키 문자는 SVG 아이콘으로 전달할 수 있다.
pub enum KbdKey<'a> {
    Text(&'a str),
    Icon(tasty_icons::Icon),
}

/// 텍스트·아이콘 키캡을 함께 그린다. 두 종류는 같은 패딩·반경·테두리를 사용한다.
pub fn kbd_parts(ui: &mut egui::Ui, theme: &Theme, keys: &[KbdKey<'_>]) {
    let radius = theme.kbd_radius().value();
    let bw = theme.border_width.value();
    let border = theme.kbd_border().to_egui();
    let fill = theme.kbd_bg().to_egui();
    let fg = theme.kbd_fg().to_egui();
    let plus = theme.text_muted().to_egui(); // 키캡 사이 "+" — muted 텍스트 역할.
    let micro = theme.kbd_font_size().value();
    let icon_glyph = theme.icon_glyph_size_sm.value();
    let gap = theme.kbd_gap().value();
    let pad_x = theme.kbd_padding_x().value();
    let kbd_h = theme.kbd_size().value();
    let bottom_border = theme.kbd_shadow_depth().value();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, key) in keys.iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new("+").size(micro).color(plus).monospace());
            }
            match key {
                KbdKey::Text(text) => {
                    let galley = ui.painter().layout_no_wrap(
                        (*text).to_owned(),
                        mono(micro),
                        egui::Color32::PLACEHOLDER,
                    );
                    let w = cap_width(galley.rect.width(), pad_x, kbd_h);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(w, kbd_h), egui::Sense::hover());
                    draw_keycap_box(ui, rect, radius, bw, fill, border, bottom_border);
                    let pos = rect.center() - galley.rect.size() * 0.5;
                    ui.painter().galley(pos, galley, fg);
                }
                KbdKey::Icon(icon) => {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(kbd_h, kbd_h), egui::Sense::hover());
                    draw_keycap_box(ui, rect, radius, bw, fill, border, bottom_border);
                    let irect = egui::Rect::from_center_size(
                        rect.center(),
                        egui::vec2(icon_glyph, icon_glyph),
                    );
                    icon.image(icon_glyph, fg).paint_at(ui, irect);
                }
            }
        }
    });
}

/// 공용 폭 계산으로 키캡을 좌표에 직접 그린다.
/// right_x에서 왼쪽으로 배치하고 center_y에 세로 중심을 맞춘 뒤 사용한 폭을 반환한다.
pub fn kbd_parts_at(
    ui: &egui::Ui,
    theme: &Theme,
    keys: &[KbdKey<'_>],
    right_x: f32,
    center_y: f32,
) -> LogicalPx {
    let total = kbd_parts_width(ui.ctx(), theme, keys);
    if keys.is_empty() {
        return total;
    }
    let radius = theme.kbd_radius().value();
    let bw = theme.border_width.value();
    let border = theme.kbd_border().to_egui();
    let fill = theme.kbd_bg().to_egui();
    let fg = theme.kbd_fg().to_egui();
    let plus = theme.text_muted().to_egui();
    let micro = theme.kbd_font_size().value();
    let icon_glyph = theme.icon_glyph_size_sm.value();
    let gap = theme.kbd_gap().value();
    let pad_x = theme.kbd_padding_x().value();
    let kbd_h = theme.kbd_size().value();
    let bottom_border = theme.kbd_shadow_depth().value();
    let top = center_y - kbd_h * 0.5;
    let mut x = right_x - total.value();
    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            let g = ui
                .painter()
                .layout_no_wrap("+".to_owned(), mono(micro), plus);
            let w = g.rect.width();
            let pos = egui::pos2(x, center_y - g.rect.height() * 0.5);
            ui.painter().galley(pos, g, plus);
            x += w + gap;
        }
        match key {
            KbdKey::Text(text) => {
                let g = ui.painter().layout_no_wrap(
                    (*text).to_owned(),
                    mono(micro),
                    egui::Color32::PLACEHOLDER,
                );
                let w = cap_width(g.rect.width(), pad_x, kbd_h);
                let rect = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, kbd_h));
                draw_keycap_box(ui, rect, radius, bw, fill, border, bottom_border);
                let pos = rect.center() - g.rect.size() * 0.5;
                ui.painter().galley(pos, g, fg);
                x += w + gap;
            }
            KbdKey::Icon(icon) => {
                let rect = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(kbd_h, kbd_h));
                draw_keycap_box(ui, rect, radius, bw, fill, border, bottom_border);
                let irect =
                    egui::Rect::from_center_size(rect.center(), egui::vec2(icon_glyph, icon_glyph));
                icon.image(icon_glyph, fg).paint_at(ui, irect);
                x += kbd_h + gap;
            }
        }
    }
    total
}

/// 텍스트·아이콘 키캡이 공유하는 배경과 아래쪽 강조 테두리.
#[allow(clippy::too_many_arguments)]
fn draw_keycap_box(
    ui: &egui::Ui,
    rect: egui::Rect,
    radius: f32,
    bw: f32,
    fill: egui::Color32,
    border: egui::Color32,
    bottom_border: f32,
) {
    ui.painter().rect_filled(rect, radius, fill);
    // 키캡 하단 보더 2px 강조 → 윗변은 1px, 아랫변은 2px 로 따로 그린다.
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, border),
        egui::StrokeKind::Inside,
    );
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + radius, rect.bottom() - bw),
            egui::pos2(rect.right() - radius, rect.bottom() - bw),
        ],
        egui::Stroke::new(bottom_border, border),
    );
}
