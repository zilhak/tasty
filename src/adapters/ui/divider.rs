//! pane 구분선과 surface 알림 테두리. 상태에서 그릴 영역을 모아 공용 view에 전달한다.

use egui::emath::GuiRounding as _;
use tasty_type_appearance::theme::Theme;

use crate::core::AttentionKind;
use crate::model::PhysicalRect;
use crate::state::AppState;
use crate::theme;

pub fn draw_pane_dividers(ctx: &egui::Context, dividers: &[PhysicalRect], scale_factor: f32) {
    let th = theme::theme();
    if dividers.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("pane_dividers"),
    ));
    let border_color = th.border_frame();
    for div in dividers {
        let rect = crate::adapters::ui::to_egui_rect(*div, scale_factor).round_ui();
        painter.rect_filled(rect, 0.0, border_color);
    }
}

/// 물리 좌표의 surface 영역과 알림 종류. kind가 없으면 테두리를 그리지 않는다.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceHighlightRegion {
    pub rect: PhysicalRect,
    pub kind: Option<AttentionKind>,
}

pub struct SurfaceHighlightsProps<'a> {
    pub theme: &'a Theme,
    pub regions: &'a [SurfaceHighlightRegion],
    pub scale_factor: f32,
}

/// NeedsInput은 accent_warning, Completion은 accent_primary 색을 쓴다.
fn highlight_stroke_color(theme: &Theme, kind: AttentionKind) -> egui::Color32 {
    match kind {
        AttentionKind::NeedsInput => theme.accent_warning().into(),
        AttentionKind::Completion => theme.accent_primary().into(),
    }
}

/// 알림이 있는 영역에 Theme.focus_ring_width로 테두리를 그린다.
pub fn draw_surface_highlights_view(ctx: &egui::Context, props: &SurfaceHighlightsProps<'_>) {
    if props.regions.iter().all(|r| r.kind.is_none()) {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("surface_highlights"),
    ));
    let scale_factor = props.scale_factor;
    for region in props.regions {
        let Some(kind) = region.kind else {
            continue;
        };
        let stroke_color = highlight_stroke_color(props.theme, kind);
        let r = region.rect;
        let egui_rect = crate::adapters::ui::to_egui_rect(r, scale_factor).round_ui();
        painter.rect_stroke(
            egui_rect,
            0.0,
            egui::Stroke::new(props.theme.focus_ring_width.value(), stroke_color),
            egui::StrokeKind::Inside,
        );
    }
}

pub(crate) fn regions_from_state(
    state: &AppState,
    engine: &crate::core::CoreState,
    terminal_rect: PhysicalRect,
    scale_factor: f32,
) -> Vec<SurfaceHighlightRegion> {
    let regions = state.surface_regions(engine, terminal_rect, scale_factor);
    let mut out = Vec::new();
    for (_pane_id, _pane_rect, surface_regions) in &regions {
        for r in surface_regions {
            // 응답 필요는 점유 표시보다 우선한다. 완료 표시는 점유 중 숨긴다.
            let occupied = engine.attach.occupancy_of(r.id).is_some();
            let kind = match engine.attention_kind(r.id) {
                Some(AttentionKind::NeedsInput) => Some(AttentionKind::NeedsInput),
                Some(AttentionKind::Completion) if !occupied => Some(AttentionKind::Completion),
                _ => None,
            };
            out.push(SurfaceHighlightRegion { rect: r.rect, kind });
        }
    }
    out
}

pub fn draw_surface_highlights(
    ctx: &egui::Context,
    state: &AppState,
    engine: &crate::core::CoreState,
    terminal_rect: PhysicalRect,
    scale_factor: f32,
) {
    let th = theme::theme();
    let regions = regions_from_state(state, engine, terminal_rect, scale_factor);
    let props = SurfaceHighlightsProps {
        theme: &th,
        regions: &regions,
        scale_factor,
    };
    draw_surface_highlights_view(ctx, &props);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PhysicalPx;

    fn mk_rect(x: f32, y: f32, w: f32, h: f32) -> PhysicalRect {
        PhysicalRect {
            x: PhysicalPx(x),
            y: PhysicalPx(y),
            width: PhysicalPx(w),
            height: PhysicalPx(h),
        }
    }

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_view(regions: &[SurfaceHighlightRegion], scale_factor: f32) {
        let ctx = egui::Context::default();
        let theme = test_theme();
        // FullOutput 불필요 — panic-free 만 검증.
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            let props = SurfaceHighlightsProps {
                theme: &theme,
                regions,
                scale_factor,
            };
            draw_surface_highlights_view(ctx, &props);
        }));
    }

    #[test]
    fn view_empty_regions_no_panic() {
        run_view(&[], 1.0);
    }

    #[test]
    fn view_all_unhighlighted_no_panic() {
        let regions = vec![
            SurfaceHighlightRegion {
                rect: mk_rect(0.0, 0.0, 100.0, 50.0),
                kind: None,
            },
            SurfaceHighlightRegion {
                rect: mk_rect(100.0, 0.0, 100.0, 50.0),
                kind: None,
            },
        ];
        run_view(&regions, 1.0);
    }

    #[test]
    fn view_mixed_highlights_no_panic() {
        let regions = vec![
            SurfaceHighlightRegion {
                rect: mk_rect(0.0, 0.0, 100.0, 50.0),
                kind: Some(AttentionKind::Completion),
            },
            SurfaceHighlightRegion {
                rect: mk_rect(100.0, 0.0, 100.0, 50.0),
                kind: None,
            },
            SurfaceHighlightRegion {
                rect: mk_rect(200.0, 0.0, 100.0, 50.0),
                kind: Some(AttentionKind::NeedsInput),
            },
        ];
        run_view(&regions, 1.0);
    }

    #[test]
    fn view_handles_non_unit_scale_factor() {
        let regions = vec![SurfaceHighlightRegion {
            rect: mk_rect(0.0, 0.0, 200.0, 100.0),
            kind: Some(AttentionKind::Completion),
        }];
        run_view(&regions, 2.0);
    }

    #[test]
    fn view_handles_zero_size_rect() {
        let regions = vec![SurfaceHighlightRegion {
            rect: mk_rect(0.0, 0.0, 0.0, 0.0),
            kind: Some(AttentionKind::NeedsInput),
        }];
        run_view(&regions, 1.0);
    }
}
