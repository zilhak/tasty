//! WebView 동기화 뒤 창 단위 오버레이 host 를 맞춘다. 계약은 docs/design/systems/webview.md 의
//! "창 단위 오버레이 host" 절이다.
//!
//! 판단은 [`host_step`] 표가 한다. 그림 위치가 오버레이 창인 모드가 필요할 때만 만들고, 필요 없어지면
//! 숨기며, 창의 WebView 가 모두 사라지면 버린다. 새로 드러난 페이지가 있으면 host 를 다시 올린다.
//! 주 창 최소화·작업 공간·다른 앱과의 순서는 묻지 않는다. OS host 가 주 창에 묶여(Linux managed
//! transient, macOS·Windows 자식) 함께 따라가는 것이 host 계약이다.

use crate::state::webview_overlay::{
    OverlayFrame, OverlayHostSlot, OverlayMode, OverlayPaintSource,
};
use crate::webview::overlay_host::{OverlayHostError, OverlayNativeHost};

/// 이번 동기화에서 host 에 할 일.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HostStep {
    Nothing,
    /// host 를 버린다(native 창 파괴).
    Detach,
    /// host 를 만들어 붙이고 보인다.
    Create,
    /// 위치·크기를 따라간 뒤 보인다. `raise` 면 WebView 위로 다시 올린다.
    Show {
        raise: bool,
    },
    Hide,
}

/// host 상태 표. OS 를 묻지 않는 순수 판단이다. 돌려주는 상태는 이 단계를 하기 전의 것이며 만들기 결과는
/// [`slot_after_create`] 가 정한다.
pub(super) fn host_step(
    slot: OverlayHostSlot,
    has_webviews: bool,
    need: bool,
    surface_alive: bool,
    newly_revealed: bool,
) -> (HostStep, OverlayHostSlot) {
    use OverlayHostSlot as S;
    if !has_webviews {
        return match slot {
            S::Attached => (HostStep::Detach, S::Absent),
            other => (HostStep::Nothing, other),
        };
    }
    match slot {
        // 그리다가 surface 를 잃었다. 그 실패는 오버레이 pass 가 이미 기록했다.
        S::Attached if !surface_alive => (HostStep::Nothing, S::FailedThisEpisode),
        S::Attached if need => (
            HostStep::Show {
                raise: newly_revealed,
            },
            S::Attached,
        ),
        S::Attached => (HostStep::Hide, S::Attached),
        S::Absent if need => (HostStep::Create, S::Absent),
        other => (HostStep::Nothing, other),
    }
}

/// 만들어 붙인 결과의 상태. 지원하지 않는 OS 는 창 수명 동안 다시 시도하지 않는다.
pub(super) fn slot_after_create(result: &Result<(), OverlayHostError>) -> OverlayHostSlot {
    match result {
        Ok(()) => OverlayHostSlot::Attached,
        Err(OverlayHostError::Unsupported) => OverlayHostSlot::Unavailable,
        Err(OverlayHostError::Failed(_)) => OverlayHostSlot::FailedThisEpisode,
    }
}

/// host 를 붙일 GPU 쪽. 창의 `GpuState` 와 시험용 surface 가 구현한다.
pub(super) trait HostSurface {
    fn attached(&self) -> bool;
    fn attach(&mut self, host: Box<dyn OverlayNativeHost>) -> Result<(), OverlayHostError>;
    fn detach(&mut self);
    /// 표시를 맞춘다. 올렸으면 참.
    fn show(&mut self, shown: bool, raise: bool) -> bool;
}

/// 한 번의 훅. 다시 그려야 하면 참이다. 만들었거나 올렸으면 다음 프레임이 그 surface 에 그리고, 만들지
/// 못했으면 다음 동기화가 낮춘 모드로 다시 정한다.
pub(super) fn drive_host(
    frame: &mut OverlayFrame,
    surface: &mut impl HostSurface,
    has_webviews: bool,
    create: impl FnOnce() -> Result<Box<dyn OverlayNativeHost>, OverlayHostError>,
) -> bool {
    let need =
        frame.mode != OverlayMode::Hide && frame.paint_source == OverlayPaintSource::OverlaySurface;
    let (step, slot) = host_step(
        frame.host,
        has_webviews,
        need,
        surface.attached(),
        frame.newly_revealed,
    );
    frame.host = slot;
    match step {
        HostStep::Nothing => false,
        HostStep::Detach => {
            surface.detach();
            false
        }
        HostStep::Hide => surface.show(false, false),
        HostStep::Show { raise } => surface.show(true, raise),
        HostStep::Create => {
            let result = create().and_then(|host| surface.attach(host));
            frame.host = slot_after_create(&result);
            match result {
                Ok(()) => {
                    surface.show(true, true);
                }
                Err(OverlayHostError::Unsupported) => tracing::info!(
                    "WebView overlay host is not available on this system; pages under popups use the main surface or hide"
                ),
                Err(OverlayHostError::Failed(e)) => {
                    tracing::warn!("WebView overlay host failed, lowering the overlay mode: {e}")
                }
            }
            true
        }
    }
}

/// 창의 GPU 와 App 이 보관한 Instance·Adapter.
struct GpuHost<'a> {
    gpu: &'a mut crate::gpu::GpuState,
    instance: &'a wgpu::Instance,
    adapter: Option<&'a wgpu::Adapter>,
}

impl HostSurface for GpuHost<'_> {
    fn attached(&self) -> bool {
        self.gpu.overlay_attached()
    }
    fn attach(&mut self, host: Box<dyn OverlayNativeHost>) -> Result<(), OverlayHostError> {
        let adapter = self
            .adapter
            .ok_or_else(|| OverlayHostError::Failed("no GPU adapter".into()))?;
        self.gpu
            .attach_overlay_surface(self.instance, adapter, host)
    }
    fn detach(&mut self) {
        self.gpu.detach_overlay_surface();
    }
    fn show(&mut self, shown: bool, raise: bool) -> bool {
        self.gpu.show_overlay_host(shown, raise)
    }
}

/// WebView 동기화 바로 뒤에 부른다. 같은 프레임의 동기화가 정한 모드·그림 위치로 host 를 맞추므로 다음 프레임
/// 렌더는 붙은 host 에 그리고, 그 그림이 present 된 동기화가 페이지를 드러낸다.
pub(crate) fn settle_overlay_host(
    view: &mut crate::view::MainView,
    instance: &wgpu::Instance,
    adapter: Option<&wgpu::Adapter>,
) {
    let window = &view.base.winit;
    let redraw = drive_host(
        &mut view.state.webview_overlay,
        &mut GpuHost {
            gpu: &mut view.base.gpu,
            instance,
            adapter,
        },
        !view.webviews.is_empty(),
        || crate::webview::overlay_host::create_overlay_host(window),
    );
    if redraw {
        crate::view::ui::View::mark_dirty(view);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::gpu::overlay_surface::OverlaySurfaceParts;
    use crate::gpu::overlay_surface::fake::{FakeHost, FakeSurface, Log, config};
    use crate::model::{PhysicalPx, PhysicalRect};
    use crate::state::webview_overlay::{OverlayTarget, TargetKind};
    use crate::webview::overlay::fake::FakeOverlayBackend;
    use crate::webview::overlay_host::HostComposite;

    /// 표의 모든 행.
    #[test]
    fn the_host_table_covers_every_slot() {
        use HostStep as H;
        use OverlayHostSlot as S;
        let rows = [
            // (slot, webviews, need, alive, newly) -> (step, slot)
            (
                (S::Attached, false, true, true, false),
                (H::Detach, S::Absent),
            ),
            (
                (S::Absent, false, true, false, false),
                (H::Nothing, S::Absent),
            ),
            (
                (S::FailedThisEpisode, false, true, false, false),
                (H::Nothing, S::FailedThisEpisode),
            ),
            (
                (S::Unavailable, false, true, false, false),
                (H::Nothing, S::Unavailable),
            ),
            (
                (S::Absent, true, false, false, false),
                (H::Nothing, S::Absent),
            ),
            (
                (S::Absent, true, true, false, false),
                (H::Create, S::Absent),
            ),
            (
                (S::Attached, true, true, true, false),
                (H::Show { raise: false }, S::Attached),
            ),
            (
                (S::Attached, true, true, true, true),
                (H::Show { raise: true }, S::Attached),
            ),
            (
                (S::Attached, true, false, true, true),
                (H::Hide, S::Attached),
            ),
            (
                (S::Attached, true, true, false, false),
                (H::Nothing, S::FailedThisEpisode),
            ),
            (
                (S::Attached, true, false, false, false),
                (H::Nothing, S::FailedThisEpisode),
            ),
            (
                (S::FailedThisEpisode, true, true, false, false),
                (H::Nothing, S::FailedThisEpisode),
            ),
            (
                (S::Unavailable, true, true, false, false),
                (H::Nothing, S::Unavailable),
            ),
        ];
        for ((slot, webviews, need, alive, newly), want) in rows {
            let got = host_step(slot, webviews, need, alive, newly);
            assert_eq!(
                got, want,
                "{slot:?} webviews={webviews} need={need} alive={alive}"
            );
        }
        assert_eq!(slot_after_create(&Ok(())), S::Attached);
        assert_eq!(
            slot_after_create(&Err(OverlayHostError::Unsupported)),
            S::Unavailable
        );
        assert_eq!(
            slot_after_create(&Err(OverlayHostError::Failed("x".into()))),
            S::FailedThisEpisode
        );
    }

    /// 시험용 GPU. 실제 `OverlaySurfaceParts` 의 표시·drop 규칙을 쓴다.
    struct FakeGpu {
        parts: Option<OverlaySurfaceParts<FakeSurface>>,
        log: Log,
        alpha: wgpu::CompositeAlphaMode,
    }

    impl HostSurface for FakeGpu {
        fn attached(&self) -> bool {
            self.parts.is_some()
        }
        fn attach(&mut self, host: Box<dyn OverlayNativeHost>) -> Result<(), OverlayHostError> {
            self.parts = Some(OverlaySurfaceParts {
                surface: FakeSurface(self.log.clone()),
                config: config(self.alpha),
                host,
                shown: false,
            });
            Ok(())
        }
        fn detach(&mut self) {
            self.parts = None;
        }
        fn show(&mut self, shown: bool, raise: bool) -> bool {
            self.parts.as_mut().is_some_and(|p| p.show(shown, raise))
        }
    }

    const SIZE: [u32; 2] = [200, 100];

    enum Make {
        Host,
        Fail,
        Unsupported,
    }

    /// 창 하나의 프레임 순서. 렌더(job 준비 → 주 표면 job 기록 → 보이는 host 에만 오버레이 그림 기록) →
    /// WebView 동기화(모드·드러내기) → host 훅.
    struct Window<'a> {
        ctx: egui::Context,
        frame: OverlayFrame,
        targets: Vec<OverlayTarget>,
        wv: &'a FakeOverlayBackend,
        gpu: FakeGpu,
        composite: Rc<Cell<HostComposite>>,
        make: Make,
        made: usize,
        open: bool,
        webviews: bool,
    }

    impl<'a> Window<'a> {
        fn new(wv: &'a FakeOverlayBackend, composite: HostComposite, make: Make) -> Self {
            let r = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(40.0, 40.0));
            Window {
                ctx: egui::Context::default(),
                frame: OverlayFrame::default(),
                targets: vec![OverlayTarget {
                    kind: TargetKind::HostPopup,
                    layer: egui::LayerId::new(egui::Order::Foreground, egui::Id::new("popup")),
                    rect: r,
                    opaque_rect: r,
                    paint_rect: r,
                    scrim: None,
                }],
                wv,
                gpu: FakeGpu {
                    parts: None,
                    log: Default::default(),
                    alpha: wgpu::CompositeAlphaMode::Opaque,
                },
                composite: Rc::new(Cell::new(composite)),
                make,
                made: 0,
                open: true,
                webviews: true,
            }
        }

        fn render(&mut self) {
            use crate::state::webview_overlay as o;
            o::begin_frame(&self.ctx, &mut self.frame);
            if self.open {
                self.frame.targets = self.targets.clone();
            }
            o::prepare_paint_job(&mut self.frame, self.gpu.parts.is_some(), SIZE, |_, _| {
                Vec::new()
            });
            let main = o::take_main_surface_job(&mut self.frame);
            let overlay = self.frame.paint_job.take();
            if let Some(job) =
                overlay.filter(|_| self.gpu.parts.as_ref().is_some_and(|p| p.paintable()))
            {
                self.frame.record_present(job, SIZE);
            }
            if let Some(job) = main {
                self.frame.record_present(job, SIZE);
            }
        }

        /// 동기화와 훅. 이번 동기화의 모드, 드러낸 페이지, 다시 그리기 요청을 돌려준다.
        fn sync(&mut self) -> (OverlayMode, Vec<u32>, bool) {
            let page = PhysicalRect {
                x: PhysicalPx(0.0),
                y: PhysicalPx(0.0),
                width: PhysicalPx(200.0),
                height: PhysicalPx(100.0),
            };
            let views = [(self.wv, Some(page))];
            let gpu = &self.gpu;
            let mode = super::super::overlay_settle::settle_overlay(
                &mut self.frame,
                self.open,
                false,
                &views,
                1.0,
                || gpu.parts.as_ref().map(OverlaySurfaceParts::ceiling),
            );
            let ids = [7u32];
            let (revealed, reveal_redraw) = super::super::overlay_settle::settle_reveal(
                &mut self.frame,
                super::super::overlay_settle::RevealFrame {
                    mode,
                    overlay_open: self.open,
                    planned: &ids,
                    pages: vec![egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(200.0, 100.0),
                    )],
                    surface_size: SIZE,
                },
                ids.iter().zip([self.wv]),
                |wv, visible| wv.visible.set(visible),
            );
            let made = &mut self.made;
            let composite = &self.composite;
            let log = self.gpu.log.clone();
            let make = &self.make;
            let host_redraw = drive_host(&mut self.frame, &mut self.gpu, self.webviews, || {
                *made += 1;
                match make {
                    Make::Host => Ok(Box::new(FakeHost {
                        composite: composite.clone(),
                        log: log.clone(),
                    }) as Box<dyn OverlayNativeHost>),
                    Make::Fail => Err(OverlayHostError::Failed("fake".into())),
                    Make::Unsupported => Err(OverlayHostError::Unsupported),
                }
            });
            (mode, revealed, reveal_redraw || host_redraw)
        }

        fn frame(&mut self) -> (OverlayMode, Vec<u32>, bool) {
            self.render();
            self.sync()
        }

        fn take_log(&self) -> Vec<&'static str> {
            std::mem::take(&mut *self.gpu.log.borrow_mut())
        }
    }

    fn transparent_backend(opaque_on_main: bool) -> FakeOverlayBackend {
        FakeOverlayBackend {
            support: OverlayMode::Transparent,
            opaque_on_main,
            ..FakeOverlayBackend::default()
        }
    }

    /// 처음 뜬 팝업: 첫 동기화는 host 가 없어 모드를 낮추지 않고 숨긴 채 host 를 만들어 보인다. 다음 프레임이
    /// 그 host 에 그린 뒤 페이지를 드러내고 host 를 다시 올린다. 그 뒤로는 다시 그리기를 요청하지 않는다.
    #[test]
    fn a_popup_bootstraps_the_host_then_reveals_after_its_present() {
        let wv = transparent_backend(false);
        let mut win = Window::new(&wv, HostComposite::ExternalArgb, Make::Host);
        bootstrap(&mut win, &wv);
    }

    fn bootstrap(win: &mut Window<'_>, wv: &FakeOverlayBackend) {
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![], true));
        assert_eq!(win.made, 1);
        assert_eq!(win.frame.host, OverlayHostSlot::Attached);
        assert_eq!(win.take_log(), ["follow", "show", "raise"]);
        assert!(!wv.visible.get());
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![7], true));
        assert!(wv.visible.get());
        assert_eq!(win.take_log(), ["follow", "raise"]);
        for _ in 0..3 {
            assert_eq!(win.frame(), (OverlayMode::Transparent, vec![7], false));
            assert_eq!(win.take_log(), ["follow"]);
        }
    }

    /// 팝업이 닫히면 숨기기만 하고 다시 열면 같은 host 를 보인 뒤 그린 프레임에 드러낸다. 창의 마지막 WebView 가
    /// 사라지면 surface 다음 host 를 버리고, 그 뒤로는 OS 를 부르지 않는다.
    #[test]
    fn a_closed_popup_hides_the_host_and_the_last_webview_drops_it() {
        let wv = transparent_backend(false);
        let mut win = Window::new(&wv, HostComposite::ExternalArgb, Make::Host);
        bootstrap(&mut win, &wv);
        win.open = false;
        assert_eq!(win.frame().0, OverlayMode::Hide);
        assert_eq!(win.take_log(), ["hide"]);
        assert_eq!(win.frame(), (OverlayMode::Hide, vec![7], false));
        assert!(win.take_log().is_empty());
        // 다시 열면 붙은 host 의 상한으로 바로 정하고, 보인 뒤 그린 프레임에 드러낸다.
        win.open = true;
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![], true));
        assert_eq!(win.take_log(), ["follow", "show", "raise"]);
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![7], true));
        assert_eq!(win.made, 1);
        win.webviews = false;
        win.frame();
        assert_eq!(win.frame.host, OverlayHostSlot::Absent);
        assert_eq!(
            win.take_log(),
            ["follow", "raise", "drop surface", "drop host"]
        );
        win.frame();
        assert!(win.take_log().is_empty());
    }

    /// 상한이 낮은 host: 불투명 사각형만 내는 host 는 투명을 불투명으로 낮춰 같은 오버레이 창에 그린다.
    /// 합성할 수 없는 host 는 host 를 숨기고, 구멍 방식 불투명이 있는 backend 는 주 표면으로 계속 드러낸다.
    #[test]
    fn a_lower_ceiling_downgrades_without_blocking_the_main_surface() {
        let wv = transparent_backend(false);
        let mut win = Window::new(&wv, HostComposite::OpaqueOnly, Make::Host);
        assert_eq!(win.frame().0, OverlayMode::Transparent);
        assert_eq!(win.frame(), (OverlayMode::Opaque, vec![], true));
        assert_eq!(win.frame(), (OverlayMode::Opaque, vec![7], true));
        assert_eq!(win.frame(), (OverlayMode::Opaque, vec![7], false));

        let cut = transparent_backend(true);
        let mut win = Window::new(&cut, HostComposite::Unavailable, Make::Host);
        assert_eq!(win.frame().0, OverlayMode::Transparent);
        assert_eq!(win.take_log(), ["follow", "show", "raise"]);
        assert_eq!(win.frame(), (OverlayMode::Opaque, vec![], true));
        assert_eq!(win.take_log(), ["hide"]);
        assert_eq!(win.frame(), (OverlayMode::Opaque, vec![7], false));
        assert!(cut.visible.get());
        assert_eq!(win.frame(), (OverlayMode::Opaque, vec![7], false));
        assert!(win.take_log().is_empty());
        // 합성이 돌아오면 다시 투명으로 올라간다.
        win.composite.set(HostComposite::ExternalArgb);
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![], true));
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![7], true));
    }

    /// 만들지 못하면 그 구간에는 다시 만들지 않고 모드를 낮춘다. 구멍 방식 불투명은 그대로 쓴다. 팝업이 닫혀
    /// 구간이 끝나면 다음 구간에 한 번 다시 시도한다. 지원하지 않는 OS 는 다시 시도하지 않는다.
    #[test]
    fn a_failed_host_is_retried_once_per_episode_and_unsupported_never() {
        let cut = transparent_backend(true);
        let mut win = Window::new(&cut, HostComposite::ExternalArgb, Make::Fail);
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![], true));
        assert_eq!(win.frame.host, OverlayHostSlot::FailedThisEpisode);
        assert_eq!(win.frame(), (OverlayMode::Opaque, vec![], true));
        for _ in 0..3 {
            assert_eq!(win.frame(), (OverlayMode::Opaque, vec![7], false));
        }
        assert_eq!(win.made, 1);
        assert!(!win.frame.health.failed());
        win.open = false;
        win.frame();
        assert_eq!(win.frame.host, OverlayHostSlot::Absent);
        win.open = true;
        win.frame();
        assert_eq!(win.made, 2);

        let wv = transparent_backend(false);
        let mut win = Window::new(&wv, HostComposite::ExternalArgb, Make::Unsupported);
        win.frame();
        assert_eq!(win.frame.host, OverlayHostSlot::Unavailable);
        assert_eq!(win.frame(), (OverlayMode::Hide, vec![7], false));
        win.open = false;
        win.frame();
        win.open = true;
        for _ in 0..3 {
            assert_eq!(win.frame().0, OverlayMode::Hide);
        }
        assert_eq!(win.made, 1);
    }

    /// 숨긴 host 에는 그리지 않으므로 그 프레임 기록이 없고 페이지를 드러내지 않는다. 그리다 surface 를 잃으면
    /// 그 구간에는 다시 만들지 않고, 구간이 끝나면 한 번 다시 시도한다.
    #[test]
    fn a_hidden_host_is_not_painted_and_a_lost_surface_waits_for_the_next_episode() {
        let wv = transparent_backend(false);
        let mut win = Window::new(&wv, HostComposite::ExternalArgb, Make::Host);
        win.frame();
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![7], true));
        win.gpu.parts.as_mut().unwrap().shown = false;
        win.render();
        assert!(win.frame.painted.is_none());
        assert_eq!(win.sync(), (OverlayMode::Transparent, vec![], true));
        assert!(!wv.visible.get());
        assert_eq!(win.frame(), (OverlayMode::Transparent, vec![7], true));
        win.take_log();

        win.gpu.parts = None;
        assert_eq!(win.take_log(), ["drop surface", "drop host"]);
        assert_eq!(win.frame(), (OverlayMode::Hide, vec![7], false));
        assert_eq!(win.frame.host, OverlayHostSlot::FailedThisEpisode);
        for _ in 0..2 {
            assert_eq!(win.frame(), (OverlayMode::Hide, vec![7], false));
        }
        assert_eq!(win.made, 1);
        win.open = false;
        win.frame();
        assert_eq!(win.frame.host, OverlayHostSlot::Absent);
        win.open = true;
        win.frame();
        assert_eq!(win.made, 2);
    }
}
