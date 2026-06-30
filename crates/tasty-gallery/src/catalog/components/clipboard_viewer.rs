//! `clipboard_viewer` specimen — clipboard-viewer plugin 의 master-detail popup
//! (plugin UiNode DSL 전사, Overlays).
//!
//! 본체 렌더 경로: plugin `crates/tasty-plugin-clipboard-viewer/src/view.rs` 의
//! `main_tree` 가 `splitter(Horizontal, 0.3, 좌 타입목록 | 우 text_preview)` 를 만들고,
//! host `ui_tree_render.rs` 가 그 UiNode 를 egui 로 페인트한다(button / text_preview /
//! splitter). 갤러리는 plugin/host crate 에 의존할 수 없어 그 *구성* 을 Theme 토큰
//! painter mock 으로 전사한다 — 픽셀 동일성 비목표, 토큰·구조 정합 목표.
//!
//! 3 상태를 나란히 노출(`main_tree` 분기와 동일):
//! - **types** — 정상 master-detail(좌 타입 버튼 목록, 선택 = primary 강조 / 우 미리보기).
//! - **empty** — 가용 타입 0개(`subtext0` 안내).
//! - **read failed** — 클립보드 핸들 실패(`red` 안내).
//!
//! 색·치수·폰트는 전부 `Theme`. host 의 catppuccin 토큰 매핑을 의미 토큰으로 옮긴다:
//! `button_primary` 채움 → `accent_primary`, 일반 버튼 → `surface_raised`,
//! `subtext0` → `text_muted`, `red` → `accent_danger`.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// ── popup 치수 (디자인 clipboard-viewer 480×360, splitter Horizontal 0.3) ──
// Theme 에 대응 토큰이 없는 화면 전용 고정값 — 디자인 px 를 주석으로 명시.
/// popup 본문 폭.
const POPUP_W: f32 = 480.0;
/// popup 본문 높이.
const POPUP_H: f32 = 360.0;
/// 좌측 타입 목록 비율 (`splitter` ratio 0.3).
const LEFT_RATIO: f32 = 0.3;

/// (라벨, 선택됨)
const TYPES: &[(&str, bool)] = &[("Text", true), ("Image", false), ("Files", false)];

/// 우측 미리보기 샘플 — 현재 클립보드 text 표현(mono).
const PREVIEW: &[&str] = &[
    "cargo build -p tasty-gallery",
    "git switch wt-5/T8-code",
    "tasty read screen --surface 3",
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "types available — master-detail", |ui| {
            master_detail(ui, theme);
        });
        spec::cluster(ui, theme, "empty clipboard", |ui| {
            state_box(
                ui,
                theme,
                "Clipboard is empty",
                theme.text_muted().to_egui(),
            );
        });
        spec::cluster(ui, theme, "read failed", |ui| {
            state_box(
                ui,
                theme,
                "Failed to read the clipboard",
                theme.accent_danger().to_egui(),
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "480×360 popup · bg-panel"),
            ("split", "splitter Horizontal · ratio 0.3"),
            ("left", "type list — Button per available type"),
            ("selected", "primary fill (button_primary)"),
            ("right", "scroll_v(text_preview) — mono"),
            ("states", "types · empty · read-failed"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "accent-primary",
                "selected type",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "idle type",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new("separator", "split divider", theme.separator.to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "A read-only window onto the current clipboard — the left rail lists the types \
         the OS currently holds, the right pane previews the selected one. The host paints \
         the plugin's splitter / buttons / text-preview; this specimen mirrors that \
         composition with tokens only.",
    );
}

/// 정상 master-detail — 좌 타입 목록(선택=primary) | 1px divider | 우 mono 미리보기.
fn master_detail(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, POPUP_H), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let split_x = rect.left() + (w * LEFT_RATIO).round();

        // ── 좌측 타입 목록 ──
        let pad = theme.spacing_sm.value();
        let row_h = theme.item_height_interactive.value();
        let mut y = rect.top() + pad;
        for (label, selected) in TYPES {
            let row = egui::Rect::from_min_size(
                egui::pos2(rect.left() + pad, y),
                egui::vec2(split_x - rect.left() - pad * 2.0, row_h),
            );
            let (fill, fg) = if *selected {
                (theme.accent_primary(), theme.text_on_accent())
            } else {
                (theme.surface_raised(), theme.text_secondary())
            };
            p.rect_filled(row, theme.corner_radius.value(), fill.to_egui());
            // idle 타입 버튼 = surface-raised + border-default(1px) — host `button()`
            // 스타일 기준(changelog). selected 행은 fill 만(accent).
            if !*selected {
                p.rect_stroke(
                    row,
                    theme.corner_radius.value(),
                    egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                    egui::StrokeKind::Inside,
                );
            }
            p.text(
                egui::pos2(row.left() + theme.spacing_md.value(), row.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(theme.font_size_body.value()),
                fg.to_egui(),
            );
            y += row_h + theme.spacing_xs.value();
        }

        // ── divider (host splitter 의 rest 색 = 얇은 1px) ──
        p.vline(
            split_x,
            rect.y_range(),
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );

        // ── 우측 미리보기 (mono, text-primary) ──
        let tx = split_x + pad;
        let mut ty = rect.top() + pad;
        let line_h = theme.font_size_body.value() + theme.spacing_xs.value();
        for line in PREVIEW {
            p.text(
                egui::pos2(tx, ty),
                egui::Align2::LEFT_TOP,
                line,
                egui::FontId::monospace(theme.font_size_body.value()),
                theme.text_primary().to_egui(),
            );
            ty += line_h;
        }
    });
}

/// 빈상태 / 읽기실패 — popup 폭 프레임에 중앙 메시지 한 줄.
fn state_box(ui: &mut egui::Ui, theme: &Theme, msg: &str, color: egui::Color32) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        let w = ui.available_width();
        // 빈상태는 본문이 짧으므로 popup 높이의 1/3 정도만 차지.
        let h = (POPUP_H / 3.0).round();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter_at(rect).text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(theme.font_size_body.value()),
            color,
        );
    });
}
