#![cfg(target_os = "windows")]
//! Windows CSD 캡션 버튼 (minimize / maximize·restore / close).
//!
//! 디자인: 각 버튼 `caption_width`(46px) 폭 × titlebar full height, 우측 정렬 클러스터
//! (좌→우 = minimize, maximize, close). hover bg = `overlay-hover`, press bg =
//! `overlay-active`. close hover 만 시스템 red(`accent-window-close`) bg + 흰
//! 글리프(`text-on-window-close`). 글리프는 벡터 스트로크로 painter 직접 렌더.
//! 색·폭은 모두 P1 titlebar 토큰(`crates/tasty-type-appearance`)에서만 가져온다.
//!
//! 본 모듈은 `view.rs` 와 함께 순수 view 계층이다 — winit `Window` / 글로벌 theme 에
//! 접근하지 않고 props(theme 참조 포함)와 egui `Ui` 만으로 동작한다.

use super::view::{TitlebarAction, TitlebarProps};
use crate::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// 글리프 한 변 크기 (logical points). 46px 버튼 안의 중앙 ~10px 박스.
const GLYPH: LogicalPx = LogicalPx(10.0);
/// 글리프 스트로크 굵기 (logical points). UI kit 1px 보더 관습과 동일.
const GLYPH_STROKE: LogicalPx = LogicalPx(1.0);

/// 캡션 클러스터(min·max·close 3버튼) 전체 폭 (logical points).
/// `view.rs` 의 우측 슬롯 carve-out + 드래그 rect 계산에 쓰인다.
pub fn cluster_width(theme: &Theme) -> f32 {
    theme.caption_width.value() * 3.0
}

/// [`draw_caption_buttons`] 의 반환값 — 클릭 액션 + 가장자리 리사이즈 우선권 판정.
pub struct CaptionDrawResult {
    pub actions: Vec<TitlebarAction>,
    /// 마우스가 3버튼(min/max/close) 중 하나 위인지 — `view.rs` 가
    /// `TitlebarDrawResult::resize_priority_hovered` 로 합성한다.
    pub hovered: bool,
}

/// 우측 캡션 슬롯(`rect`)에 3버튼을 그리고 클릭 액션을 보고한다.
/// `rect.width()` 는 `cluster_width(props.theme)` 와 같다고 가정한다.
pub fn draw_caption_buttons(
    ui: &egui::Ui,
    rect: egui::Rect,
    props: &TitlebarProps,
) -> CaptionDrawResult {
    let th = props.theme;
    let w = th.caption_width.value();
    let mut actions = Vec::new();
    let mut hovered = false;

    // 좌→우: minimize, maximize, close.
    for (idx, kind) in [Glyph::Minimize, Glyph::Maximize, Glyph::Close]
        .into_iter()
        .enumerate()
    {
        let cell = egui::Rect::from_min_size(
            egui::pos2(rect.left() + w * idx as f32, rect.top()),
            egui::vec2(w, rect.height()),
        );
        let resp = ui.interact(
            cell,
            egui::Id::new(("tasty_caption_btn", idx)),
            egui::Sense::click(),
        );
        let btn_hovered = resp.hovered();
        hovered |= btn_hovered;
        let pressed = resp.is_pointer_button_down_on();
        let is_close = matches!(kind, Glyph::Close);

        // 배경: close hover 만 시스템 red, 그 외엔 overlay hover/active.
        let painter = ui.painter();
        if is_close && btn_hovered {
            painter.rect_filled(cell, 0.0, th.accent_window_close().to_egui());
        } else if pressed {
            painter.rect_filled(cell, 0.0, th.overlay_active().to_egui());
        } else if btn_hovered {
            painter.rect_filled(cell, 0.0, th.overlay_hover().to_egui());
        }

        // 글리프 색: close hover=white, 그 외 hover=text-primary,
        // 평상시=active/inactive 디밍된 titlebar fg.
        let glyph_color = if is_close && btn_hovered {
            th.text_on_window_close().to_egui()
        } else if btn_hovered {
            th.text_primary().to_egui()
        } else if props.active {
            th.titlebar_fg().to_egui()
        } else {
            th.titlebar_fg_inactive().to_egui()
        };
        paint_glyph(painter, cell.center(), kind, props.maximized, glyph_color);

        if resp.clicked() {
            actions.push(match kind {
                Glyph::Minimize => TitlebarAction::Minimize,
                Glyph::Maximize => TitlebarAction::ToggleMaximize,
                Glyph::Close => TitlebarAction::Close,
            });
        }
    }

    CaptionDrawResult { actions, hovered }
}

#[derive(Clone, Copy)]
enum Glyph {
    Minimize,
    Maximize,
    Close,
}

/// 중심 `c` 기준으로 캡션 글리프를 벡터 스트로크로 그린다.
/// maximize 는 `maximized` 면 restore(겹친 두 사각형) 글리프로 토글한다.
fn paint_glyph(
    painter: &egui::Painter,
    c: egui::Pos2,
    kind: Glyph,
    maximized: bool,
    color: egui::Color32,
) {
    let h = GLYPH / 2.0;
    let stroke = egui::Stroke::new(GLYPH_STROKE.value(), color);
    match kind {
        Glyph::Minimize => {
            // 중앙 수평선.
            painter.line_segment(
                [
                    egui::pos2(c.x - h.value(), c.y),
                    egui::pos2(c.x + h.value(), c.y),
                ],
                stroke,
            );
        }
        Glyph::Maximize if !maximized => {
            // 단일 사각형 외곽선.
            painter.rect_stroke(
                egui::Rect::from_center_size(c, egui::vec2(GLYPH.value(), GLYPH.value())),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Glyph::Maximize => {
            // restore: 앞 사각형(좌하) + 뒤 사각형(우상)으로 겹침을 표현.
            let s = GLYPH - LogicalPx(2.0);
            let off = 2.0;
            let front = egui::Rect::from_min_size(
                egui::pos2(c.x - h.value(), c.y - h.value() + off),
                egui::vec2(s.value(), s.value()),
            );
            painter.rect_stroke(front, 0.0, stroke, egui::StrokeKind::Inside);
            // 뒤 사각형은 우상단 모서리만 보이도록 ㄱ자 두 선분으로 그린다.
            let bx0 = LogicalPx(front.left() + off);
            let bx1 = bx0 + s;
            let by0 = front.top() - off;
            let by1 = front.top();
            painter.line_segment(
                [egui::pos2(bx0.value(), by0), egui::pos2(bx1.value(), by0)],
                stroke,
            );
            painter.line_segment(
                [egui::pos2(bx1.value(), by0), egui::pos2(bx1.value(), by1)],
                stroke,
            );
        }
        Glyph::Close => {
            // X (두 대각선).
            painter.line_segment(
                [
                    egui::pos2(c.x - h.value(), c.y - h.value()),
                    egui::pos2(c.x + h.value(), c.y + h.value()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(c.x - h.value(), c.y + h.value()),
                    egui::pos2(c.x + h.value(), c.y - h.value()),
                ],
                stroke,
            );
        }
    }
}
