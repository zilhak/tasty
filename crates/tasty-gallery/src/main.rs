//! `tasty-gallery` 바이너리 진입점.
//!
//! winit + wgpu + egui_wgpu 부트스트랩. 본체 `tasty` 의 GPU 파이프라인과
//! 별개로 가장 단순한 형태만 갖춘다 — 본 phase 의 목적은 egui 위젯 카탈로그를
//! 시각화하는 것뿐이라 터미널 렌더러 / shm / plugin 등은 끌어오지 않는다.

use std::sync::Arc;

use tasty_gallery::host_shell::{self, GalleryState};

use wgpu::TextureViewDescriptor;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let event_loop = EventLoop::new()?;
    let mut app = App::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[derive(Default)]
struct App {
    runtime: Option<Runtime>,
}

struct Runtime {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    gallery: GalleryState,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.runtime.is_some() {
            return;
        }
        let attrs = WindowAttributes::default()
            .with_title("Tasty Gallery")
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

        let rt = pollster::block_on(init_runtime(window)).expect("gallery runtime init");
        self.runtime = Some(rt);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(rt) = self.runtime.as_mut() else {
            return;
        };

        // egui 입력 처리.
        let response = rt.egui_state.on_window_event(&rt.window, &event);
        if response.repaint {
            rt.window.request_redraw();
        }
        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(new_size) => {
                if new_size.width > 0 && new_size.height > 0 {
                    rt.config.width = new_size.width;
                    rt.config.height = new_size.height;
                    rt.surface.configure(&rt.device, &rt.config);
                    rt.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = render_frame(rt) {
                    tracing::error!("render error: {err:?}");
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(rt) = self.runtime.as_ref() {
            rt.window.request_redraw();
        }
    }
}

async fn init_runtime(window: Arc<Window>) -> anyhow::Result<Runtime> {
    let size = window.inner_size();
    let scale_factor = window.scale_factor() as f32;

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let surface = instance.create_surface(window.clone())?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("no compatible GPU adapter"))?;

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("tasty_gallery_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
            },
            None,
        )
        .await?;

    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
        .formats
        .iter()
        .find(|f| !f.is_srgb())
        .copied()
        .or_else(|| surface_caps.formats.first().copied())
        .ok_or_else(|| anyhow::anyhow!("no surface format"))?;

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: surface_caps
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Auto),
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &config);

    let egui_ctx = egui::Context::default();
    egui_ctx.options_mut(|opts| {
        opts.zoom_with_keyboard = false;
    });
    install_cjk_fallback(&egui_ctx);

    let egui_state = egui_winit::State::new(
        egui_ctx.clone(),
        egui_ctx.viewport_id(),
        &*window,
        Some(scale_factor),
        None,
        Some(2048),
    );

    let egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1, false);

    Ok(Runtime {
        window,
        device,
        queue,
        surface,
        config,
        egui_ctx,
        egui_state,
        egui_renderer,
        gallery: GalleryState::new(),
    })
}

/// 갤러리는 본체의 폰트 파이프라인을 끌어오지 않으므로, egui Context 가
/// 한글/한자/かな 글리프를 그리지 못한다 (Proportional / Monospace 기본
/// family 에는 라틴 글리프만 포함됨 → □ 로 fallback). 시스템 CJK 폰트를
/// 직접 읽어 양쪽 family 끝에 fallback 으로 붙인다. 본체의
/// `src/gfx/gpu/fonts.rs` 와 같은 경로 리스트.
fn install_cjk_fallback(ctx: &egui::Context) {
    let Some(bytes) = load_system_cjk_font() else {
        tracing::warn!(
            "gallery: no system CJK font found; Korean/Japanese/Chinese labels will render as □"
        );
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "system_cjk".to_owned(),
        Arc::new(egui::FontData::from_owned(bytes)),
    );
    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(fam)
            .or_default()
            .push("system_cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn load_system_cjk_font() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(data) = std::fs::read("C:/Windows/Fonts/malgun.ttf") {
            return Some(data);
        }
    }

    #[cfg(target_os = "macos")]
    {
        for path in &[
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        for path in &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }

    None
}

fn render_frame(rt: &mut Runtime) -> anyhow::Result<()> {
    let raw_input = rt.egui_state.take_egui_input(&rt.window);
    let full_output = rt.egui_ctx.run(raw_input, |ctx| {
        host_shell::draw(ctx, &mut rt.gallery);
    });
    rt.egui_state
        .handle_platform_output(&rt.window, full_output.platform_output);

    let frame = match rt.surface.get_current_texture() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            rt.surface.configure(&rt.device, &rt.config);
            return Ok(());
        }
        Err(e) => return Err(anyhow::anyhow!("surface acquire: {e:?}")),
    };
    let view = frame.texture.create_view(&TextureViewDescriptor::default());

    let mut encoder = rt
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gallery_encoder"),
        });

    let paint_jobs = rt
        .egui_ctx
        .tessellate(full_output.shapes, full_output.pixels_per_point);

    let screen_descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [rt.config.width, rt.config.height],
        pixels_per_point: full_output.pixels_per_point,
    };

    for (id, image_delta) in &full_output.textures_delta.set {
        rt.egui_renderer
            .update_texture(&rt.device, &rt.queue, *id, image_delta);
    }
    rt.egui_renderer.update_buffers(
        &rt.device,
        &rt.queue,
        &mut encoder,
        &paint_jobs,
        &screen_descriptor,
    );

    {
        let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gallery_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.05,
                        b: 0.07,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let mut render_pass = render_pass.forget_lifetime();
        rt.egui_renderer
            .render(&mut render_pass, &paint_jobs, &screen_descriptor);
    }

    for id in &full_output.textures_delta.free {
        rt.egui_renderer.free_texture(id);
    }

    rt.queue.submit(std::iter::once(encoder.finish()));
    frame.present();
    Ok(())
}
