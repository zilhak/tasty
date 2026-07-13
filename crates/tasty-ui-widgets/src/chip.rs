//! 정적 chip primitive — `Tag` / `Badge` / `Kbd` (디자인 `components/core/*`).
//!
//! 상호작용 없는 시각 라벨. 색·폰트·치수는 전부 `tag-*`/`badge-*`/`kbd-*`
//! component 접근자(`&Theme` 경유, ui_zoom 반영)에서 가져온다. egui 한계: 폰트
//! weight(bold/medium)는 별도 family 없이 재현 불가 → 크기·색만 충실히 따른다.

use tasty_type_appearance::theme::Theme;

/// Tag variant (디자인 `core/Tag`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TagVariant {
    /// 외곽선 chip (기본) — surface-raised + border-default + text-secondary.
    Default,
    Accent,
    Agent,
    /// sky 톤 tinted chip — 투명 채움 + accent-info 40% border + accent-info fg.
    /// 디자인 Tag variants 에 없던 톤(git-viewer 의 main/oid/refs/hunk = sky).
    Info,
    Success,
    Warning,
    Danger,
    /// sky 톤 **채움** chip(fill 16%/border 45%) — accent-remote 기준. 디자인
    /// 2026-07-13 workspace-remote-indicator: 사이드바 mirror 워크스페이스 pill
    /// 전용. `Info`(투명 채움+40% 보더, git-viewer 태그용)와 시각이 달라 별도
    /// variant로 분리 — `Info` alpha를 바꾸면 git-viewer 태그가 회귀한다.
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

/// Tag — 모노 라벨 chip. `dot` 이 true 면 선행 상태 점(현재 fg 색).
pub fn tag(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    variant: TagVariant,
    dot: bool,
) -> egui::Response {
    let (fill, border, fg) = match variant {
        // Default(외곽선 chip)만 `tag-*` component 색 대응. 나머지 상태 변형(accent
        // 계열)은 대응 component 토큰이 없어 semantic 유지.
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
            Some(theme.accent_info().to_egui().gamma_multiply(0.4)),
            theme.accent_info().to_egui(),
        ),
        TagVariant::Remote => (
            theme.accent_remote().to_egui().gamma_multiply(0.16),
            Some(theme.accent_remote().to_egui().gamma_multiply(0.45)),
            theme.accent_remote().to_egui(),
        ),
        TagVariant::Success => (
            egui::Color32::TRANSPARENT,
            Some(theme.accent_success().to_egui().gamma_multiply(0.4)),
            theme.accent_success().to_egui(),
        ),
        TagVariant::Warning => (
            egui::Color32::TRANSPARENT,
            Some(theme.accent_warning().to_egui().gamma_multiply(0.4)),
            theme.accent_warning().to_egui(),
        ),
        TagVariant::Danger => (
            egui::Color32::TRANSPARENT,
            Some(theme.accent_danger().to_egui().gamma_multiply(0.4)),
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

/// Badge dot — 라벨 없는 8px 상태 점.
pub fn badge_dot(ui: &mut egui::Ui, theme: &Theme, variant: BadgeVariant) -> egui::Response {
    let fill = match variant {
        BadgeVariant::Danger => theme.accent_danger().to_egui(),
        BadgeVariant::Primary => theme.accent_primary().to_egui(),
        BadgeVariant::Agent => theme.accent_agent().to_egui(),
        BadgeVariant::Success => theme.accent_success().to_egui(),
        BadgeVariant::Neutral => theme.surface_active().to_egui(),
    };
    let dot_sz = theme.badge_dot_size().value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(dot_sz, dot_sz), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), dot_sz * 0.5, fill);
    resp
}

/// 단일 숫자 키캡 (디자인 `overlays/NumCap` — switch-number overlay).
///
/// `kbd()` 의 단일 키캡 시각을 그대로 따르되 `active` 면 accent fill 로 교체한다.
/// modifier 홀드 중 탭/워크스페이스의 leading indicator 를 제자리 교체하는 용도라
/// 16×16 고정. core `kbd()` 와 시각 기준(상수·하단 2px·radius)을 공유한다.
///
/// - inactive: `surface_raised` fill + `border_strong` 엣지 + `text_secondary` 숫자.
/// - active: `accent_primary` fill/엣지 + `text_on_accent` 숫자.
pub fn num_keycap(ui: &mut egui::Ui, theme: &Theme, digit: &str, active: bool) -> egui::Response {
    // inactive 는 `kbd-*` component 색 대응. active accent 는 chip component 토큰
    // 없어 semantic 유지.
    let (fill, border, fg) = if active {
        let accent = theme.accent_primary().to_egui();
        (accent, accent, theme.text_on_accent().to_egui())
    } else {
        (
            theme.kbd_bg().to_egui(),
            theme.kbd_border().to_egui(),
            theme.kbd_fg().to_egui(),
        )
    };
    let radius = theme.kbd_radius().value();
    let bw = theme.border_width.value();
    let micro = theme.kbd_font_size().value();
    let kbd_h = theme.kbd_size().value();
    let bottom_border = theme.kbd_shadow_depth().value();
    let galley =
        ui.painter()
            .layout_no_wrap(digit.to_owned(), mono(micro), egui::Color32::PLACEHOLDER);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(kbd_h, kbd_h), egui::Sense::hover());
    ui.painter().rect_filled(rect, radius, fill);
    // 키캡 하단 보더 2px 강조 → 윗변 1px, 아랫변 2px 로 따로 그린다 (kbd 와 동일).
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
    let pos = rect.center() - galley.rect.size() * 0.5;
    ui.painter().galley(pos, galley, fg);
    resp
}

/// Kbd — 키캡 시퀀스. `keys` 는 `"+"` 로 분할(예: `"Ctrl+K"`), 각 키를 키캡으로.
pub fn kbd(ui: &mut egui::Ui, theme: &Theme, keys: &str) {
    let radius = theme.kbd_radius().value();
    let bw = theme.border_width.value();
    let border = theme.kbd_border().to_egui();
    let fill = theme.kbd_bg().to_egui();
    let fg = theme.kbd_fg().to_egui();
    let plus = theme.text_muted().to_egui(); // 키캡 사이 "+" — muted 텍스트 역할.
    let micro = theme.kbd_font_size().value();
    let gap = theme.kbd_gap().value();
    let pad_x = theme.kbd_padding_x().value();
    let kbd_h = theme.kbd_size().value();
    let bottom_border = theme.kbd_shadow_depth().value();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let parts: Vec<&str> = keys.split('+').collect();
        for (i, key) in parts.iter().enumerate() {
            if i > 0 {
                ui.label(egui::RichText::new("+").size(micro).color(plus).monospace());
            }
            let galley = ui.painter().layout_no_wrap(
                (*key).to_owned(),
                mono(micro),
                egui::Color32::PLACEHOLDER,
            );
            let w = (galley.rect.width() + 2.0 * pad_x).max(kbd_h);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, kbd_h), egui::Sense::hover());
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
            let pos = rect.center() - galley.rect.size() * 0.5;
            ui.painter().galley(pos, galley, fg);
        }
    });
}
