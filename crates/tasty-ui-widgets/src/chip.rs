//! 정적 chip primitive — `Tag` / `Badge` / `Kbd` (디자인 `components/core/*`).
//!
//! 상호작용 없는 시각 라벨. 색·폰트는 `&Theme`, 디자인 고정 px(높이/패딩/폰트)는
//! 위젯 const. egui 한계: 폰트 weight(bold/medium)는 별도 family 없이 재현 불가 →
//! 크기·색만 충실히 따른다.

use tasty_type_appearance::theme::Theme;

// ── 디자인 고정 px (components/core 의 token-policy 반영 값) ──
// pill 높이 16(size-16), 폰트는 micro(10) — 모두 Theme 토큰에서. padding/gap/dot 은
// space/size 스케일에 정합 (Tag pad sm=8, Badge pad xs=4, dot 8).
const TAG_HEIGHT: f32 = 16.0;
const TAG_PAD_X: f32 = 8.0; // space-sm — 외곽선 chip
const TAG_GAP: f32 = 4.0; // space-xs
const TAG_DOT: f32 = 8.0; // status-dot-size
const BADGE_HEIGHT: f32 = 16.0;
const BADGE_MIN_W: f32 = 16.0;
const BADGE_PAD_X: f32 = 4.0; // space-xs — tight count pill
const BADGE_DOT: f32 = 8.0;
const KBD_HEIGHT: f32 = 16.0;
const KBD_MIN_W: f32 = 16.0;
const KBD_PAD_X: f32 = 4.0; // space-xs
const KBD_GAP: f32 = 3.0; // kbd 키캡 간 간격(off-grid 키캡 관습)
const KBD_BOTTOM_BORDER: f32 = 2.0;

/// Tag variant (디자인 `core/Tag`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TagVariant {
    /// 외곽선 chip (기본) — surface-raised + border-default + text-secondary.
    Default,
    Accent,
    Agent,
    Success,
    Warning,
    Danger,
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
pub fn tag(ui: &mut egui::Ui, theme: &Theme, label: &str, variant: TagVariant, dot: bool) -> egui::Response {
    let (fill, border, fg) = match variant {
        TagVariant::Default => (
            theme.surface_raised().to_egui(),
            Some(theme.border_default().to_egui()),
            theme.text_secondary().to_egui(),
        ),
        TagVariant::Accent => (theme.accent_primary().to_egui(), None, theme.text_on_accent().to_egui()),
        TagVariant::Agent => (theme.accent_agent().to_egui(), None, theme.text_on_accent().to_egui()),
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
    let radius = theme.corner_radius_sm.value();
    let bw = theme.border_width.value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        mono(theme.font_size_micro.value()),
        egui::Color32::PLACEHOLDER,
    );
    let dot_w = if dot { TAG_DOT + TAG_GAP } else { 0.0 };
    let w = galley.rect.width() + dot_w + 2.0 * TAG_PAD_X;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, TAG_HEIGHT), egui::Sense::hover());
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, radius, fill);
    }
    if let Some(bc) = border {
        ui.painter()
            .rect_stroke(rect, radius, egui::Stroke::new(bw, bc), egui::StrokeKind::Inside);
    }
    let mut x = rect.left() + TAG_PAD_X;
    if dot {
        let c = egui::pos2(x + TAG_DOT * 0.5, rect.center().y);
        ui.painter().circle_filled(c, TAG_DOT * 0.5, fg);
        x += TAG_DOT + TAG_GAP;
    }
    let pos = egui::pos2(x, rect.center().y - galley.rect.height() * 0.5);
    ui.painter().galley(pos, galley, fg);
    resp
}

/// Badge — 채움 count/status pill (디자인 `core/Badge`).
pub fn badge(ui: &mut egui::Ui, theme: &Theme, label: &str, variant: BadgeVariant) -> egui::Response {
    let (fill, fg) = match variant {
        BadgeVariant::Danger => (theme.accent_danger().to_egui(), theme.text_on_accent().to_egui()),
        BadgeVariant::Primary => (theme.accent_primary().to_egui(), theme.text_on_accent().to_egui()),
        BadgeVariant::Agent => (theme.accent_agent().to_egui(), theme.text_on_accent().to_egui()),
        BadgeVariant::Success => (theme.accent_success().to_egui(), theme.text_on_accent().to_egui()),
        BadgeVariant::Neutral => (theme.surface_active().to_egui(), theme.text_primary().to_egui()),
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        mono(theme.font_size_micro.value()),
        egui::Color32::PLACEHOLDER,
    );
    let w = (galley.rect.width() + 2.0 * BADGE_PAD_X).max(BADGE_MIN_W);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, BADGE_HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(rect, BADGE_HEIGHT * 0.5, fill); // radius-pill = 완전 둥금
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
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(BADGE_DOT, BADGE_DOT), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), BADGE_DOT * 0.5, fill);
    resp
}

/// Kbd — 키캡 시퀀스. `keys` 는 `"+"` 로 분할(예: `"Ctrl+K"`), 각 키를 키캡으로.
pub fn kbd(ui: &mut egui::Ui, theme: &Theme, keys: &str) {
    let radius = theme.corner_radius_sm.value();
    let bw = theme.border_width.value();
    let border = theme.border_strong().to_egui();
    let fill = theme.surface_raised().to_egui();
    let fg = theme.text_secondary().to_egui();
    let plus = theme.subtext0.to_egui();
    let micro = theme.font_size_micro.value();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = KBD_GAP;
        let parts: Vec<&str> = keys.split('+').collect();
        for (i, key) in parts.iter().enumerate() {
            if i > 0 {
                ui.label(
                    egui::RichText::new("+")
                        .size(micro)
                        .color(plus)
                        .monospace(),
                );
            }
            let galley = ui.painter().layout_no_wrap(
                (*key).to_owned(),
                mono(micro),
                egui::Color32::PLACEHOLDER,
            );
            let w = (galley.rect.width() + 2.0 * KBD_PAD_X).max(KBD_MIN_W);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, KBD_HEIGHT), egui::Sense::hover());
            ui.painter().rect_filled(rect, radius, fill);
            // 키캡 하단 보더 2px 강조 → 윗변은 1px, 아랫변은 2px 로 따로 그린다.
            ui.painter()
                .rect_stroke(rect, radius, egui::Stroke::new(bw, border), egui::StrokeKind::Inside);
            ui.painter().line_segment(
                [
                    egui::pos2(rect.left() + radius, rect.bottom() - bw),
                    egui::pos2(rect.right() - radius, rect.bottom() - bw),
                ],
                egui::Stroke::new(KBD_BOTTOM_BORDER, border),
            );
            let pos = rect.center() - galley.rect.size() * 0.5;
            ui.painter().galley(pos, galley, fg);
        }
    });
}
