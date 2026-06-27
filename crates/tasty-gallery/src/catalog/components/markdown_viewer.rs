//! `markdown_viewer` specimen — Markdown surface 의 host egui 패널 (Layouts).
//!
//! 본체 렌더 경로: `src/adapters/ui/surface/markdown.rs::draw_markdown` 가
//! `ScrollArea::vertical` 안에서 `egui_commonmark::CommonMarkViewer` 로 문서를 그린다.
//! toolbar·헤더 없이 surface 타일 전체를 본문이 채운다. 색은 commonmark visuals 에
//! 주입된다: `override_text_color = subtext1`(→ `text_secondary`, 헤딩 포함 전체 본문),
//! `hyperlink_color = accent_primary`, `code_bg_color = surface0`(→ `surface_raised`).
//! 폰트는 본문 기준 비례: 헤딩 = body×1.5, small = body×0.85, 코드 = mono.
//!
//! 갤러리는 본체 crate·`egui_commonmark` 에 의존하지 않으므로 대표 문서(헤딩 / 문단 /
//! 링크 / 리스트 / 코드블록 / 캡션)를 같은 토큰·비례로 painter 전사한다 — 픽셀 동일성
//! 비목표, 토큰·구조 정합 목표.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// ── surface 타일 대표 치수 (surface 는 타일 전체를 채움 — 전시용 고정 박스) ──
/// 본문 폭(전시 박스).
const PANE_W: f32 = 560.0;
/// 본문 높이(전시 박스).
const PANE_H: f32 = 360.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        document(ui, theme);
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "surface tile · bg-panel · no toolbar"),
            ("scroll", "ScrollArea vertical · fills width"),
            ("body", "override_text_color = text-secondary"),
            ("heading", "body × 1.5 (same color)"),
            ("link", "hyperlink = accent-primary"),
            ("code", "code_bg = surface-raised · mono"),
        ],
        &[
            TokenChip::new("bg-panel", "surface", theme.bg_panel().to_egui()),
            TokenChip::new("text-secondary", "body", theme.text_secondary().to_egui()),
            TokenChip::new("accent-primary", "link", theme.accent_primary().to_egui()),
            TokenChip::new("surface-raised", "code bg", theme.surface_raised().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "A read-only Markdown surface — egui_commonmark renders the file into a vertical \
         scroll area that fills the tile, with theme colors injected into the renderer's \
         visuals (body text, links, code background). This specimen transcribes a \
         representative document with the same tokens and body-relative font scale.",
    );
}

/// 대표 마크다운 문서 — 헤딩 / 문단 / 링크 / 리스트 / 코드블록 / 캡션.
fn document(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, PANE_W, kit::panel_fill(theme), |ui| {
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, PANE_H), egui::Sense::hover());
        let p = ui.painter_at(rect);

        let pad = theme.spacing_md.value();
        let body = theme.font_size_body.value();
        let secondary = theme.text_secondary().to_egui();
        let left = rect.left() + pad;
        let mut y = rect.top() + pad;

        // H1 — body×1.5, override_text_color(=text-secondary) 그대로.
        y = line(&p, theme, left, y, "Markdown surface", body * 1.5, secondary);
        y += theme.spacing_sm.value();

        // 문단 (두 줄로 wrap).
        y = line(
            &p,
            theme,
            left,
            y,
            "A read-only viewer that reloads on external file changes.",
            body,
            secondary,
        );
        y = line(
            &p,
            theme,
            left,
            y,
            "Tables, checkboxes, links and code blocks all render.",
            body,
            secondary,
        );
        y += theme.spacing_sm.value();

        // 링크 줄 — hyperlink_color = accent-primary.
        y = line(
            &p,
            theme,
            left,
            y,
            "See docs/index.md for the full guide",
            body,
            theme.accent_primary().to_egui(),
        );
        y += theme.spacing_sm.value();

        // 불릿 리스트.
        for item in ["fenced code blocks", "task lists", "inline `code`"] {
            y = line(&p, theme, left + pad, y, &format!("•  {item}"), body, secondary);
        }
        y += theme.spacing_sm.value();

        // 코드 블록 — surface-raised 배경, mono.
        let code_lines = [
            "fn main() {",
            "    println!(\"hello, tasty\");",
            "}",
        ];
        let line_h = body + theme.spacing_xs.value();
        let block_h = line_h * code_lines.len() as f32 + theme.spacing_sm.value() * 2.0;
        let block = egui::Rect::from_min_size(
            egui::pos2(left, y),
            egui::vec2(rect.right() - left - pad, block_h),
        );
        p.rect_filled(
            block,
            theme.corner_radius.value(),
            theme.surface_raised().to_egui(),
        );
        let mut cy = block.top() + theme.spacing_sm.value();
        for cl in code_lines {
            p.text(
                egui::pos2(block.left() + theme.spacing_sm.value(), cy),
                egui::Align2::LEFT_TOP,
                cl,
                egui::FontId::monospace(body),
                secondary,
            );
            cy += line_h;
        }
        y = block.bottom() + theme.spacing_sm.value();

        // 캡션 — small = body×0.85, text-muted.
        let _ = line(
            &p,
            theme,
            left,
            y,
            "Last reloaded just now",
            (body * 0.85).max(1.0),
            theme.text_muted().to_egui(),
        );
    });
}

/// 한 줄 본문 텍스트(proportional). `gap` 만큼 띄운 다음 줄 y 반환.
fn line(
    p: &egui::Painter,
    theme: &Theme,
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    color: egui::Color32,
) -> f32 {
    let r = p.text(
        egui::pos2(x, y),
        egui::Align2::LEFT_TOP,
        text,
        egui::FontId::proportional(size),
        color,
    );
    r.bottom() + theme.spacing_xs.value()
}
