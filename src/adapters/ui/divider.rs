//! Pane dividers + surface highlight overlay (terminal 위 시각 표시).
//!
//! ## Tier 3 분리 — `draw_surface_highlights`
//!
//! 순수 시각 `draw_surface_highlights_view` 는 [`SurfaceHighlightsProps`] 만
//! 받고 AppState/CoreState/`theme::theme()` 비의존. wrapper
//! `draw_surface_highlights` 는 `state.surface_regions(engine, terminal_rect)`
//! 와 `engine.is_surface_highlighted(id)` 를 호출해 owned
//! `Vec<SurfaceHighlightRegion>` 으로 평탄화한 뒤 view 에 전달.
//!
//! `draw_pane_dividers` 는 이미 단순 (Tier 2) 라 손대지 않음.

use egui::emath::GuiRounding as _;
use tasty_type_appearance::theme::Theme;

use crate::model::PhysicalRect;
use crate::state::AppState;
use crate::theme;

/// Draw pane dividers (borders between split panes).
pub fn draw_pane_dividers(ctx: &egui::Context, dividers: &[PhysicalRect], scale_factor: f32) {
    let th = theme::theme();
    if dividers.is_empty() {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("pane_dividers"),
    ));
    // divergence: pane divider 는 border 인데 surface2(=surface-active/selection 값) 로 그린다.
    // surface2 값을 반환하는 border-role 접근자가 없어(§4-3) 값-보존 위해 surface_active() 사용.
    let border_color = th.surface_active();
    for div in dividers {
        let rect = egui::Rect::from_min_size(
            egui::pos2(div.x.value() / scale_factor, div.y.value() / scale_factor),
            egui::vec2(
                div.width.value() / scale_factor,
                div.height.value() / scale_factor,
            ),
        )
        .round_ui();
        painter.rect_filled(rect, 0.0, border_color);
    }
}

/// View 입력 — 한 surface 의 *물리* 좌표 사각형 + highlight 여부.
///
/// `is_highlighted == false` 인 region 은 view 가 그리지 않는다 (테스트/갤러리
/// 디버깅 시각화 목적의 owned bool — wrapper 가 미리 필터링하지 않음으로써
/// "이 surface 가 후보였으나 highlight 가 꺼져 있음" 같은 mock 상태를 갤러리에서
/// 자연스럽게 표현 가능).
#[derive(Debug, Clone, Copy)]
pub struct SurfaceHighlightRegion {
    pub rect: PhysicalRect,
    pub is_highlighted: bool,
}

/// View 입력 — 전체 surface 평탄화 + theme + scale_factor.
pub struct SurfaceHighlightsProps<'a> {
    pub theme: &'a Theme,
    pub regions: &'a [SurfaceHighlightRegion],
    pub scale_factor: f32,
}

/// Pure 시각 view. AppState/CoreState/`theme::theme()` 비의존.
///
/// `is_highlighted` 가 true 인 region 의 외곽선만 (`accent_primary`, 2px) 그린다.
/// Action 없음 — 상시 그리기 위젯이라 사용자 의도 산출이 없다.
pub fn draw_surface_highlights_view(ctx: &egui::Context, props: &SurfaceHighlightsProps<'_>) {
    if props.regions.iter().all(|r| !r.is_highlighted) {
        return;
    }
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("surface_highlights"),
    ));
    let scale_factor = props.scale_factor;
    let stroke_color = props.theme.accent_primary();
    for region in props.regions {
        if !region.is_highlighted {
            continue;
        }
        let r = region.rect;
        let egui_rect = egui::Rect::from_min_size(
            egui::pos2(r.x.value() / scale_factor, r.y.value() / scale_factor),
            egui::vec2(
                r.width.value() / scale_factor,
                r.height.value() / scale_factor,
            ),
        )
        .round_ui();
        painter.rect_stroke(
            egui_rect,
            0.0,
            egui::Stroke::new(2.0, stroke_color),
            egui::StrokeKind::Inside,
        );
    }
}

/// `state.surface_regions(engine, terminal_rect)` 결과를 view 용
/// `Vec<SurfaceHighlightRegion>` 으로 평탄화. 별도 함수로 분리해 view 와 무관하게
/// 단위 테스트 가능.
pub(crate) fn regions_from_state(
    state: &AppState,
    engine: &crate::core::CoreState,
    terminal_rect: PhysicalRect,
) -> Vec<SurfaceHighlightRegion> {
    let regions = state.surface_regions(engine, terminal_rect);
    let mut out = Vec::new();
    for (_pane_id, _pane_rect, surface_regions) in &regions {
        for r in surface_regions {
            // 우선순위(ADR-0040): 점유(soft/hard) > 완료. 점유 중 surface 는 완료
            // 테두리를 억제한다 — 점유색만 남기고 완료 테두리는 그리지 않는다. 완료의
            // 다른 채널(탭 제목 yellow·워크스페이스 배지)은 불변.
            let occupied = engine.attach.occupancy_of(r.id).is_some();
            out.push(SurfaceHighlightRegion {
                rect: r.rect,
                is_highlighted: engine.is_surface_highlighted(r.id) && !occupied,
            });
        }
    }
    out
}

/// Draw highlight borders around surfaces that have unread notifications.
///
/// Wrapper — state/engine 에서 props 추출 → view 호출. 시그니처는 기존과 동일
/// (외부 호출처 무영향).
pub fn draw_surface_highlights(
    ctx: &egui::Context,
    state: &AppState,
    engine: &crate::core::CoreState,
    terminal_rect: PhysicalRect,
    scale_factor: f32,
) {
    let th = theme::theme();
    let regions = regions_from_state(state, engine, terminal_rect);
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
                is_highlighted: false,
            },
            SurfaceHighlightRegion {
                rect: mk_rect(100.0, 0.0, 100.0, 50.0),
                is_highlighted: false,
            },
        ];
        run_view(&regions, 1.0);
    }

    #[test]
    fn view_mixed_highlights_no_panic() {
        let regions = vec![
            SurfaceHighlightRegion {
                rect: mk_rect(0.0, 0.0, 100.0, 50.0),
                is_highlighted: true,
            },
            SurfaceHighlightRegion {
                rect: mk_rect(100.0, 0.0, 100.0, 50.0),
                is_highlighted: false,
            },
            SurfaceHighlightRegion {
                rect: mk_rect(200.0, 0.0, 100.0, 50.0),
                is_highlighted: true,
            },
        ];
        run_view(&regions, 1.0);
    }

    #[test]
    fn view_handles_non_unit_scale_factor() {
        let regions = vec![SurfaceHighlightRegion {
            rect: mk_rect(0.0, 0.0, 200.0, 100.0),
            is_highlighted: true,
        }];
        run_view(&regions, 2.0);
    }

    #[test]
    fn view_handles_zero_size_rect() {
        let regions = vec![SurfaceHighlightRegion {
            rect: mk_rect(0.0, 0.0, 0.0, 0.0),
            is_highlighted: true,
        }];
        run_view(&regions, 1.0);
    }
}
