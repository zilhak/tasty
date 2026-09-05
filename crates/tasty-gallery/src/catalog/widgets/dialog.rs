//! Scrim & frame — overlay 공통 레시피 Spec + 다른 overlay specimen 이 공유하는
//! 모달 프레임 키트.
//!
//! 디자인(4) Overlays 의 모든 모달은 같은 frame 레시피를 공유한다:
//! `bg-panel`(palette/search/tools 는 `surface-raised`) + 1px `border-strong` +
//! modal shadow, scrim `rgba(0,0,0,.5)` + blur. 이 모듈은 그 레시피를 한 Spec 으로
//! 보여주고(`draw`), 동시에 14 Spec 전부가 호출하는 frame/region/field 헬퍼를
//! `pub` 으로 노출한다 (research §2.4 공통).
//!
//! 색·간격·보더는 모두 `Theme` 토큰. scrim/shadow 의 alpha 는 디자인 토큰
//! (`scrim-bg` black 50% / `shadow-modal` black .55) 을 black-alpha 로 도출한다.
//!
//! 치수 중 **specimen 무대의 비율**은 토큰이 아니다 — 대응하는 `Theme` 값이 없고
//! (`measure_*` 는 300/400/460), 소비자가 이 파일 안뿐이라 토큰으로 올릴 근거가 없다.
//! 대신 이름 붙인 상수로 둔다(`SCRIM_STAGE_H` · `FRAME_CARD_W`) — 두 anchor 변형이
//! 같은 무대와 같은 카드 폭을 써야 나란히 놓고 비교할 수 있으므로 값이 갈리면 안 된다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::icons::MockGlyph;
use crate::catalog::spec::{self, StageVariant, TokenChip};

// ── specimen 무대 치수 (모듈 문서의 규칙: 두 변형이 나눠 쓰므로 이름을 붙인다) ──
/// scrim Spec 무대의 높이. center anchor 와 top anchor 변형이 공유한다.
const SCRIM_STAGE_H: LogicalPx = LogicalPx(200.0);
/// 무대 안에 놓는 모달 카드의 폭. 두 변형이 같아야 anchor 차이만 눈에 남는다.
const FRAME_CARD_W: LogicalPx = LogicalPx(240.0);

// ── 공유 frame 키트 (모든 overlay specimen 이 호출) ────────────────────────

/// 모달 프레임 — 지정 `fill` + 1px border-strong + modal shadow, 고정 폭.
/// 내부 콘텐츠는 region/hsep/field 로 채운다. item_spacing 은 0 으로 둔다
/// (각 region 이 자체 패딩을 가짐).
pub fn frame_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: LogicalPx,
    fill: egui::Color32,
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            // 부모 stage 가 `horizontal_wrapped`(`StageVariant::Wrap`) 여도 모달
            // 콘텐츠는 항상 세로(top_down)로 적층 + 폭을 `width` 로 bound 한다.
            // `Frame::show` 의 콘텐츠 ui 는 부모 레이아웃을 상속하므로, 명시적
            // vertical child 없이는 region 들이 가로 흐름에 얹혀 본문이 글자당
            // 줄바꿈으로 붕괴한다 (scrim_backdrop 의 top_down child 와 동일 원리).
            ui.set_width(width.value());
            ui.vertical(|ui| {
                ui.set_width(width.value());
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                add(ui);
            });
        });
}

/// 인라인 글리프 — `size` 정사각 영역을 할당해 `color` tint 로 그린다.
pub fn icon(ui: &mut egui::Ui, glyph: MockGlyph, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    glyph.image(size, color).paint_at(ui, rect);
}

/// 모달 기본 배경 (bg-panel).
pub fn panel_fill(theme: &Theme) -> egui::Color32 {
    theme.bg_panel().to_egui()
}

/// 팝오버/팔레트 배경 (surface-raised).
pub fn raised_fill(theme: &Theme) -> egui::Color32 {
    theme.surface_raised().to_egui()
}

/// 패딩 영역 — 전체 폭을 차지하는 child Ui 를 margin 안에 그린다.
pub fn region(ui: &mut egui::Ui, margin: egui::Margin, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        add(ui);
    });
}

/// 대칭 패딩 region (좌우 `x`, 상하 `y`).
pub fn region_sym(ui: &mut egui::Ui, x: LogicalPx, y: LogicalPx, add: impl FnOnce(&mut egui::Ui)) {
    region(
        ui,
        egui::Margin::symmetric(x.value() as i8, y.value() as i8),
        add,
    );
}

/// 전체 폭 1px separator (모달 region 구분선 — border-bottom).
pub fn hsep(ui: &mut egui::Ui, theme: &Theme) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

/// 모달 제목 — 14px(font-size-max) semibold, text-primary.
pub fn title(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_max.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );
}

/// 본문 산문 — 13px(body), text-secondary.
pub fn body(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_body.value())
            .color(theme.text_secondary().to_egui()),
    );
}

/// 보조 caption — 11px(caption), text-muted (mono 옵션).
pub fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str, mono: bool) {
    let mut rt = egui::RichText::new(text)
        .size(theme.font_size_caption.value())
        .color(theme.text_muted().to_egui());
    if mono {
        rt = rt.monospace();
    }
    ui.label(rt);
}

/// 정적 입력 필드 박스 (height 28, surface-raised + border-default). 데모 전용 —
/// 실 입력 없이 placeholder/값 텍스트만 표시 (gallery 는 focus 경합을 피한다).
pub fn field(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: Option<f32>,
    text: &str,
    placeholder: bool,
    mono: bool,
) {
    let h = theme.item_height_interactive.value();
    let w = width.unwrap_or_else(|| ui.available_width());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
    );
    p.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    let color = if placeholder {
        theme.text_placeholder()
    } else {
        theme.text_primary()
    };
    let font = if mono {
        egui::FontId::monospace(theme.font_size_body.value())
    } else {
        egui::FontId::proportional(theme.font_size_body.value())
    };
    p.text(
        egui::pos2(rect.left() + theme.spacing_md.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color.to_egui(),
    );
}

/// faux 앱 배경 + scrim — 모달이 그 위에 뜨는 무대. `add` 가 scrim 위 모달을
/// 그린다. `top_space` 만큼 위에서 띄워 anchor(center/top) 를 표현한다.
pub fn scrim_backdrop(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: LogicalPx,
    height: LogicalPx,
    top_space: LogicalPx,
    add: impl FnOnce(&mut egui::Ui),
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), height.value()),
        egui::Sense::hover(),
    );
    let p = ui.painter_at(rect);
    // faux app (bg-app).
    p.rect_filled(rect, theme.corner_radius.value(), theme.bg_app().to_egui());
    // scrim — theme.scrim() 토큰(다른 specimen·호스트와 동일 경로). SCRIM_ALPHA=128 이라
    // 값은 from_black_alpha(128) 과 동일하다(theme.rs scrim() 주석) — 표류를 토큰으로 돌린다.
    p.rect_filled(rect, theme.corner_radius.value(), theme.scrim().to_egui());

    // 모달을 위에서 top_space 만큼 띄워 가로 중앙 배치.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.add_space(top_space.value());
    add(&mut child);
}

// ── scrim & frame Spec ────────────────────────────────────────────────────

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        // center anchor.
        spec::cluster(ui, theme, "center anchor", |ui| {
            scrim_backdrop(
                ui,
                theme,
                theme.measure_sm,
                SCRIM_STAGE_H,
                LogicalPx(64.0),
                |ui| {
                    frame_card(ui, theme, FRAME_CARD_W, panel_fill(theme), |ui| {
                        region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
                            title(ui, theme, "Frame");
                            ui.add_space(theme.spacing_sm.value());
                            body(ui, theme, "bg-panel · 1px border-strong · modal shadow");
                        });
                    });
                },
            );
        });
        // top anchor (~88px offset).
        spec::cluster(ui, theme, "top anchor (~88px)", |ui| {
            scrim_backdrop(
                ui,
                theme,
                theme.measure_sm,
                SCRIM_STAGE_H,
                LogicalPx(28.0),
                |ui| {
                    frame_card(ui, theme, FRAME_CARD_W, raised_fill(theme), |ui| {
                        region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
                            title(ui, theme, "Palette-style");
                            ui.add_space(theme.spacing_sm.value());
                            body(ui, theme, "surface-raised · spawns under the title bar");
                        });
                    });
                },
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("scrim", "black 50% + blur 1px"),
            ("frame bg", "bg-panel / surface-raised"),
            ("frame border", "1px border-strong"),
            ("shadow", "modal — 0 20px 60px /.55"),
            ("dismiss", "scrim click / Esc"),
            ("anchors", "center · top (~88px)"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "border-strong",
                "frame edge",
                theme.border_strong().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "popover frame",
                theme.surface_raised().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Every overlay on this page is built from this recipe — the frame, the \
         scrim, and the lift never change; only the contents and the anchor do.",
    );
}
