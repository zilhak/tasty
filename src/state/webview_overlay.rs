//! egui 오버레이와 native WebView 의 공통 판정. 플랫폼 호출 없이 모드·대상 영역·표시 여부를 정한다.
//! 계약은 docs/design/systems/webview.md 의 "egui 오버레이와 native WebView" 절이다.
//!
//! - 모드는 `Hide < Opaque < Transparent` 다. backend 가 낼 수 있는 모드와 차단 조건(전체화면
//!   무대·튜토리얼, 적용 실패)으로 정한다. `Hide` 는 지금까지의 동작(모든 WebView 숨김)이다.
//! - 대상은 host 의 non-passive 팝업, plugin 팝업, 팝업 안에서 연 드롭다운이다. 토스트와 passive
//!   팝업은 대상이 아니며 겹친 WebView 만 숨기는 기존 규칙을 따른다.
//! - 대상 영역은 egui 논리 좌표로 모았다가 WebView 창 기준 물리 정수 사각형으로 바꿔 backend 에 넘긴다.

use crate::model::{LogicalPx, LogicalRect, PhysicalRect};

/// 대상 팝업이 떠 있는 동안 native WebView 를 다루는 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum OverlayMode {
    /// 모든 WebView 를 숨기고 그 자리에 경계 타일을 그린다. 지원하지 않는 OS 의 동작이다.
    #[default]
    Hide,
    /// 페이지를 보인 채 host 가 팝업·드롭다운 상자 안만 불투명하게 칠한다.
    Opaque,
    /// 페이지를 보인 채 host 가 투명도를 섞어 그 위에 합성한다.
    Transparent,
}

/// 대상 레이어의 종류. 오버레이 그림과 대상 영역은 이 목록의 레이어만 모은다. 대상 레이어 목록은
/// 이 enum 하나이며 그리는 곳과의 일치는 `source_guards::webview_overlay_targets` 가 검사한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// `PopupManager::draw` 가 그린 non-passive host 팝업.
    HostPopup,
    /// `draw_plugin_popups` 가 그린 plugin 팝업 셸.
    PluginPopup,
    /// 팝오버 래퍼(`tasty_egui_theme::with_popover_frame`)가 그린 egui 드롭다운.
    Dropdown,
}

/// 이번 프레임에 보이는 대상 하나(egui 논리 좌표).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayTarget {
    pub kind: TargetKind,
    pub layer: egui::LayerId,
    /// 상자 사각형. 입력 판정과 가장자리(각진 모서리) 판정의 기준이다.
    pub rect: egui::Rect,
    /// 불투명 모드에서 host 가 칠하는 영역. 상자 바깥에 그리는 테두리까지 넣는다([`outlined_box`]).
    pub opaque_rect: egui::Rect,
    /// 상자 바깥으로 번지는 그림(그림자)까지 넣은 사각형. 투명 모드의 합성 영역이다.
    pub paint_rect: egui::Rect,
    /// 이 대상이 그린 scrim 범위. 불투명 모드에서는 scrim 을 그리지 않으므로 쓰지 않는다.
    pub scrim: Option<egui::Rect>,
}

/// 창 하나의 오버레이 상태. egui 프레임이 대상을 모으고, WebView 동기화가 모드와 페이지를 정한다.
/// 상자 가장자리와 오버레이 그림은 다음 프레임이 `pages`·`planned_pages` 로 정한다. 그래서 대상이
/// 처음 뜬 프레임에는 그림이 아직 없고, 동기화는 [`overlay_reveal_ready`] 가 참이 될 때까지 페이지를
/// 숨긴 채 다시 그리기를 요청한다.
#[derive(Debug, Default)]
pub struct OverlayFrame {
    /// 이번 프레임에 그린 대상.
    pub targets: Vec<OverlayTarget>,
    /// 마지막 동기화가 정한 모드.
    pub mode: OverlayMode,
    /// 불투명 모드에서 드러낼 페이지(egui 논리 좌표). 다른 모드에서는 비어 있다.
    pub pages: Vec<egui::Rect>,
    /// 마지막 동기화가 이 모드로 드러내려는 페이지(egui 논리 좌표, surface id 순). 오버레이 그림을
    /// 이 영역으로 자른다. `Hide` 이거나 대상 팝업이 없으면 비어 있다.
    pub planned_pages: Vec<egui::Rect>,
    /// 직전 프레임 plugin 팝업 셸 레이어와 영역.
    pub prev_plugin_rects: Vec<(egui::LayerId, egui::Rect)>,
    pub health: OverlayHealth,
    /// backend 에 오버레이를 적용한 상태다. `Hide` 로 돌아가면 `clear_overlay` 를 부른다.
    pub applied: bool,
    /// 이번 프레임 오버레이 창에 그릴 그림. 오버레이 창이 있고 모드가 `Hide` 가 아닐 때만 프레임 끝에
    /// 채우고 렌더가 가져간다. `None` 은 준비하지 않은 것이고 빈 `shapes` 는 유효한 빈 그림이다.
    pub paint_job: Option<OverlayPaintJob>,
    /// 이번 프레임에 오버레이 창에 실제로 올린 그림. 프레임 시작에 비우고 렌더가 present 뒤에 채운다.
    pub painted: Option<PaintedOverlay>,
    /// debug 빌드에서 backend 지원 모드를 덮어쓴다.
    pub forced: Option<OverlayMode>,
}

/// 오버레이 창 한 프레임의 그림. 어떤 모드·페이지를 기준으로 만들었는지 함께 든다.
#[derive(Debug, Default)]
pub struct OverlayPaintJob {
    pub mode: OverlayMode,
    pub pages: Vec<egui::Rect>,
    pub shapes: Vec<egui::epaint::ClippedShape>,
}

/// 오버레이 창에 present 한 그림의 기준.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintedOverlay {
    pub mode: OverlayMode,
    pub pages: Vec<egui::Rect>,
}

/// 모드를 정하는 입력. 매 프레임 새로 만든다.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModeInputs {
    /// backend 가 이 창에서 낼 수 있는 가장 좋은 모드. 지원하지 않으면 `Hide`.
    pub support: OverlayMode,
    /// debug 빌드에서만 쓰는 지원 모드 덮어쓰기.
    pub forced: Option<OverlayMode>,
    /// 전체화면 무대나 튜토리얼이 떠 있다.
    pub blocked: bool,
    /// 이번 오버레이 구간에서 backend 적용이 실패했다.
    pub failed: bool,
}

pub fn select_mode(inputs: ModeInputs) -> OverlayMode {
    if inputs.blocked || inputs.failed {
        return OverlayMode::Hide;
    }
    inputs.forced.unwrap_or(inputs.support)
}

/// 이 WebView 를 드러낼지. `overlay_open` 은 `has_egui_overlay_open()` 그대로이며, 모드가 `Hide` 가
/// 아니면 대상 팝업이 떠 있어도 숨기지 않는다. 토스트·passive 팝업 규칙(`under_toast`)은 그대로다.
pub fn reveal_wanted(
    mode: OverlayMode,
    overlay_open: bool,
    active: bool,
    under_toast: bool,
) -> bool {
    active && !under_toast && (!overlay_open || mode != OverlayMode::Hide)
}

/// 대상 팝업이 떠 있는 동안 `mode` 로 페이지를 드러내도 되는지. 같은 프레임에 오버레이 창이 같은 모드와
/// 같은 페이지로 그림을 올렸을 때만 참이다. 팝업이 없거나 `Hide` 면 이 판정과 무관하다.
pub fn overlay_reveal_ready(
    mode: OverlayMode,
    overlay_open: bool,
    painted: Option<&PaintedOverlay>,
    planned_pages: &[egui::Rect],
) -> bool {
    if !overlay_open || mode == OverlayMode::Hide {
        return true;
    }
    painted.is_some_and(|p| p.mode == mode && p.pages == planned_pages)
}

/// 상자 바깥(`StrokeKind::Outside`)에 그리는 테두리까지 넣은 그림 영역. 소속 범위 밖은 자른다.
pub fn outlined_box(rect: egui::Rect, outside_stroke: f32, scope: egui::Rect) -> egui::Rect {
    rect.expand(outside_stroke).intersect(scope)
}

/// 불투명 모드에서 상자 가장자리를 정할 페이지 영역(egui 논리 좌표). 다른 모드에서는 비운다.
pub fn opaque_pages(mode: OverlayMode, revealed: &[LogicalRect]) -> Vec<egui::Rect> {
    if mode != OverlayMode::Opaque {
        return Vec::new();
    }
    revealed.iter().map(|r| to_egui(*r)).collect()
}

/// backend 에 넘길 대상 영역(WebView 창 기준 물리 정수 사각형). 불투명 모드는 상자와 바깥 테두리만, 투명 모드는
/// 그림자와 scrim 범위까지 넣는다. `webview` 는 `set_bounds` 와 같이 반올림한 창 사각형이다.
pub fn webview_regions(
    mode: OverlayMode,
    webview: PhysicalRect,
    targets: &[OverlayTarget],
    scale_factor: f32,
) -> Vec<PhysicalRect> {
    let logical: Vec<egui::Rect> = match mode {
        OverlayMode::Hide => return Vec::new(),
        OverlayMode::Opaque => targets.iter().map(|t| t.opaque_rect).collect(),
        OverlayMode::Transparent => targets
            .iter()
            .flat_map(|t| std::iter::once(t.paint_rect).chain(t.scrim))
            .collect(),
    };
    let physical: Vec<PhysicalRect> = logical
        .into_iter()
        .map(|r| from_egui(r).to_physical(scale_factor))
        .collect();
    crate::state::webview_edges::notice_clip_holes(webview, &physical)
}

/// 대상 팝업이 떠 있는 동안 WebView 영역 위 클릭·휠은 host 가 받는다. 페이지에는 가지 않는다.
pub fn input_goes_to_host(mode: OverlayMode, overlay_open: bool) -> bool {
    overlay_open && mode != OverlayMode::Hide
}

/// 적용 실패를 오버레이 구간 단위로 기억한다. 실패하면 그 프레임부터 구간이 끝날 때까지 `Hide` 이고,
/// 경고는 구간마다 한 번 남긴다. 구간은 대상 팝업이 모두 닫히면 끝난다.
#[derive(Debug, Default)]
pub struct OverlayHealth {
    failed: bool,
}

impl OverlayHealth {
    pub fn failed(&self) -> bool {
        self.failed
    }

    /// 실패를 기록한다. 이 구간의 첫 실패면 참을 돌려준다(호출자가 경고를 남긴다).
    pub fn record_failure(&mut self) -> bool {
        !std::mem::replace(&mut self.failed, true)
    }

    /// 오버레이가 닫힌 프레임에 부른다. 다음 구간은 다시 backend 를 시도한다.
    pub fn end_episode(&mut self) {
        self.failed = false;
    }
}

/// plugin 팝업은 host 가 직전 프레임 영역으로 입력을 판정한다. 움직인 프레임에도 상자가 비지 않도록
/// 같은 레이어의 직전 영역과 현재 영역을 합친 사각형을 대상으로 둔다.
pub fn plugin_box(
    layer: egui::LayerId,
    current: egui::Rect,
    previous: &[(egui::LayerId, egui::Rect)],
) -> egui::Rect {
    previous
        .iter()
        .filter(|(l, _)| *l == layer)
        .fold(current, |acc, (_, r)| acc.union(*r))
}

/// 프레임 시작에 부른다. 지난 프레임 대상·그림 기록과 드롭다운 기록을 지우고 이번 프레임 그리기가 쓸 페이지를 넣는다.
pub fn begin_frame(ctx: &egui::Context, frame: &mut OverlayFrame) {
    frame.targets.clear();
    frame.paint_job = None;
    frame.painted = None;
    tasty_egui_theme::overlay_edge::clear_frame_popovers(ctx);
    tasty_egui_theme::overlay_edge::set_opaque_pages(ctx, frame.pages.clone());
}

/// 프레임 끝에 부른다. 열린 egui 드롭다운(팝업 레이어)을 대상에 더한다. 팝오버 래퍼가 기록한
/// 드롭다운은 그때 정한 가장자리를 쓰고, 래퍼 밖에서 연 드롭다운도 열려 있으면 대상으로 둔다.
pub fn end_frame(ctx: &egui::Context, frame: &mut OverlayFrame) {
    let records = tasty_egui_theme::overlay_edge::frame_popovers(ctx);
    let shadow = ctx.style().visuals.popup_shadow;
    let open: Vec<(egui::LayerId, egui::Rect)> = ctx.memory(|m| {
        m.layer_ids()
            .filter(|l| l.order == egui::Order::Foreground && m.is_popup_open(l.id))
            .filter_map(|l| m.area_rect(l.id).map(|r| (l, r)))
            .collect()
    });
    for (layer, rect) in open {
        let shadow = if records.iter().any(|r| r.layer == layer && r.edge.opaque) {
            egui::Shadow::NONE
        } else {
            shadow
        };
        frame.targets.push(OverlayTarget {
            kind: TargetKind::Dropdown,
            layer,
            rect,
            opaque_rect: rect,
            paint_rect: rect + shadow.margin(),
            scrim: None,
        });
    }
}

fn to_egui(r: LogicalRect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(r.x.value(), r.y.value()),
        egui::vec2(r.width.value(), r.height.value()),
    )
}

fn from_egui(r: egui::Rect) -> LogicalRect {
    LogicalRect {
        x: LogicalPx(r.min.x),
        y: LogicalPx(r.min.y),
        width: LogicalPx(r.width()),
        height: LogicalPx(r.height()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PhysicalPx;

    fn er(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    fn pr(x: f32, y: f32, w: f32, h: f32) -> PhysicalRect {
        PhysicalRect {
            x: PhysicalPx(x),
            y: PhysicalPx(y),
            width: PhysicalPx(w),
            height: PhysicalPx(h),
        }
    }

    fn values(rects: &[PhysicalRect]) -> Vec<[f32; 4]> {
        rects
            .iter()
            .map(|r| [r.x.value(), r.y.value(), r.width.value(), r.height.value()])
            .collect()
    }

    fn target(rect: egui::Rect, paint: egui::Rect, scrim: Option<egui::Rect>) -> OverlayTarget {
        OverlayTarget {
            kind: TargetKind::HostPopup,
            layer: egui::LayerId::new(egui::Order::Foreground, egui::Id::new("t")),
            rect,
            opaque_rect: rect,
            paint_rect: paint,
            scrim,
        }
    }

    /// 바깥 1px 테두리가 불투명 영역 안에 들어간다. 입력 판정 상자(`rect`)는 그대로다.
    #[test]
    fn the_opaque_region_keeps_the_outside_border() {
        let popup = er(100.0, 100.0, 100.0, 100.0);
        let scope = er(0.0, 0.0, 400.0, 300.0);
        let t = OverlayTarget {
            opaque_rect: outlined_box(popup, 1.0, scope),
            ..target(popup, popup.expand(8.0), None)
        };
        let webview = pr(0.0, 0.0, 400.0, 300.0);
        let at1 = webview_regions(OverlayMode::Opaque, webview, &[t], 1.0);
        assert_eq!(values(&at1), vec![[99.0, 99.0, 102.0, 102.0]]);
        let at2 = webview_regions(OverlayMode::Opaque, pr(0.0, 0.0, 800.0, 600.0), &[t], 2.0);
        assert_eq!(values(&at2), vec![[198.0, 198.0, 204.0, 204.0]]);
        assert_eq!(t.rect, popup);
        // 소속 범위 끝에 붙은 상자는 범위 밖 테두리를 칠하지 않는다.
        assert_eq!(
            outlined_box(er(0.0, 0.0, 50.0, 50.0), 1.0, scope),
            er(0.0, 0.0, 51.0, 51.0)
        );
    }

    /// 대상 팝업이 처음 뜬 프레임에는 오버레이 그림이 없으므로 페이지를 드러내지 않는다. 같은 모드·페이지로
    /// 그린 뒤에만 드러낸다. 빈 그림도 그 모드로 올렸다면 준비된 것이다.
    #[test]
    fn pages_wait_for_an_overlay_frame_painted_for_the_same_mode_and_pages() {
        let page = er(0.0, 0.0, 300.0, 200.0);
        let painted = |mode, pages: &[egui::Rect]| PaintedOverlay {
            mode,
            pages: pages.to_vec(),
        };
        assert!(!overlay_reveal_ready(
            OverlayMode::Opaque,
            true,
            None,
            &[page]
        ));
        assert!(!overlay_reveal_ready(
            OverlayMode::Transparent,
            true,
            Some(&painted(OverlayMode::Opaque, &[page])),
            &[page]
        ));
        assert!(!overlay_reveal_ready(
            OverlayMode::Opaque,
            true,
            Some(&painted(OverlayMode::Opaque, &[er(0.0, 0.0, 100.0, 100.0)])),
            &[page]
        ));
        assert!(overlay_reveal_ready(
            OverlayMode::Opaque,
            true,
            Some(&painted(OverlayMode::Opaque, &[page])),
            &[page]
        ));
        assert!(overlay_reveal_ready(
            OverlayMode::Transparent,
            true,
            Some(&painted(OverlayMode::Transparent, &[])),
            &[]
        ));
        // 팝업이 없거나 숨김 모드면 기존 표시 판정 그대로다.
        assert!(overlay_reveal_ready(
            OverlayMode::Opaque,
            false,
            None,
            &[page]
        ));
        assert!(overlay_reveal_ready(OverlayMode::Hide, true, None, &[page]));
    }

    #[test]
    fn an_unsupported_backend_keeps_the_current_hide_behavior() {
        let mode = select_mode(ModeInputs::default());
        assert_eq!(mode, OverlayMode::Hide);
        // 대상 팝업이 뜨면 활성 WebView 도 숨긴다 — 지금 동작 그대로다.
        assert!(!reveal_wanted(mode, true, true, false));
        assert!(reveal_wanted(mode, false, true, false));
        assert!(webview_regions(mode, pr(0.0, 0.0, 100.0, 100.0), &[], 1.0).is_empty());
        assert!(!input_goes_to_host(mode, true));
    }

    #[test]
    fn a_supporting_backend_keeps_the_webview_visible_under_a_popup() {
        let mode = select_mode(ModeInputs {
            support: OverlayMode::Opaque,
            ..ModeInputs::default()
        });
        assert_eq!(mode, OverlayMode::Opaque);
        assert!(reveal_wanted(mode, true, true, false));
        assert!(input_goes_to_host(mode, true));
        // 비활성 탭과 토스트 규칙은 그대로다.
        assert!(!reveal_wanted(mode, true, false, false));
        assert!(!reveal_wanted(mode, true, true, true));
    }

    #[test]
    fn stage_tutorial_and_failure_fall_back_to_hide() {
        for inputs in [
            ModeInputs {
                support: OverlayMode::Transparent,
                blocked: true,
                ..ModeInputs::default()
            },
            ModeInputs {
                support: OverlayMode::Transparent,
                failed: true,
                ..ModeInputs::default()
            },
            ModeInputs {
                forced: Some(OverlayMode::Opaque),
                blocked: true,
                ..ModeInputs::default()
            },
        ] {
            assert_eq!(select_mode(inputs), OverlayMode::Hide);
        }
    }

    #[test]
    fn the_debug_override_replaces_the_backend_report() {
        let mode = select_mode(ModeInputs {
            support: OverlayMode::Hide,
            forced: Some(OverlayMode::Opaque),
            ..ModeInputs::default()
        });
        assert_eq!(mode, OverlayMode::Opaque);
    }

    /// 팝업과 드롭다운 상자는 WebView 창 기준 좌표, 바깥쪽 정수 픽셀로 간다. scrim 은 넘기지 않는다.
    #[test]
    fn opaque_regions_are_the_boxes_in_webview_coordinates() {
        let webview = pr(200.0, 100.0, 600.0, 400.0);
        let popup = target(
            er(300.0, 150.0, 240.0, 160.0),
            er(280.0, 150.0, 280.0, 200.0),
            Some(er(0.0, 0.0, 1000.0, 800.0)),
        );
        let dropdown = OverlayTarget {
            kind: TargetKind::Dropdown,
            ..target(
                er(320.5, 290.25, 100.0, 300.0),
                er(320.0, 290.0, 100.0, 300.0),
                None,
            )
        };
        let regions = webview_regions(OverlayMode::Opaque, webview, &[popup, dropdown], 1.0);
        assert_eq!(
            values(&regions),
            vec![
                [100.0, 50.0, 240.0, 160.0],
                // 아래로 WebView 밖까지 늘어진 부분은 잘린다.
                [120.0, 190.0, 101.0, 210.0],
            ]
        );
    }

    #[test]
    fn transparent_regions_include_the_shadow_and_scrim() {
        let webview = pr(0.0, 0.0, 400.0, 300.0);
        let popup = target(
            er(100.0, 100.0, 50.0, 50.0),
            er(90.0, 95.0, 70.0, 70.0),
            Some(er(0.0, 0.0, 400.0, 300.0)),
        );
        let regions = webview_regions(OverlayMode::Transparent, webview, &[popup], 1.0);
        assert_eq!(
            values(&regions),
            vec![[90.0, 95.0, 70.0, 70.0], [0.0, 0.0, 400.0, 300.0]]
        );
    }

    /// 배율이 바뀌면 같은 논리 상자가 다른 물리 사각형이 된다. 매 프레임 현재 배율로 다시 계산한다.
    #[test]
    fn regions_follow_the_scale_factor() {
        let webview = pr(0.0, 0.0, 2000.0, 2000.0);
        let popup = target(
            er(10.0, 10.0, 100.0, 50.0),
            er(10.0, 10.0, 100.0, 50.0),
            None,
        );
        let at1 = webview_regions(OverlayMode::Opaque, webview, &[popup], 1.0);
        let at2 = webview_regions(OverlayMode::Opaque, webview, &[popup], 2.0);
        assert_eq!(values(&at1), vec![[10.0, 10.0, 100.0, 50.0]]);
        assert_eq!(values(&at2), vec![[20.0, 20.0, 200.0, 100.0]]);
    }

    #[test]
    fn a_failure_warns_once_per_episode() {
        let mut health = OverlayHealth::default();
        assert!(health.record_failure());
        assert!(!health.record_failure());
        assert!(health.failed());
        health.end_episode();
        assert!(!health.failed());
        assert!(health.record_failure());
    }

    #[test]
    fn a_plugin_box_covers_its_previous_and_current_frame() {
        let mine = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("p1"));
        let other = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("p2"));
        let prev = er(0.0, 0.0, 10.0, 10.0);
        let now = er(5.0, 0.0, 10.0, 10.0);
        assert_eq!(
            plugin_box(
                mine,
                now,
                &[(mine, prev), (other, er(100.0, 0.0, 1.0, 1.0))]
            ),
            er(0.0, 0.0, 15.0, 10.0)
        );
        assert_eq!(plugin_box(mine, now, &[]), now);
    }

    #[test]
    fn opaque_pages_are_empty_outside_the_opaque_mode() {
        let page = LogicalRect {
            x: LogicalPx(1.0),
            y: LogicalPx(2.0),
            width: LogicalPx(3.0),
            height: LogicalPx(4.0),
        };
        assert!(opaque_pages(OverlayMode::Hide, &[page]).is_empty());
        assert!(opaque_pages(OverlayMode::Transparent, &[page]).is_empty());
        assert_eq!(
            opaque_pages(OverlayMode::Opaque, &[page]),
            vec![er(1.0, 2.0, 3.0, 4.0)]
        );
    }
}
