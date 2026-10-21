//! 대상 팝업 오버레이 모드 결정과 backend 적용. 계약은 docs/design/systems/webview.md 의
//! "egui 오버레이와 native WebView" 절이다.

/// 대상 팝업 오버레이의 이번 프레임 모드를 정하고 활성 WebView 마다 적용한다. 모든 활성 WebView 가
/// 같은 모드를 내야 하며 하나라도 낮으면 그 모드를 쓴다. 적용이 실패하면 그 프레임부터 구간이 끝날
/// 때까지 `Hide` 이고 경고는 한 번 남긴다. `views` 의 사각형은 활성 WebView 의 물리 사각형이다.
pub(super) fn settle_overlay<B: crate::webview::overlay::WebViewOverlayBackend>(
    frame: &mut crate::state::webview_overlay::OverlayFrame,
    overlay_open: bool,
    blocked: bool,
    views: &[(&B, Option<crate::model::PhysicalRect>)],
    scale_factor: f64,
) -> crate::state::webview_overlay::OverlayMode {
    use crate::state::webview_overlay::{ModeInputs, OverlayMode, select_mode, webview_regions};
    let active: Vec<(&B, crate::model::PhysicalRect)> = views
        .iter()
        .filter_map(|(wv, rect)| rect.map(|r| (*wv, r)))
        .collect();
    let mut mode = OverlayMode::Hide;
    if overlay_open && !active.is_empty() {
        let support = active
            .iter()
            .map(|(wv, _)| wv.overlay_support())
            .min()
            .unwrap_or(OverlayMode::Hide);
        mode = select_mode(ModeInputs {
            support,
            forced: frame.forced,
            blocked,
            failed: frame.health.failed(),
        });
    }
    // 이번 프레임에 적용을 시도한 backend 가 있다. 뒤 WebView 가 실패하면 앞에서 성공한 적용도 되돌린다.
    let mut attempted = false;
    if mode != OverlayMode::Hide {
        for (wv, rect) in &active {
            // native 창은 반올림한 정수 사각형에 놓인다(`set_bounds`). 영역도 같은 기준으로 자른다.
            let px = |v: crate::model::PhysicalPx| crate::model::PhysicalPx(v.value().round());
            let window = crate::model::PhysicalRect {
                x: px(rect.x),
                y: px(rect.y),
                width: px(rect.width),
                height: px(rect.height),
            };
            let regions = webview_regions(mode, window, &frame.targets, scale_factor as f32);
            let request = crate::webview::overlay::OverlayRequest {
                mode,
                regions: &regions,
                host_takes_pointer: crate::state::webview_overlay::input_goes_to_host(
                    mode,
                    overlay_open,
                ),
                scale_factor,
            };
            attempted = true;
            if let Err(e) = wv.apply_overlay(&request) {
                if frame.health.record_failure() {
                    tracing::warn!(
                        "WebView overlay {mode:?} failed, hiding pages under popups: {e}"
                    );
                }
                mode = OverlayMode::Hide;
                break;
            }
        }
    }
    if mode == OverlayMode::Hide && (frame.applied || attempted) {
        for (wv, _) in views {
            wv.clear_overlay();
        }
    }
    if !overlay_open {
        frame.health.end_episode();
    }
    frame.applied = mode != OverlayMode::Hide;
    frame.mode = mode;
    mode
}

/// 드러낼 후보(`planned`)의 표시를 적용하고 실제로 드러낸 surface id 를 돌려준다. 대상 팝업이 떠 있고
/// 모드가 `Hide` 가 아니면 같은 프레임에 오버레이 창이 같은 모드·페이지로 그림을 올렸을 때만
/// 드러낸다. 아니면 모두 숨긴 채 그 모드·페이지를 다음 프레임 그리기에 넘기고 다시 그리기를 요청한다.
pub(super) fn apply_reveal(
    view: &mut crate::view::MainView,
    mode: crate::state::webview_overlay::OverlayMode,
    overlay_open: bool,
    active: &std::collections::HashMap<u32, crate::model::PhysicalRect>,
    mut planned: Vec<u32>,
    scale_factor: f64,
) -> Vec<u32> {
    use crate::state::webview_overlay::{OverlayMode, overlay_reveal_ready};
    planned.sort_unstable();
    let pages: Vec<egui::Rect> = planned
        .iter()
        .filter_map(|sid| active.get(sid))
        .map(|r| {
            let l = r.to_logical(scale_factor as f32);
            egui::Rect::from_min_size(
                egui::pos2(l.x.value(), l.y.value()),
                egui::vec2(l.width.value(), l.height.value()),
            )
        })
        .collect();
    let overlay = &mut view.state.webview_overlay;
    let ready = overlay_reveal_ready(mode, overlay_open, overlay.painted.as_ref(), &pages);
    let gated = overlay_open && mode != OverlayMode::Hide;
    overlay.planned_pages = if gated { pages } else { Vec::new() };
    let revealed = if ready { planned.clone() } else { Vec::new() };
    for (sid, wv) in &view.webviews {
        wv.set_visible(revealed.contains(sid));
    }
    publish_opaque_pages(view, mode, active, &planned, scale_factor);
    if !ready {
        crate::view::ui::View::mark_dirty(view);
    }
    revealed
}

/// 불투명 모드에서 드러낼 페이지를 다음 프레임 그리기에 넘긴다. 바뀌면 상자 가장자리를 다시 그린다.
fn publish_opaque_pages(
    view: &mut crate::view::MainView,
    mode: crate::state::webview_overlay::OverlayMode,
    active: &std::collections::HashMap<u32, crate::model::PhysicalRect>,
    revealed: &[u32],
    scale_factor: f64,
) {
    let logical: Vec<crate::model::LogicalRect> = revealed
        .iter()
        .filter_map(|sid| active.get(sid))
        .map(|r| r.to_logical(scale_factor as f32))
        .collect();
    let pages = crate::state::webview_overlay::opaque_pages(mode, &logical);
    if view.state.webview_overlay.pages != pages {
        view.state.webview_overlay.pages = pages;
        crate::view::ui::View::mark_dirty(view);
    }
}

#[cfg(test)]
mod tests {
    use super::settle_overlay;
    use crate::model::{PhysicalPx, PhysicalRect};
    use crate::state::webview_overlay::{OverlayFrame, OverlayMode, OverlayTarget, TargetKind};
    use crate::webview::overlay::fake::FakeOverlayBackend;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> PhysicalRect {
        PhysicalRect {
            x: PhysicalPx(x),
            y: PhysicalPx(y),
            width: PhysicalPx(w),
            height: PhysicalPx(h),
        }
    }

    fn frame_with(targets: &[(TargetKind, egui::Rect)]) -> OverlayFrame {
        OverlayFrame {
            targets: targets
                .iter()
                .map(|(kind, r)| OverlayTarget {
                    kind: *kind,
                    layer: egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new(r.min.x as i32),
                    ),
                    rect: *r,
                    opaque_rect: *r,
                    paint_rect: *r,
                    scrim: None,
                })
                .collect(),
            ..OverlayFrame::default()
        }
    }

    fn er(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    #[test]
    fn an_unsupported_backend_changes_nothing() {
        let wv = FakeOverlayBackend::default();
        let mut frame = frame_with(&[(TargetKind::HostPopup, er(10.0, 10.0, 50.0, 50.0))]);
        let mode = settle_overlay(
            &mut frame,
            true,
            false,
            &[(&wv, Some(rect(0.0, 0.0, 100.0, 100.0)))],
            1.0,
        );
        assert_eq!(mode, OverlayMode::Hide);
        assert!(wv.applied.borrow().is_empty());
        assert_eq!(*wv.cleared.borrow(), 0);
    }

    /// 지원하는 backend 는 팝업과 파일 피커 경로 드롭다운 상자를 WebView 좌표로 받는다.
    #[test]
    fn a_supporting_backend_receives_popup_and_dropdown_rects() {
        let wv = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            ..FakeOverlayBackend::default()
        };
        let mut frame = frame_with(&[
            (TargetKind::HostPopup, er(300.0, 150.0, 240.0, 160.0)),
            (TargetKind::Dropdown, er(320.0, 280.0, 120.0, 90.0)),
        ]);
        let mode = settle_overlay(
            &mut frame,
            true,
            false,
            &[(&wv, Some(rect(200.0, 100.0, 600.0, 400.0)))],
            1.0,
        );
        assert_eq!(mode, OverlayMode::Opaque);
        assert_eq!(
            *wv.applied.borrow(),
            vec![(
                OverlayMode::Opaque,
                vec![[100, 50, 240, 160], [120, 180, 120, 90]],
                true
            )]
        );
        assert!(frame.applied);
    }

    #[test]
    fn stage_or_tutorial_hides_even_with_a_supporting_backend() {
        let wv = FakeOverlayBackend {
            support: OverlayMode::Transparent,
            ..FakeOverlayBackend::default()
        };
        let mut frame = frame_with(&[(TargetKind::HostPopup, er(0.0, 0.0, 10.0, 10.0))]);
        let mode = settle_overlay(
            &mut frame,
            true,
            true,
            &[(&wv, Some(rect(0.0, 0.0, 100.0, 100.0)))],
            1.0,
        );
        assert_eq!(mode, OverlayMode::Hide);
        assert!(wv.applied.borrow().is_empty());
    }

    /// 적용이 실패하면 그 프레임부터 숨기고, 구간이 끝날 때까지 다시 시도하지 않는다.
    #[test]
    fn an_apply_failure_hides_from_that_frame_for_the_episode() {
        let wv = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            fail: true,
            ..FakeOverlayBackend::default()
        };
        let mut frame = frame_with(&[(TargetKind::HostPopup, er(0.0, 0.0, 10.0, 10.0))]);
        let views = [(&wv, Some(rect(0.0, 0.0, 100.0, 100.0)))];
        assert_eq!(
            settle_overlay(&mut frame, true, false, &views, 1.0),
            OverlayMode::Hide
        );
        assert!(frame.health.failed());
        assert_eq!(
            settle_overlay(&mut frame, true, false, &views, 1.0),
            OverlayMode::Hide
        );
        // 팝업이 닫히면 구간이 끝나 다음에는 다시 시도한다.
        settle_overlay(&mut frame, false, false, &views, 1.0);
        assert!(!frame.health.failed());
    }

    #[test]
    fn closing_the_popup_clears_an_applied_overlay() {
        let wv = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            ..FakeOverlayBackend::default()
        };
        let mut frame = frame_with(&[(TargetKind::HostPopup, er(0.0, 0.0, 10.0, 10.0))]);
        let views = [(&wv, Some(rect(0.0, 0.0, 100.0, 100.0)))];
        settle_overlay(&mut frame, true, false, &views, 1.0);
        assert_eq!(*wv.cleared.borrow(), 0);
        assert_eq!(
            settle_overlay(&mut frame, false, false, &views, 1.0),
            OverlayMode::Hide
        );
        assert_eq!(*wv.cleared.borrow(), 1);
        assert!(!frame.applied);
    }

    /// 첫 적용에서 앞 WebView 가 성공한 뒤 다음 WebView 가 실패하면 앞의 적용도 그 프레임에 되돌린다.
    #[test]
    fn a_partial_first_apply_is_cleared_when_a_later_webview_fails() {
        let ok = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            ..FakeOverlayBackend::default()
        };
        let bad = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            fail: true,
            ..FakeOverlayBackend::default()
        };
        let mut frame = frame_with(&[(TargetKind::HostPopup, er(0.0, 0.0, 10.0, 10.0))]);
        let views = [
            (&ok, Some(rect(0.0, 0.0, 100.0, 100.0))),
            (&bad, Some(rect(100.0, 0.0, 100.0, 100.0))),
        ];
        assert_eq!(
            settle_overlay(&mut frame, true, false, &views, 1.0),
            OverlayMode::Hide
        );
        assert_eq!(ok.applied.borrow().len(), 1);
        assert!(*ok.cleared.borrow() >= 1);
        assert!(!*ok.pointer_held.borrow());
        assert!(!frame.applied);
        settle_overlay(&mut frame, false, false, &views, 1.0);
        assert!(!*ok.pointer_held.borrow());
    }

    /// 활성 WebView 하나라도 지원하지 않으면 모두 숨긴다(지금 동작).
    #[test]
    fn the_weakest_active_webview_decides_the_mode() {
        let good = FakeOverlayBackend {
            support: OverlayMode::Transparent,
            ..FakeOverlayBackend::default()
        };
        let none = FakeOverlayBackend::default();
        let mut frame = frame_with(&[(TargetKind::HostPopup, er(0.0, 0.0, 10.0, 10.0))]);
        let mode = settle_overlay(
            &mut frame,
            true,
            false,
            &[
                (&good, Some(rect(0.0, 0.0, 100.0, 100.0))),
                (&none, Some(rect(100.0, 0.0, 100.0, 100.0))),
            ],
            1.0,
        );
        assert_eq!(mode, OverlayMode::Hide);
        assert!(good.applied.borrow().is_empty());
    }
}
