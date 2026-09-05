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

/// `tag()`가 그릴 pill의 폭을 실제로 그리지 않고 미리 계산한다(`dot=false` 가정 —
/// 현재 폭 계산이 필요한 호출부는 모두 dot 없는 pill). 호출부가 그리기 전에 상한
/// 폭 안에 들어가는지 판단할 때 사용(예: git-viewer commit row의 refs pill 축약
/// 판단, `cm_row`).
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
    // 태그 테두리/채움은 accent 를 그대로 쓰지 않고 낮춘 톤이다. 대응 component
    // 토큰이 없어 값을 여기 이름으로 둔다 — 어느 토큰으로 수렴할지는 디자인 판단.
    const TAG_BORDER_OPACITY: f32 = 0.4;
    const TAG_REMOTE_FILL_OPACITY: f32 = 0.16;
    const TAG_REMOTE_BORDER_OPACITY: f32 = 0.45;
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
                    .gamma_multiply(TAG_REMOTE_BORDER_OPACITY),
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

/// Badge dot — 라벨 없는 상태 점.
///
/// `Ui` 에 `badge-dot-size` 정사각 자리를 할당하고 그 중심에 [`paint_badge_dot`] 으로
/// 그린다 — **그림은 그쪽 한 벌**이고 여기는 자리 계산만 한다. 이미 정해진 좌표에
/// 겹쳐 그려야 하는 쪽은 [`paint_badge_dot`] 을 직접 부른다.
pub fn badge_dot(ui: &mut egui::Ui, theme: &Theme, variant: BadgeVariant) -> egui::Response {
    let dot_sz = theme.badge_dot_size().value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(dot_sz, dot_sz), egui::Sense::hover());
    paint_badge_dot(ui.painter(), theme, rect.center(), variant);
    resp
}

/// 상태 점 하나를 **좌표에 직접** 그린다 — [`badge_dot`] 이 레이아웃에 자리를 잡아
/// 부르는 것과 같은 그림이다([`num_keycap`] ↔ [`paint_num_keycap`] 과 같은 갈래).
///
/// 본체 목록의 행 우측 점처럼 **행 rect 에서 계산한 좌표**에 그려야 하는 자리가 있어
/// 갈래가 둘이다. 지름은 `badge-dot-size` 에서만 오므로 지역 상수로 반지름을 박으면
/// 안 된다 — 토큰은 `ui_zoom` 을 타고 상수는 안 탄다.
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

/// 단일 숫자 키캡 (디자인 `overlays/NumCap` — switch-number overlay).
///
/// `Ui` 에 한 변 `switch-overlay-size` 인 정사각 자리를 할당하고 그 중심에
/// [`paint_num_keycap`] 으로 그린다 — **그림은 그쪽 한 벌**이고 여기는 자리 계산만
/// 한다. 이미 정해진 좌표에 겹쳐 그려야 하는 쪽은 [`paint_num_keycap`] 을 직접 부른다.
pub fn num_keycap(ui: &mut egui::Ui, theme: &Theme, digit: &str, active: bool) -> egui::Response {
    let side = theme.switch_overlay_size().value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    paint_num_keycap(ui.painter(), theme, rect.center(), digit, active, 1.0);
    resp
}

/// 숫자 키캡 한 장을 **좌표에 직접** 그린다 — [`num_keycap`] 이 레이아웃에 자리를
/// 잡아 부르는 것과 같은 그림이다.
///
/// 이 갈래가 필요한 이유는 소비처가 둘이고 **레이아웃 여부가 다르기 때문**이다.
/// 갤러리 specimen 은 `Ui` 안에 자리를 할당해 그리고, 본체 switch-number overlay 는
/// 탭 스트립·사이드바 행 위에 이미 정해진 좌표로 겹쳐 그린다(그래서 알파 페이드도
/// 필요하다). 모양이 아니라 **놓는 방식**이 다르므로, 갈리는 것은 자리 계산까지고
/// 그림은 여기 한 벌이다.
///
/// `alpha` 는 등장 페이드 계수(0..=1) — 채움·엣지·글자에 같은 값이 걸려 키캡 전체가
/// 함께 떠오른다.
///
/// 색과 두 치수(한 변·하단 두께)는 `switch-overlay-*` component 토큰을 읽는다 —
/// 이 컴포넌트가 자기 토큰 집합을 가지므로 semantic 을 직접 읽지 않는다. 디자인이
/// 키캡만 다시 칠하면 여기만 바뀐다. radius 와 글자 크기는 그 집합에 없어
/// `kbd-*` 를 그대로 쓴다(`switch-overlay-*` 자신이 `kbd-*` 의 별칭이다).
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

/// [`kbd_parts`] 한 키캡의 콘텐츠 — 텍스트 또는 벡터 아이콘.
///
/// macOS modifier 심볼(⌘/⌥/⇧)처럼 egui 폰트 fallback 체인에 없는 glyph 는
/// 텍스트로 넘기면 tofu box 로 깨진다 — 그런 키는 `Icon` 으로 넘겨 벡터로 그린다
/// (`tasty_icons::{CMD_KEY,OPTION_KEY,SHIFT_KEY}`).
pub enum KbdKey<'a> {
    Text(&'a str),
    Icon(tasty_icons::Icon),
}

/// [`kbd`] 의 텍스트+아이콘 혼합 버전 — 시각 기준(패딩·radius·하단 보더 강조)은
/// [`kbd`] 와 동일하게 공유한다. 아이콘 키는 `icon_glyph_size_sm`(14px, modifier-hint
/// 키캡 칩 표준 — 12px mono 라벨과 광학적으로 맞춘 크기)로 정사각 키캡 중앙에 그린다.
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
                    let w = (galley.rect.width() + 2.0 * pad_x).max(kbd_h);
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

/// 키캡 배경 + 보더(하단 2px 강조) — [`kbd`]/[`kbd_parts`] 공유. `kbd()`가 원래
/// 인라인으로 갖고 있던 시각 규칙 그대로, 텍스트/아이콘 두 키 종류가 재사용한다.
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
