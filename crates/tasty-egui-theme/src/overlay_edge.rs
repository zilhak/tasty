//! 불투명 모드 native WebView 페이지와 겹친 상자의 가장자리와, 팝업 안에서 열린 드롭다운 기록.
//! 계약은 docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절이다.
//!
//! host 는 프레임마다 보이는 불투명 페이지 영역을 [`set_opaque_pages`] 로 넣는다. 상자는 자기
//! 사각형이 페이지와 겹치거나 부모 셸이 불투명이면 상자 전체가 각진 모서리·그림자 없음이 된다.
//! 드롭다운은 [`crate::with_popover_frame`] 이 열어 이 판정과 부모 레이어 기록을 함께 처리한다.

use tasty_type_appearance::theme::{ShadowToken, Theme};
use tasty_type_geometry::length::LogicalPx;

/// 상자 하나의 가장자리. 변마다 따로 정하지 않고 상자 전체에 한 번 적용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BoxEdge {
    pub opaque: bool,
}

impl BoxEdge {
    pub const NORMAL: Self = Self { opaque: false };
    pub const OPAQUE: Self = Self { opaque: true };

    /// 불투명이면 `webview-opaque-radius`(0), 아니면 상자가 원래 쓰는 모서리.
    pub fn radius(self, theme: &Theme, normal: LogicalPx) -> LogicalPx {
        if self.opaque {
            theme.webview_opaque_radius()
        } else {
            normal
        }
    }

    /// 불투명이면 `webview-opaque-shadow`(→ shadow-none), 아니면 상자가 원래 쓰는 그림자.
    pub fn shadow(self, theme: &Theme, normal: ShadowToken) -> ShadowToken {
        if self.opaque {
            theme.shadow_none()
        } else {
            normal
        }
    }
}

/// 두 사각형이 면적을 공유하는지. 변이 맞닿기만 하면 겹침이 아니다.
pub fn rects_overlap(a: egui::Rect, b: egui::Rect) -> bool {
    a.min.x < b.max.x && b.min.x < a.max.x && a.min.y < b.max.y && b.min.y < a.max.y
}

/// 상자 사각형과 부모 셸 판정으로 가장자리를 정한다.
pub fn edge_for(rect: egui::Rect, parent_opaque: bool, pages: &[egui::Rect]) -> BoxEdge {
    BoxEdge {
        opaque: parent_opaque || pages.iter().any(|p| rects_overlap(rect, *p)),
    }
}

/// scrim 범위 안에 불투명 페이지가 보이면 그 scrim 은 그리지 않는다. 페이지 밖만 어둡게 하는
/// 얼룩을 만들지 않기 위해 범위 전체를 생략한다.
pub fn scrim_skipped(scope: egui::Rect, pages: &[egui::Rect]) -> bool {
    pages.iter().any(|p| rects_overlap(scope, *p))
}

fn pages_id() -> egui::Id {
    egui::Id::new("tasty.webview_overlay.opaque_pages")
}

fn parent_id() -> egui::Id {
    egui::Id::new("tasty.webview_overlay.opaque_parent")
}

fn popovers_id() -> egui::Id {
    egui::Id::new("tasty.webview_overlay.popovers")
}

/// 이번 프레임에 보이는 불투명 모드 페이지 영역(egui 논리 좌표). 비우면 모든 상자가 원래 모양이다.
pub fn set_opaque_pages(ctx: &egui::Context, pages: Vec<egui::Rect>) {
    ctx.data_mut(|d| d.insert_temp(pages_id(), pages));
}

pub fn opaque_pages(ctx: &egui::Context) -> Vec<egui::Rect> {
    ctx.data(|d| d.get_temp::<Vec<egui::Rect>>(pages_id()))
        .unwrap_or_default()
}

/// 불투명 셸의 콘텐츠를 그리는 동안 켜 둔다. 그 안에서 열린 드롭다운이 부모를 따른다.
pub fn with_opaque_parent<R>(ctx: &egui::Context, opaque: bool, f: impl FnOnce() -> R) -> R {
    let prev = opaque_parent(ctx);
    ctx.data_mut(|d| d.insert_temp(parent_id(), opaque));
    let out = f();
    ctx.data_mut(|d| d.insert_temp(parent_id(), prev));
    out
}

pub fn opaque_parent(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(parent_id()))
        .unwrap_or(false)
}

/// 팝오버 래퍼가 그린 egui 드롭다운 하나. `parent` 는 그 드롭다운을 연 위젯의 레이어다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopoverRecord {
    pub layer: egui::LayerId,
    pub parent: egui::LayerId,
    pub edge: BoxEdge,
}

/// 프레임 시작에 지운다. 그 프레임에 그린 드롭다운만 남아야 한다.
pub fn clear_frame_popovers(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(popovers_id(), Vec::<PopoverRecord>::new()));
}

/// 이번 프레임에 팝오버 래퍼가 그린 드롭다운. 영역은 `Memory::area_rect(layer.id)` 로 읽는다.
pub fn frame_popovers(ctx: &egui::Context) -> Vec<PopoverRecord> {
    ctx.data(|d| d.get_temp::<Vec<PopoverRecord>>(popovers_id()))
        .unwrap_or_default()
}

/// 열린 egui 팝업 레이어. egui 팝업은 Area id 가 팝업 id 라 그 id 로 열림을 확인한다.
pub(crate) fn open_popup_layers(ctx: &egui::Context) -> Vec<egui::LayerId> {
    ctx.memory(|m| {
        m.layer_ids()
            .filter(|l| l.order == egui::Order::Foreground && m.is_popup_open(l.id))
            .collect()
    })
}

/// 팝오버 래퍼 호출 전 각 열린 팝업 레이어의 그림 수. 호출 뒤 늘었으면 그 호출이 그린 것이다.
pub(crate) fn paint_counts(ctx: &egui::Context, layers: &[egui::LayerId]) -> Vec<usize> {
    ctx.graphics(|g| {
        layers
            .iter()
            .map(|l| g.get(*l).map_or(0, |list| list.next_idx().0))
            .collect()
    })
}

/// 래퍼 호출이 그린 드롭다운을 기록한다.
pub(crate) fn record_drawn(
    ctx: &egui::Context,
    parent: egui::LayerId,
    edge: BoxEdge,
    layers: &[egui::LayerId],
    before: &[usize],
) {
    let after = paint_counts(ctx, layers);
    let drawn: Vec<PopoverRecord> = layers
        .iter()
        .zip(before.iter().zip(&after))
        .filter(|(_, (b, a))| a > b)
        .map(|(layer, _)| PopoverRecord {
            layer: *layer,
            parent,
            edge,
        })
        .collect();
    if drawn.is_empty() {
        return;
    }
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<Vec<PopoverRecord>>(popovers_id());
        for r in drawn {
            if !list.iter().any(|x| x.layer == r.layer) {
                list.push(r);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    #[test]
    fn a_box_touching_a_page_edge_keeps_its_normal_edge() {
        let page = r(100.0, 0.0, 200.0, 200.0);
        assert!(!edge_for(r(0.0, 0.0, 100.0, 50.0), false, &[page]).opaque);
        assert!(edge_for(r(0.0, 0.0, 101.0, 50.0), false, &[page]).opaque);
    }

    #[test]
    fn a_dropdown_follows_an_opaque_parent_even_off_the_page() {
        let page = r(100.0, 0.0, 200.0, 200.0);
        let off_page = r(400.0, 0.0, 50.0, 50.0);
        assert!(!edge_for(off_page, false, &[page]).opaque);
        assert!(edge_for(off_page, true, &[page]).opaque);
    }

    #[test]
    fn no_pages_means_every_box_is_normal_and_every_scrim_draws() {
        assert!(!edge_for(r(0.0, 0.0, 10.0, 10.0), false, &[]).opaque);
        assert!(!scrim_skipped(r(0.0, 0.0, 10.0, 10.0), &[]));
    }

    #[test]
    fn a_scrim_scope_containing_any_page_is_skipped_whole() {
        let window = r(0.0, 0.0, 1000.0, 800.0);
        let page = r(500.0, 100.0, 300.0, 300.0);
        assert!(scrim_skipped(window, &[page]));
        let other_surface = r(0.0, 0.0, 400.0, 800.0);
        assert!(!scrim_skipped(other_surface, &[page]));
    }

    #[test]
    fn the_opaque_edge_reads_the_two_tokens() {
        let theme = tasty_themes::mocha_fallback();
        let normal = theme.corner_radius;
        assert_eq!(BoxEdge::NORMAL.radius(&theme, normal), normal);
        assert_eq!(
            BoxEdge::OPAQUE.radius(&theme, normal),
            theme.webview_opaque_radius()
        );
        assert_eq!(
            BoxEdge::OPAQUE.shadow(&theme, theme.shadow_modal()),
            theme.shadow_none()
        );
        assert_eq!(
            BoxEdge::NORMAL.shadow(&theme, theme.shadow_modal()),
            theme.shadow_modal()
        );
    }

    #[test]
    fn the_parent_flag_is_restored_after_the_scope() {
        let ctx = egui::Context::default();
        assert!(!opaque_parent(&ctx));
        with_opaque_parent(&ctx, true, || {
            assert!(opaque_parent(&ctx));
            with_opaque_parent(&ctx, false, || assert!(!opaque_parent(&ctx)));
            assert!(opaque_parent(&ctx));
        });
        assert!(!opaque_parent(&ctx));
    }
}
