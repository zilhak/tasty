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
    let mut app = App {
        shot: parse_shot_env(),
        ..App::default()
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// 배치 스크린샷 계획 — `TASTY_GALLERY_SHOT=<idx>:<png>[,<idx>:<png>...]`.
/// 지정 카탈로그 항목들을 **한 인스턴스에서** 순차로 선택→settle→캡처하고
/// 마지막에 종료한다(콜드스타트 1회). 갤러리는 IPC 가 없어 격리 자동 시각검증을
/// 이 경로로 한다.
struct ShotPlan {
    /// (catalog index, png 경로) 목록.
    items: Vec<(usize, std::path::PathBuf)>,
    /// 현재 캡처 중인 항목.
    current: usize,
    /// 현재 항목을 띄운 뒤 지난 프레임 수(settle 카운터).
    frame: u32,
}

fn parse_shot_env() -> Option<ShotPlan> {
    let raw = std::env::var("TASTY_GALLERY_SHOT").ok()?;
    let items: Vec<(usize, std::path::PathBuf)> = raw
        .split(',')
        .filter_map(|entry| {
            let (idx, path) = entry.split_once(':')?;
            Some((
                idx.trim().parse().ok()?,
                std::path::PathBuf::from(path.trim()),
            ))
        })
        .collect();
    (!items.is_empty()).then_some(ShotPlan {
        items,
        current: 0,
        frame: 0,
    })
}

#[derive(Default)]
struct App {
    runtime: Option<Runtime>,
    shot: Option<ShotPlan>,
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

        let mut rt = pollster::block_on(init_runtime(window)).expect("gallery runtime init");
        // 스크린샷 모드: 첫 페이지를 선택해 둔다 (idx = 페이지 index).
        if let Some(plan) = &self.shot
            && let Some(&(idx, _)) = plan.items.first()
        {
            rt.gallery.select_page(idx);
        }
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
                // 배치 스크린샷: 현재 항목을 4프레임 settle 후 캡처.
                let capture_path = if let Some(plan) = self.shot.as_mut() {
                    plan.frame += 1;
                    (plan.frame >= 4)
                        .then(|| plan.items.get(plan.current).map(|(_, p)| p.clone()))
                        .flatten()
                } else {
                    None
                };
                if let Err(err) = render_frame(rt, capture_path.as_deref()) {
                    tracing::error!("render error: {err:?}");
                }
                // 캡처했으면 다음 항목으로 진행, 끝났으면 종료.
                if capture_path.is_some()
                    && let Some(plan) = self.shot.as_mut()
                {
                    plan.current += 1;
                    plan.frame = 0;
                    match plan.items.get(plan.current) {
                        Some(&(idx, _)) => rt.gallery.select_page(idx),
                        None => event_loop.exit(),
                    }
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
        // COPY_SRC: 스크린샷 모드에서 surface 텍스처를 버퍼로 복사하기 위함.
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
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
    tasty_egui_theme::install_cjk_fallback(&egui_ctx);
    // SVG icon (chevron) loaders.
    egui_extras::install_image_loaders(&egui_ctx);

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

fn render_frame(rt: &mut Runtime, capture: Option<&std::path::Path>) -> anyhow::Result<()> {
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

    // 스크린샷 모드: present 전에 surface 텍스처를 PNG 로 떨군다.
    if let Some(path) = capture {
        capture_to_png(
            &rt.device,
            &rt.queue,
            &frame.texture,
            rt.config.width,
            rt.config.height,
            path,
        );
    }

    frame.present();
    Ok(())
}

/// surface 텍스처(BGRA)를 RGB PNG 로 저장. 본체 `gpu/screenshot.rs` 의 readback
/// 로직과 동일(256B row 정렬, BGRA→RGB, map_async + Wait poll).
fn capture_to_png(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
    path: &std::path::Path,
) {
    let bpp = 4u32;
    let unpadded = width * bpp;
    let padded = (unpadded + 255) & !255;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gallery_screenshot_buffer"),
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gallery_screenshot_encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::Maintain::Wait);
    if let Ok(Ok(())) = rx.recv() {
        let data = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for row in 0..height {
            let off = (row * padded) as usize;
            for col in 0..width {
                let px = off + (col * bpp) as usize;
                pixels.push(data[px + 2]); // R
                pixels.push(data[px + 1]); // G
                pixels.push(data[px]); // B
            }
        }
        drop(data);
        buffer.unmap();
        if let Ok(file) = std::fs::File::create(path) {
            let w = std::io::BufWriter::new(file);
            let mut enc = png::Encoder::new(w, width, height);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            if let Ok(mut writer) = enc.write_header() {
                let _ = writer.write_image_data(&pixels);
                tracing::info!("gallery screenshot saved to {}", path.display());
            }
        }
    } else {
        tracing::warn!("gallery screenshot capture failed");
    }
}
