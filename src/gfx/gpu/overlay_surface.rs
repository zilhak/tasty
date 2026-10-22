//! 창 단위 native 오버레이 host 에 붙인 wgpu surface. 붙이기·떼기·크기 맞추기·표시와 그 surface 로 낼 수
//! 있는 최고 모드(ceiling)를 정한다. 계약은 docs/design/systems/webview.md 의 "창 단위 오버레이 host" 절과
//! docs/dev-guide/gpu-rendering.md 의 "WebView 오버레이 pass" 절이다.

use crate::state::webview_overlay::OverlayMode;
use crate::webview::overlay_host::{HostComposite, OverlayHostError, OverlayNativeHost};

/// 오버레이 표면과 그 host. `surface` 를 먼저 선언해 host 보다 먼저 drop 한다. host 가 보유한 부모 창
/// `Arc` 가 surface 수명 내내 native 핸들을 살려 두는 것이 unsafe surface 생성의 안전 조건이다.
/// surface 형식을 일반화한 것은 시험이 GPU 없이 같은 필드 순서·표시 규칙을 쓰게 하기 위해서다.
pub(crate) struct OverlaySurfaceParts<S> {
    pub(crate) surface: S,
    pub(crate) config: wgpu::SurfaceConfiguration,
    pub(crate) host: Box<dyn OverlayNativeHost>,
    /// host 가 지금 보이는 중이다.
    pub(crate) shown: bool,
    /// 숨긴 채 이번 구간 그림을 준비 중이다. 보이기 전에 이 host 에 현재 그림을 먼저 present 한다.
    pub(crate) preparing: bool,
}

pub(crate) type OverlaySurface = OverlaySurfaceParts<wgpu::Surface<'static>>;

impl<S> OverlaySurfaceParts<S> {
    /// 오버레이 pass 가 그릴 수 있다. 보이는 중이거나 보이기 전 준비 중일 때다. 숨겨 둔 host 에는 그리지
    /// 않는다.
    pub(crate) fn paintable(&self) -> bool {
        self.shown || self.preparing
    }

    /// 숨긴 채 위치·크기를 따라가고 다음 오버레이 pass 가 이 host 에 그리게 한다.
    pub(crate) fn prepare(&mut self) {
        self.host.follow();
        self.preparing = true;
    }

    /// 이 surface 로 낼 수 있는 최고 모드.
    pub(crate) fn ceiling(&self) -> OverlayMode {
        surface_ceiling(self.host.composite(), self.config.alpha_mode)
    }

    /// 표시를 맞춘다. 보일 때는 위치·크기를 따라간 뒤 보이고, 새로 보였거나 `raise` 면 WebView 위로
    /// 올린다. 올렸으면 참이다(호출자가 한 프레임 더 그린다). 숨기면 준비도 멈춘다. 보이기는 이 host 에
    /// 이번 구간 그림을 present 한 뒤에만 부른다.
    pub(crate) fn show(&mut self, shown: bool, raise: bool) -> bool {
        self.preparing = false;
        let became = shown && !self.shown;
        if shown {
            self.host.follow();
        }
        if shown != self.shown {
            self.host.set_shown(shown);
            self.shown = shown;
        }
        let raised = shown && (became || raise);
        if raised {
            self.host.raise();
        }
        raised
    }

    /// 주 surface 크기가 바뀌었다. 같은 물리 크기로 맞추고 host 를 부모 창에 맞춘다.
    pub(crate) fn resized(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.host.follow();
    }
}

/// host 합성 계약과 오버레이 surface alpha 설정으로 정한 최고 모드. 외부 합성기가 알파를 반영하는 host 는
/// swapchain alpha 와 관계없이 투명을 낼 수 있다.
pub(crate) fn surface_ceiling(
    composite: HostComposite,
    alpha: wgpu::CompositeAlphaMode,
) -> OverlayMode {
    use wgpu::CompositeAlphaMode as A;
    match composite {
        HostComposite::ExternalArgb => OverlayMode::Transparent,
        HostComposite::SwapchainAlpha if matches!(alpha, A::PreMultiplied | A::PostMultiplied) => {
            OverlayMode::Transparent
        }
        HostComposite::SwapchainAlpha | HostComposite::OpaqueOnly => OverlayMode::Opaque,
        HostComposite::Unavailable => OverlayMode::Hide,
    }
}

/// 오버레이 surface 의 alpha 설정. 그림 규칙일 뿐 합성 판정이 아니다. caps 에 있는 값만 고른다.
pub(crate) fn pick_overlay_alpha(modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
    use wgpu::CompositeAlphaMode as A;
    [A::PreMultiplied, A::PostMultiplied]
        .into_iter()
        .find(|m| modes.contains(m))
        .or_else(|| modes.first().copied())
        .unwrap_or(A::Auto)
}

/// 주 surface 설정과 오버레이 surface caps 로 오버레이 설정을 만든다. 같은 egui renderer 로 그리므로 형식이
/// 같아야 하고 크기도 같다. present mode 는 주 surface 와 같은 규칙이다.
pub(crate) fn overlay_config(
    main: &wgpu::SurfaceConfiguration,
    caps: &wgpu::SurfaceCapabilities,
) -> Result<wgpu::SurfaceConfiguration, OverlayHostError> {
    if !caps.formats.contains(&main.format) {
        return Err(OverlayHostError::Failed(format!(
            "overlay surface does not offer {:?} (offers {:?})",
            main.format, caps.formats
        )));
    }
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: main.format,
        width: main.width,
        height: main.height,
        present_mode: super::pick_present_mode(&caps.present_modes),
        alpha_mode: pick_overlay_alpha(&caps.alpha_modes),
        view_formats: vec![],
        desired_maximum_frame_latency: main.desired_maximum_frame_latency,
    })
}

impl super::GpuState {
    /// host 의 native 표면에 wgpu surface 를 만들어 붙인다. 이미 붙어 있으면 먼저 뗀다. 실패하면 host 는
    /// 이 함수 안에서 drop 되어 native 창도 사라진다. App 의 Instance·Adapter 를 그대로 쓰고 같은 Device 로
    /// 설정한다.
    pub(crate) fn attach_overlay_surface(
        &mut self,
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        host: Box<dyn OverlayNativeHost>,
    ) -> Result<(), OverlayHostError> {
        self.detach_overlay_surface();
        // SAFETY: 이 대상으로 만든 surface 는 아래 `OverlaySurfaceParts` 에서 host 보다 먼저 선언돼 먼저 drop
        // 되고, 실패로 반환할 때도 지역 변수인 surface 가 인자인 host 보다 먼저 drop 된다.
        let target = unsafe { host.surface_target() };
        // SAFETY: host 가 부모 창 Arc 를 보유해 위 surface 수명 동안 대상 핸들을 살려 둔다.
        let surface = unsafe { instance.create_surface_unsafe(target) }
            .map_err(|e| OverlayHostError::Failed(format!("create overlay surface: {e}")))?;
        if !adapter.is_surface_supported(&surface) {
            return Err(OverlayHostError::Failed(
                "the GPU adapter cannot present to the overlay surface".into(),
            ));
        }
        let config = overlay_config(&self.config, &surface.get_capabilities(adapter))?;
        surface.configure(&self.device, &config);
        tracing::info!(
            alpha = ?config.alpha_mode,
            present_mode = ?config.present_mode,
            "WebView overlay surface attached"
        );
        self.overlay_surface = Some(OverlaySurfaceParts {
            surface,
            config,
            host,
            shown: false,
            preparing: false,
        });
        Ok(())
    }

    /// 오버레이 surface 를 떼고 host(native 창)를 버린다. 없으면 아무것도 하지 않는다.
    pub(crate) fn detach_overlay_surface(&mut self) {
        self.overlay_surface = None;
    }

    pub(crate) fn overlay_attached(&self) -> bool {
        self.overlay_surface.is_some()
    }

    /// 붙은 host 가 지금 보이는지. 붙지 않았으면 거짓.
    pub(crate) fn overlay_shown(&self) -> bool {
        self.overlay_surface.as_ref().is_some_and(|o| o.shown)
    }

    /// 붙은 host 를 숨긴 채 준비한다. 붙지 않았으면 아무것도 하지 않는다.
    pub(crate) fn prepare_overlay_host(&mut self) {
        if let Some(o) = self.overlay_surface.as_mut() {
            o.prepare();
        }
    }

    /// 붙은 surface 로 낼 수 있는 최고 모드. 붙지 않았으면 `None`.
    pub(crate) fn overlay_surface_ceiling(&self) -> Option<OverlayMode> {
        self.overlay_surface
            .as_ref()
            .map(OverlaySurfaceParts::ceiling)
    }

    /// host 표시를 맞춘다. 올렸으면 참이다. 붙지 않았으면 아무것도 하지 않는다.
    pub(crate) fn show_overlay_host(&mut self, shown: bool, raise: bool) -> bool {
        self.overlay_surface
            .as_mut()
            .is_some_and(|o| o.show(shown, raise))
    }

    /// 주 창이 움직였다. 붙은 host 만 따라간다.
    pub(crate) fn follow_overlay_host(&self) {
        if let Some(o) = self.overlay_surface.as_ref() {
            o.host.follow();
        }
    }

    /// 주 surface 와 같은 크기로 오버레이 surface 를 다시 설정한다.
    pub(super) fn resize_overlay_surface(&mut self, width: u32, height: u32) {
        if let Some(o) = self.overlay_surface.as_mut() {
            o.resized(width, height);
            o.surface.configure(&self.device, &o.config);
        }
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;

    /// host 와 surface 가 겪은 일을 순서대로 남긴다.
    pub(crate) type Log = Rc<RefCell<Vec<&'static str>>>;

    /// 시험용 host. 합성 계약을 정하고 호출을 기록한다.
    pub(crate) struct FakeHost {
        pub composite: Rc<Cell<HostComposite>>,
        pub log: Log,
    }

    impl OverlayNativeHost for FakeHost {
        unsafe fn surface_target(&self) -> wgpu::SurfaceTargetUnsafe {
            unreachable!("tests never create a GPU surface")
        }
        fn composite(&self) -> HostComposite {
            self.composite.get()
        }
        fn follow(&self) {
            self.log.borrow_mut().push("follow");
        }
        fn set_shown(&self, shown: bool) {
            self.log
                .borrow_mut()
                .push(if shown { "show" } else { "hide" });
        }
        fn raise(&self) {
            self.log.borrow_mut().push("raise");
        }
    }

    impl Drop for FakeHost {
        fn drop(&mut self) {
            self.log.borrow_mut().push("drop host");
        }
    }

    /// GPU surface 대신 drop 만 기록한다.
    pub(crate) struct FakeSurface(pub Log);

    impl Drop for FakeSurface {
        fn drop(&mut self) {
            self.0.borrow_mut().push("drop surface");
        }
    }

    pub(crate) fn config(alpha: wgpu::CompositeAlphaMode) -> wgpu::SurfaceConfiguration {
        wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: wgpu::TextureFormat::Bgra8Unorm,
            width: 200,
            height: 100,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: alpha,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        }
    }

    pub(crate) fn parts(
        composite: &Rc<Cell<HostComposite>>,
        log: &Log,
        alpha: wgpu::CompositeAlphaMode,
    ) -> OverlaySurfaceParts<FakeSurface> {
        OverlaySurfaceParts {
            surface: FakeSurface(log.clone()),
            config: config(alpha),
            host: Box::new(FakeHost {
                composite: composite.clone(),
                log: log.clone(),
            }),
            shown: false,
            preparing: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::fake::{Log, parts};
    use super::*;
    use wgpu::CompositeAlphaMode as A;

    /// 외부 합성기가 알파를 반영하는 host 는 Opaque alpha 로 설정돼도 투명을 낸다. swapchain alpha 가 필요한
    /// host 는 Pre·Post 일 때만 투명이다.
    #[test]
    fn the_ceiling_follows_the_host_composite_and_the_alpha_config() {
        use HostComposite as C;
        let rows = [
            (C::ExternalArgb, A::Opaque, OverlayMode::Transparent),
            (C::ExternalArgb, A::PreMultiplied, OverlayMode::Transparent),
            (
                C::SwapchainAlpha,
                A::PreMultiplied,
                OverlayMode::Transparent,
            ),
            (
                C::SwapchainAlpha,
                A::PostMultiplied,
                OverlayMode::Transparent,
            ),
            (C::SwapchainAlpha, A::Opaque, OverlayMode::Opaque),
            (C::SwapchainAlpha, A::Inherit, OverlayMode::Opaque),
            (C::OpaqueOnly, A::PreMultiplied, OverlayMode::Opaque),
            (C::Unavailable, A::PreMultiplied, OverlayMode::Hide),
        ];
        for (composite, alpha, want) in rows {
            assert_eq!(
                surface_ceiling(composite, alpha),
                want,
                "{composite:?} {alpha:?}"
            );
        }
    }

    #[test]
    fn the_alpha_config_prefers_premultiplied_then_postmultiplied_then_the_first_offer() {
        assert_eq!(
            pick_overlay_alpha(&[A::Opaque, A::PostMultiplied, A::PreMultiplied]),
            A::PreMultiplied
        );
        assert_eq!(
            pick_overlay_alpha(&[A::Opaque, A::PostMultiplied]),
            A::PostMultiplied
        );
        assert_eq!(pick_overlay_alpha(&[A::Inherit, A::Opaque]), A::Inherit);
        assert_eq!(pick_overlay_alpha(&[]), A::Auto);
    }

    fn caps(
        formats: Vec<wgpu::TextureFormat>,
        present: Vec<wgpu::PresentMode>,
    ) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats,
            present_modes: present,
            alpha_modes: vec![A::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
        }
    }

    /// 오버레이는 주 surface 형식·크기를 그대로 쓴다. 형식이 없으면 붙이지 않는다. Mailbox 가 없으면 Fifo 다.
    #[test]
    fn the_overlay_config_keeps_the_main_format_and_size() {
        let main = fake::config(A::Opaque);
        let ok = overlay_config(
            &main,
            &caps(
                vec![
                    wgpu::TextureFormat::Rgba8Unorm,
                    wgpu::TextureFormat::Bgra8Unorm,
                ],
                vec![wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate],
            ),
        )
        .unwrap();
        assert_eq!(ok.format, main.format);
        assert_eq!([ok.width, ok.height], [main.width, main.height]);
        assert_eq!(ok.present_mode, wgpu::PresentMode::Fifo);
        assert_eq!(ok.alpha_mode, A::Opaque);
        let missing = overlay_config(
            &main,
            &caps(
                vec![wgpu::TextureFormat::Rgba8Unorm],
                vec![wgpu::PresentMode::Fifo],
            ),
        );
        assert!(matches!(missing, Err(OverlayHostError::Failed(_))));
    }

    /// 숨긴 채 준비하면 따라가기만 하고 그릴 수 있게 된다. 보일 때는 따라간 뒤 보이고 올린다. 계속 보이는
    /// 동안은 따라가기만 하고, 새로 드러난 페이지가 있을 때만 다시 올린다. 숨김은 바뀔 때 한 번이고 그 뒤로는
    /// 그리지 않는다. 떼면 surface 가 host 보다 먼저 사라진다.
    #[test]
    fn the_host_shows_after_following_and_drops_after_its_surface() {
        let log: Log = Default::default();
        let composite = Rc::new(Cell::new(HostComposite::ExternalArgb));
        let mut p = parts(&composite, &log, A::Opaque);
        assert_eq!(p.ceiling(), OverlayMode::Transparent);
        assert!(!p.paintable());
        p.prepare();
        assert!(p.paintable() && !p.shown);
        assert!(p.show(true, false));
        assert!(p.paintable() && !p.preparing);
        assert!(!p.show(true, false));
        assert!(p.show(true, true));
        assert!(!p.show(false, true));
        assert!(!p.paintable());
        assert!(!p.show(false, false));
        p.resized(300, 150);
        assert_eq!([p.config.width, p.config.height], [300, 150]);
        composite.set(HostComposite::Unavailable);
        assert_eq!(p.ceiling(), OverlayMode::Hide);
        drop(p);
        assert_eq!(
            *log.borrow(),
            [
                "follow",
                "follow",
                "show",
                "raise",
                "follow",
                "follow",
                "raise",
                "hide",
                "follow",
                "drop surface",
                "drop host"
            ]
        );
    }
}
