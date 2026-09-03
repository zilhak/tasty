//! `drop-overlay` specimen — 외부 drag&drop hover 피드백 (Overlays).
//!
//! 본체 `src/adapters/ui/drop_overlay.rs::draw_drop_overlay` 의 구조 전사.
//! 그 함수는 `AppState.drop_hover` 가 활성인 동안 terminal rect 위 `Order::Tooltip`
//! 레이어에 다음 3층을 painter 로 그린다 — specimen 도 같은 순서·같은 토큰이다.
//!
//! 1. **fill**: `accent-primary` alpha 31(12%) + `corner_radius`.
//! 2. **보더**: rect 를 `spacing_sm` 만큼 shrink 한 자리에 `border_width` 1px,
//!    `accent-primary` alpha 153(60%), `StrokeKind::Inside`.
//! 3. **중앙 라벨**: `font_size_heading` + `text_primary`, `CENTER_CENTER`.
//!    파일이 2개 이상이면 `"{hover_label}  ({n} files)"` 형태로 개수를 덧붙인다.
//!
//! 갤러리는 `LayerId`/`Order` 를 넘겨받지 않는다(부유 배치는 본체 정책) — 넘겨받은
//! `Ui` 안에 무대 rect 를 할당하고 그 rect 기준으로만 그린다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 `draw_drop_overlay` 가 쓰는 alpha 두 값 (0..255).
const FILL_ALPHA: u8 = 31;
const BORDER_ALPHA: u8 = 153;

/// 무대 한 칸 크기 — 터미널 rect 를 대신하는 데모 면적.
fn stage_size(theme: &Theme) -> egui::Vec2 {
    egui::vec2(theme.measure_md.value(), theme.measure_sm.value() * 0.5)
}

/// 터미널 rect 위 overlay 1장. `label` 은 단일/다중 파일 문구.
fn overlay(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    let (rect, _) = ui.allocate_exact_size(stage_size(theme), egui::Sense::hover());
    let p = ui.painter_at(rect);

    // 무대 배경 — 본체에서는 터미널 콘텐츠가 있는 자리.
    p.rect_filled(rect, theme.corner_radius.value(), theme.bg_app().to_egui());

    // ① 반투명 fill (accent-primary 12%).
    p.rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.accent_primary().with_alpha(FILL_ALPHA).to_egui(),
    );

    // ② 1px 보더 — spacing_sm 만큼 안쪽.
    p.rect_stroke(
        rect.shrink(theme.spacing_sm.value()),
        theme.corner_radius.value(),
        egui::Stroke::new(
            theme.border_width.value(),
            theme.accent_primary().with_alpha(BORDER_ALPHA).to_egui(),
        ),
        egui::StrokeKind::Inside,
    );

    // ③ 중앙 라벨.
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_heading.value()),
        theme.text_primary().to_egui(),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Single file", |ui| {
            overlay(ui, theme, "Drop to open")
        });
        spec::cluster(ui, theme, "Multiple files", |ui| {
            overlay(ui, theme, "Drop to open  (3 files)")
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("fill", "accent-primary @ 12% · corner-radius"),
            ("border", "1px accent-primary @ 60% · inset spacing-sm"),
            ("label", "font-size-heading · text-primary · centered"),
            ("layer", "Order::Tooltip — popup 위, plugin popup 아래"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "fill + border",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("text-primary", "label", theme.text_primary().to_egui()),
            TokenChip::new("bg-app", "terminal beneath", theme.bg_app().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "터미널 rect 위에만 뜬다 — 그 밖에 떨어진 파일은 무시되고 별도 안내가 나간다. \
         hover 가 취소되거나 파일이 실제로 떨어지면 다음 프레임에 사라진다(지속 상태 없음).",
    );
}
