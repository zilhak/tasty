//! `git_viewer` specimen — git-viewer plugin 의 worktree 종합 popup
//! (plugin UiNode DSL 전사, Overlays).
//!
//! 본체 렌더 경로: plugin `crates/tasty-plugin-git-viewer/src/view.rs` 의 `main_tree`
//! 가 `vbox[ header, splitter(Horizontal 0.25, worktree_rail | right_column) ]` 를 만들고
//! `right_column = splitter(Vertical 0.5, status | log·diff)` 로 분할한다. host
//! `ui_tree_render.rs` 가 selectable_row / label / splitter 를 egui 로 페인트한다.
//! 갤러리는 plugin/host crate 에 의존할 수 없어 그 *구성* 을 Theme 토큰 painter mock
//! 으로 전사한다 — 픽셀 동일성 비목표, 토큰·구조 정합 목표.
//!
//! 두 cluster 로 idiom 전수 노출:
//! - **status + log** — rail | (status 상 / log 하) 의 기본 종합 화면.
//! - **diff** — 파일 선택 시 하단 pane 이 diff 로 교체되는 변형(toolbar + ± 라인).
//!
//! 색은 host catppuccin 토큰을 의미 토큰으로 옮긴다: head/refs/main 배지 `blue`→
//! `accent_info`, current/added `green`→`accent_success`, locked/modified `yellow`→
//! `accent_warning`, invalid/deleted `red`→`accent_danger`, 비활성/`subtext0`→
//! `text_muted`, invalid 이름/`overlay0`→`text_disabled`, 선택행 → `surface_active`.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// ── popup 치수 (디자인 popup ≈960 wide, rail 0.25 · column 0.5). gallery 전시용 축소 ──
// Theme 에 대응 토큰이 없는 화면 전용 고정값 — 디자인 의미를 주석으로 명시.
/// popup 본문 폭(전시 축소; 디자인 ≈960).
const POPUP_W: f32 = 660.0;
/// popup 본문 높이.
const POPUP_H: f32 = 420.0;
/// 좌측 worktree rail 비율 (`splitter` Horizontal 0.25).
const RAIL_RATIO: f32 = 0.25;
/// 우측 컬럼 상/하 분할 비율 (`splitter` Vertical 0.5).
const COL_RATIO: f32 = 0.5;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        spec::cluster(ui, theme, "status + log — rail | (status / log)", |ui| {
            shell(ui, theme, false);
        });
        spec::cluster(ui, theme, "diff — selected file replaces the bottom pane", |ui| {
            shell(ui, theme, true);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "≈960 popup · bg-panel · read-only"),
            ("header", "Refresh + repo path"),
            ("rail", "splitter Horizontal 0.25 · worktrees"),
            ("right", "splitter Vertical 0.5 · status / log"),
            ("bottom", "log ↔ diff on file select"),
            ("badges", "main·linked / current·locked·invalid"),
        ],
        &[
            TokenChip::new("accent-info", "head · main · refs", theme.accent_info().to_egui()),
            TokenChip::new(
                "accent-success",
                "current · added",
                theme.accent_success().to_egui(),
            ),
            TokenChip::new(
                "accent-warning",
                "locked · modified",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "invalid · deleted",
                theme.accent_danger().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "A read-only worktree overview — pick a worktree on the left rail and the right \
         column rebinds its status (top) and log (bottom); selecting a file swaps the log \
         for a diff. No action buttons. The host paints the plugin's two nested splitters \
         and selectable rows; this mirrors that structure with tokens only.",
    );
}

/// popup shell — header + rail | (status / log-or-diff) 두 splitter 합성.
fn shell(ui: &mut egui::Ui, theme: &Theme, show_diff: bool) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        let w = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, POPUP_H), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let pad = theme.spacing_sm.value();

        // ── header (Refresh 버튼 + repo path) ──
        let head_h = theme.item_height_interactive.value() + pad * 2.0;
        let btn = egui::Rect::from_min_size(
            egui::pos2(rect.left() + pad, rect.top() + pad),
            egui::vec2(theme.field_width_xs.value() * 0.8, theme.item_height_interactive.value()),
        );
        p.rect_filled(
            btn,
            theme.corner_radius.value(),
            theme.surface_raised().to_egui(),
        );
        p.rect_stroke(
            btn,
            theme.corner_radius.value(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
        p.text(
            btn.center(),
            egui::Align2::CENTER_CENTER,
            "Refresh",
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_primary().to_egui(),
        );
        p.text(
            egui::pos2(btn.right() + theme.spacing_md.value(), btn.center().y),
            egui::Align2::LEFT_CENTER,
            "(~/work/tasty)",
            egui::FontId::monospace(theme.font_size_caption.value()),
            theme.text_muted().to_egui(),
        );
        // header 하단 separator.
        let body_top = rect.top() + head_h;
        p.hline(
            rect.x_range(),
            body_top,
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );

        // ── splitter Horizontal 0.25 : rail | right column ──
        let split_x = rect.left() + (w * RAIL_RATIO).round();
        let rail = egui::Rect::from_min_max(
            egui::pos2(rect.left(), body_top),
            egui::pos2(split_x, rect.bottom()),
        );
        let right = egui::Rect::from_min_max(
            egui::pos2(split_x, body_top),
            egui::pos2(rect.right(), rect.bottom()),
        );
        p.vline(
            split_x,
            body_top..=rect.bottom(),
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );

        paint_rail(&p, theme, rail);

        // ── right column: splitter Vertical 0.5 : status | log/diff ──
        let split_y = (body_top + (rect.bottom() - body_top) * COL_RATIO).round();
        let status = egui::Rect::from_min_max(right.min, egui::pos2(right.right(), split_y));
        let bottom = egui::Rect::from_min_max(egui::pos2(right.left(), split_y), right.max);
        p.hline(
            right.x_range(),
            split_y,
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );
        paint_status(&p, theme, status);
        if show_diff {
            paint_diff(&p, theme, bottom);
        } else {
            paint_log(&p, theme, bottom);
        }
    });
}

/// pane 제목 (host LabelStyle::Heading) — term-lg semibold text-primary. 다음 y 반환.
fn heading(p: &egui::Painter, theme: &Theme, pane: egui::Rect, text: &str) -> f32 {
    let pad = theme.spacing_sm.value();
    let y = pane.top() + pad;
    p.text(
        egui::pos2(pane.left() + pad, y),
        egui::Align2::LEFT_TOP,
        text,
        egui::FontId::proportional(theme.font_size_term_lg.value()),
        theme.text_primary().to_egui(),
    );
    y + theme.font_size_term_lg.value() + theme.spacing_sm.value()
}

/// selectable_row — full-width 행, 선택 시 surface-active. 좌측부터 colored 세그먼트.
fn row(
    p: &egui::Painter,
    theme: &Theme,
    pane: egui::Rect,
    y: f32,
    selected: bool,
    runs: &[(&str, egui::Color32)],
) -> f32 {
    let pad = theme.spacing_sm.value();
    let h = theme.item_height_interactive.value();
    let rect = egui::Rect::from_min_size(
        egui::pos2(pane.left() + theme.spacing_xs.value(), y),
        egui::vec2(pane.width() - theme.spacing_xs.value() * 2.0, h),
    );
    if selected {
        p.rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            theme.surface_active().to_egui(),
        );
    }
    let mut x = rect.left() + pad;
    let cy = rect.center().y;
    for (text, color) in runs {
        let r = p.text(
            egui::pos2(x, cy),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::monospace(theme.font_size_body.value()),
            *color,
        );
        x = r.right() + theme.spacing_sm.value();
    }
    y + h
}

fn paint_rail(p: &egui::Painter, theme: &Theme, pane: egui::Rect) {
    let info = theme.accent_info().to_egui();
    let mut y = heading(p, theme, pane, "Worktrees (3)");
    y = row(
        p,
        theme,
        pane,
        y,
        true,
        &[
            ("tasty", theme.text_primary().to_egui()),
            ("a1b2c3d", info),
            ("main", info),
            ("current", theme.accent_success().to_egui()),
        ],
    );
    y = row(
        p,
        theme,
        pane,
        y,
        false,
        &[
            ("feature-ui", theme.text_muted().to_egui()),
            ("9f8e7d6", info),
            ("linked", theme.text_muted().to_egui()),
            ("locked", theme.accent_warning().to_egui()),
        ],
    );
    let _ = row(
        p,
        theme,
        pane,
        y,
        false,
        &[
            ("stale-wt", theme.text_disabled().to_egui()),
            ("invalid", theme.accent_danger().to_egui()),
        ],
    );
}

fn paint_status(p: &egui::Painter, theme: &Theme, pane: egui::Rect) {
    let mut y = heading(p, theme, pane, "Status (3)");
    let text = theme.text_primary().to_egui();
    y = row(
        p,
        theme,
        pane,
        y,
        false,
        &[(" M ", theme.accent_warning().to_egui()), ("src/view.rs", text)],
    );
    y = row(
        p,
        theme,
        pane,
        y,
        false,
        &[(" A ", theme.accent_success().to_egui()), ("docs/git.md", text)],
    );
    let _ = row(
        p,
        theme,
        pane,
        y,
        false,
        &[(" D ", theme.accent_danger().to_egui()), ("old/legacy.rs", text)],
    );
}

fn paint_log(p: &egui::Painter, theme: &Theme, pane: egui::Rect) {
    let mut y = heading(p, theme, pane, "Log");
    let muted = theme.text_muted().to_egui();
    let oid = theme.accent_warning().to_egui();
    let text = theme.text_primary().to_egui();
    y = row(
        p,
        theme,
        pane,
        y,
        false,
        &[
            ("a1b2c3d", oid),
            ("feat: add worktree rail", text),
            ("zilhak", muted),
        ],
    );
    let _ = row(
        p,
        theme,
        pane,
        y,
        false,
        &[
            ("9f8e7d6", oid),
            ("fix: diff pane scroll", text),
            ("zilhak", muted),
        ],
    );
}

fn paint_diff(p: &egui::Painter, theme: &Theme, pane: egui::Rect) {
    let pad = theme.spacing_sm.value();
    // toolbar: Back 버튼 + 파일 path.
    let y = pane.top() + pad;
    p.text(
        egui::pos2(pane.left() + pad, y),
        egui::Align2::LEFT_TOP,
        "← Back",
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.accent_info().to_egui(),
    );
    p.text(
        egui::pos2(pane.left() + pad + theme.field_width_xs.value(), y),
        egui::Align2::LEFT_TOP,
        "src/view.rs",
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    let mut ly = y + theme.font_size_body.value() + theme.spacing_sm.value();
    let line_h = theme.font_size_body.value() + theme.spacing_xs.value();
    let lines: &[(&str, egui::Color32)] = &[
        ("@@ -1,4 +1,5 @@", theme.accent_info().to_egui()),
        ("   1    1   fn main_tree(vm) {", theme.text_primary().to_egui()),
        ("        2 + let header = build();", theme.accent_success().to_egui()),
        ("   2      - let h = old();", theme.accent_danger().to_egui()),
        ("   3    3   vbox(children)", theme.text_primary().to_egui()),
    ];
    for (text, color) in lines {
        p.text(
            egui::pos2(pane.left() + pad, ly),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::monospace(theme.font_size_caption.value()),
            *color,
        );
        ly += line_h;
    }
}
