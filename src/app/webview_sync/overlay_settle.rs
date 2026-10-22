//! 대상 팝업 오버레이 모드 결정과 backend 적용. 계약은 docs/design/systems/webview.md 의
//! "egui 오버레이와 native WebView" 절이다.

/// 대상 팝업 오버레이의 이번 프레임 모드를 정하고 활성 WebView 마다 적용한다. 모든 활성 WebView 가
/// 같은 모드를 내야 하며 하나라도 낮으면 그 모드를 쓴다. 적용이 실패하면 그 프레임부터 구간이 끝날
/// 때까지 `Hide` 이고 경고는 한 번 남긴다. `views` 의 사각형은 활성 WebView 의 물리 사각형이다.
///
/// 그림 위치가 오버레이 창인 모드는 창 단위 host 가 낼 수 있는 상한까지 한 단계씩 낮춘다. 붙은 host 의
/// 상한은 `attached_ceiling` 으로 필요할 때만 묻는다. 주 표면에 그리는 모드는 host 와 관계없다.
pub(super) fn settle_overlay<B: crate::webview::overlay::WebViewOverlayBackend>(
    frame: &mut crate::state::webview_overlay::OverlayFrame,
    overlay_open: bool,
    blocked: bool,
    views: &[(&B, Option<crate::model::PhysicalRect>)],
    scale_factor: f64,
    attached_ceiling: impl FnOnce() -> Option<crate::state::webview_overlay::OverlayMode>,
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
        let host = frame.host;
        mode = lower_to_ceiling(
            mode,
            |m| paint_source(m, active.iter().map(|(wv, _)| *wv)),
            || host.ceiling(attached_ceiling),
        );
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
        frame.host.end_episode();
    }
    frame.applied = mode != OverlayMode::Hide;
    frame.mode = mode;
    frame.paint_source = paint_source(mode, active.iter().map(|(wv, _)| *wv));
    mode
}

/// 그림 위치가 오버레이 창인 동안 host 상한보다 높으면 한 단계 낮추고 그 모드의 그림 위치를 다시 본다.
/// 상한은 처음 필요할 때 한 번만 묻는다. `None` 은 아직 모르는 것이라 낮추지 않는다.
fn lower_to_ceiling(
    mut mode: crate::state::webview_overlay::OverlayMode,
    source_of: impl Fn(
        crate::state::webview_overlay::OverlayMode,
    ) -> crate::state::webview_overlay::OverlayPaintSource,
    ceiling: impl FnOnce() -> Option<crate::state::webview_overlay::OverlayMode>,
) -> crate::state::webview_overlay::OverlayMode {
    use crate::state::webview_overlay::{OverlayMode, OverlayPaintSource};
    let mut ceiling = Some(ceiling);
    let mut known = None;
    while mode != OverlayMode::Hide && source_of(mode) == OverlayPaintSource::OverlaySurface {
        if let Some(ask) = ceiling.take() {
            known = ask();
        }
        match known {
            Some(top) if mode > top => {
                mode = match mode {
                    OverlayMode::Transparent => OverlayMode::Opaque,
                    _ => OverlayMode::Hide,
                }
            }
            _ => break,
        }
    }
    mode
}

/// 고른 모드의 그림 위치. 활성 WebView 가 모두 주 표면에 맡기면 주 표면이다. 하나라도 오버레이 창을 쓰면
/// 오버레이 창이다. 오버레이 창은 주 표면 위에 덮이므로 구멍 방식 페이지 위에도 팝업이 보인다.
fn paint_source<'a, B: crate::webview::overlay::WebViewOverlayBackend + 'a>(
    mode: crate::state::webview_overlay::OverlayMode,
    active: impl IntoIterator<Item = &'a B>,
) -> crate::state::webview_overlay::OverlayPaintSource {
    use crate::state::webview_overlay::OverlayPaintSource;
    let all_main = active
        .into_iter()
        .all(|wv| wv.overlay_paint_source(mode) == OverlayPaintSource::MainSurface);
    if all_main {
        OverlayPaintSource::MainSurface
    } else {
        OverlayPaintSource::OverlaySurface
    }
}

/// 한 동기화의 드러내기 입력. `planned` 는 정렬한 드러낼 후보, `pages` 는 그 egui 논리 사각형이다.
pub(super) struct RevealFrame<'a> {
    pub mode: crate::state::webview_overlay::OverlayMode,
    pub overlay_open: bool,
    pub planned: &'a [u32],
    pub pages: Vec<egui::Rect>,
    /// 지금 주 표면의 물리 크기.
    pub surface_size: [u32; 2],
}

/// 드러낼 후보의 표시를 정한다. 대상 팝업이 떠 있고 모드가 `Hide` 가 아니면 같은 프레임에 같은 모드·그림
/// 위치·페이지·표면 크기의 그림이 present 됐을 때만 드러낸다. 아니면 모두 숨긴 채 그 페이지를 다음 프레임
/// 그리기에 넘긴다. 드러낸 surface id 와 다시 그려야 하는지를 돌려준다.
pub(super) fn settle_reveal<'a, W: 'a>(
    overlay: &mut crate::state::webview_overlay::OverlayFrame,
    frame: RevealFrame<'_>,
    webviews: impl IntoIterator<Item = (&'a u32, &'a W)>,
    set_visible: impl Fn(&W, bool),
) -> (Vec<u32>, bool) {
    use crate::state::webview_overlay::{OverlayMode, overlay_reveal_ready};
    let ready = overlay_reveal_ready(
        frame.mode,
        overlay.paint_source,
        frame.overlay_open,
        overlay.painted.as_ref(),
        &frame.pages,
        frame.surface_size,
    );
    let gated = frame.overlay_open && frame.mode != OverlayMode::Hide;
    overlay.planned_pages = if gated { frame.pages } else { Vec::new() };
    let revealed = if ready {
        frame.planned.to_vec()
    } else {
        Vec::new()
    };
    overlay.newly_revealed = revealed.iter().any(|id| !overlay.revealed.contains(id));
    overlay.revealed.clone_from(&revealed);
    for (sid, wv) in webviews {
        set_visible(wv, revealed.contains(sid));
    }
    (revealed, !ready)
}

/// 드러낼 후보(`planned`)의 표시를 [`settle_reveal`] 로 적용하고 실제로 드러낸 surface id 를 돌려준다.
/// 그림이 아직 준비되지 않았으면 다시 그리기를 요청한다.
pub(super) fn apply_reveal(
    view: &mut crate::view::MainView,
    mode: crate::state::webview_overlay::OverlayMode,
    overlay_open: bool,
    active: &std::collections::HashMap<u32, crate::model::PhysicalRect>,
    mut planned: Vec<u32>,
    scale_factor: f64,
) -> Vec<u32> {
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
    let size = view.base.gpu.size();
    let (revealed, redraw) = settle_reveal(
        &mut view.state.webview_overlay,
        RevealFrame {
            mode,
            overlay_open,
            planned: &planned,
            pages,
            surface_size: [size.width, size.height],
        },
        &view.webviews,
        |wv, visible| wv.set_visible(visible),
    );
    publish_opaque_pages(view, mode, active, &planned, scale_factor);
    if redraw {
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
            || None,
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
            || None,
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
            || None,
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
            settle_overlay(&mut frame, true, false, &views, 1.0, || None),
            OverlayMode::Hide
        );
        assert!(frame.health.failed());
        assert_eq!(
            settle_overlay(&mut frame, true, false, &views, 1.0, || None),
            OverlayMode::Hide
        );
        // 팝업이 닫히면 구간이 끝나 다음에는 다시 시도한다.
        settle_overlay(&mut frame, false, false, &views, 1.0, || None);
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
        settle_overlay(&mut frame, true, false, &views, 1.0, || None);
        assert_eq!(*wv.cleared.borrow(), 0);
        assert_eq!(
            settle_overlay(&mut frame, false, false, &views, 1.0, || None),
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
            settle_overlay(&mut frame, true, false, &views, 1.0, || None),
            OverlayMode::Hide
        );
        assert_eq!(ok.applied.borrow().len(), 1);
        assert!(*ok.cleared.borrow() >= 1);
        assert!(!*ok.pointer_held.borrow());
        assert!(!frame.applied);
        settle_overlay(&mut frame, false, false, &views, 1.0, || None);
        assert!(!*ok.pointer_held.borrow());
    }

    const SIZE: [u32; 2] = [200, 100];

    /// 한 창의 프레임 순서를 그대로 흉내 낸다. egui 프레임(대상 수집·job 준비) → 렌더(주 표면 job 꺼내기,
    /// 오버레이 창 job 은 `overlay_present` 일 때만 올림, 주 표면 present) → WebView 동기화.
    struct Window<'a> {
        ctx: egui::Context,
        frame: OverlayFrame,
        targets: Vec<OverlayTarget>,
        wv: &'a FakeOverlayBackend,
    }

    impl Window<'_> {
        fn render(&mut self, has_overlay_surface: bool, presented: Option<[u32; 2]>) {
            crate::state::webview_overlay::begin_frame(&self.ctx, &mut self.frame);
            self.frame.targets = self.targets.clone();
            crate::state::webview_overlay::prepare_paint_job(
                &mut self.frame,
                has_overlay_surface,
                SIZE,
                |targets, _| {
                    assert!(!targets.is_empty());
                    Vec::new()
                },
            );
            let main = crate::state::webview_overlay::take_main_surface_job(&mut self.frame);
            let overlay = self.frame.paint_job.take();
            let Some(size) = presented else { return };
            // 오버레이 창 job 은 오버레이 pass 가, 주 표면 job 은 주 pass present 뒤 렌더가 기록한다.
            for job in [overlay, main].into_iter().flatten() {
                self.frame.record_present(job, size);
            }
        }

        /// 동기화 한 번. 드러낸 surface 와 다시 그리기 요청 여부를 돌려준다.
        fn sync(&mut self) -> (Vec<u32>, bool) {
            let views = [(self.wv, Some(rect(0.0, 0.0, 200.0, 100.0)))];
            let mode = settle_overlay(&mut self.frame, true, false, &views, 1.0, || None);
            let ids = [7u32];
            super::settle_reveal(
                &mut self.frame,
                super::RevealFrame {
                    mode,
                    overlay_open: true,
                    planned: &ids,
                    pages: vec![er(0.0, 0.0, 200.0, 100.0)],
                    surface_size: SIZE,
                },
                ids.iter().zip([self.wv]),
                |wv, visible| wv.visible.set(visible),
            )
        }
    }

    fn window(wv: &FakeOverlayBackend) -> Window<'_> {
        let frame = frame_with(&[(TargetKind::HostPopup, er(10.0, 10.0, 50.0, 50.0))]);
        Window {
            ctx: egui::Context::default(),
            targets: frame.targets.clone(),
            frame,
            wv,
        }
    }

    /// 구멍 방식 Opaque backend 는 오버레이 창이 없어도 주 pass 가 같은 모드·페이지로 present 된 프레임에
    /// 페이지를 드러내고, 그 뒤로는 다시 그리기를 요청하지 않는다. present 전과 크기가 바뀐 present 는 기록이 아니다.
    #[test]
    fn a_main_surface_opaque_backend_reveals_pages_without_an_overlay_surface() {
        use crate::state::webview_overlay::OverlayPaintSource;
        let wv = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            paint_source: OverlayPaintSource::MainSurface,
            ..FakeOverlayBackend::default()
        };
        let mut win = window(&wv);
        // 팝업이 처음 뜬 동기화: 그림이 없으므로 숨기고 다시 그린다.
        assert_eq!(win.sync(), (vec![], true));
        assert_eq!(win.frame.paint_source, OverlayPaintSource::MainSurface);
        assert!(!wv.visible.get());
        // job 은 만들었지만 주 pass 가 아직 present 하지 않았다.
        win.render(false, None);
        assert_eq!(win.sync(), (vec![], true));
        // job 을 만든 뒤 표면 크기가 바뀐 present 는 쓰지 않는다.
        win.render(false, Some([201, 100]));
        assert_eq!(win.sync(), (vec![], true));
        assert!(!wv.visible.get());
        win.render(false, Some(SIZE));
        assert_eq!(win.sync(), (vec![7], false));
        assert!(wv.visible.get());
        win.render(false, Some(SIZE));
        assert_eq!(win.sync(), (vec![7], false));
        assert!(wv.visible.get());
        assert!(
            wv.applied
                .borrow()
                .iter()
                .all(|(mode, regions, pointer)| *mode == OverlayMode::Opaque
                    && regions == &vec![[10, 10, 50, 50]]
                    && *pointer)
        );
    }

    /// 오버레이 창 backend 는 그 창이 없으면 job 을 만들지 않아 계속 숨긴다. 창이 같은 기준으로 present 해야
    /// 드러낸다. 활성 WebView 중 하나라도 오버레이 창을 쓰면 그 프레임의 그림 위치는 오버레이 창이다.
    #[test]
    fn an_overlay_surface_backend_still_waits_for_the_overlay_pass() {
        use crate::state::webview_overlay::OverlayPaintSource;
        let wv = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            ..FakeOverlayBackend::default()
        };
        let mut win = window(&wv);
        for _ in 0..3 {
            win.render(false, Some(SIZE));
            assert!(win.frame.painted.is_none());
            assert_eq!(win.sync(), (vec![], true));
        }
        assert!(!wv.visible.get());
        win.render(true, Some(SIZE));
        assert_eq!(win.sync(), (vec![7], false));
        assert!(wv.visible.get());

        let main = FakeOverlayBackend {
            support: OverlayMode::Opaque,
            paint_source: OverlayPaintSource::MainSurface,
            ..FakeOverlayBackend::default()
        };
        assert_eq!(
            super::paint_source(OverlayMode::Opaque, [&main, &wv]),
            OverlayPaintSource::OverlaySurface
        );
        assert_eq!(
            super::paint_source(OverlayMode::Opaque, [&main]),
            OverlayPaintSource::MainSurface
        );
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
            || None,
        );
        assert_eq!(mode, OverlayMode::Hide);
        assert!(good.applied.borrow().is_empty());
    }
}
