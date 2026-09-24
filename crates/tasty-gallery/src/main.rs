#![forbid(unsafe_code)]

//! winit·wgpu·egui로 갤러리를 실행한다. 본체의 터미널·플러그인 실행은 포함하지 않는다.

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

/// TASTY_GALLERY_SHOT=<idx>[@<y>]:<png>[,...]로 페이지별 캡처를 지정한다.
/// 한 인스턴스에서 페이지 선택·대기·캡처를 반복한 뒤 종료한다. y는 본문 스크롤 위치다.
struct ShotPlan {
    /// (catalog index, 스크롤 오프셋, png 경로) 목록.
    items: Vec<(usize, f32, std::path::PathBuf)>,
    /// 현재 캡처 중인 항목.
    current: usize,
    /// 현재 항목을 띄운 뒤 지난 프레임 수(settle 카운터).
    frame: u32,
}

fn parse_shot_env() -> Option<ShotPlan> {
    let raw = std::env::var("TASTY_GALLERY_SHOT").ok()?;
    let items: Vec<(usize, f32, std::path::PathBuf)> = raw
        .split(',')
        .filter_map(|entry| {
            let (head, path) = entry.split_once(':')?;
            let (idx, y) = match head.split_once('@') {
                Some((i, y)) => (i, y.trim().parse().ok()?),
                None => (head, 0.0),
            };
            Some((
                idx.trim().parse().ok()?,
                y,
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

/// TASTY_GALLERY_SIZE=<w>x<h>로 창 크기를 지정한다. 넓은 예제의 캡처에 사용한다.
fn window_size() -> (f64, f64) {
    const DEFAULT: (f64, f64) = (1100.0, 720.0);
    let Ok(raw) = std::env::var("TASTY_GALLERY_SIZE") else {
        return DEFAULT;
    };
    let Some((w, h)) = raw.split_once('x') else {
        return DEFAULT;
    };
    match (w.trim().parse(), h.trim().parse()) {
        (Ok(w), Ok(h)) => (w, h),
        _ => DEFAULT,
    }
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
        let (w, h) = window_size();
        let attrs = WindowAttributes::default()
            .with_title("Tasty Gallery")
            .with_inner_size(winit::dpi::LogicalSize::new(w, h));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

        let mut rt = pollster::block_on(init_runtime(window)).expect("gallery runtime init");
        if let Some(plan) = &self.shot
            && let Some(&(idx, y, _)) = plan.items.first()
        {
            rt.gallery.select_page(idx);
            rt.gallery.shot_scroll = Some(y);
        }
        self.runtime = Some(rt);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(rt) = self.runtime.as_mut() else {
            return;
        };

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
                // 배치 캡처는 페이지를 선택한 뒤 4프레임 기다린다.
                let capture_path = if let Some(plan) = self.shot.as_mut() {
                    plan.frame += 1;
                    (plan.frame >= 4)
                        .then(|| plan.items.get(plan.current).map(|(_, _, p)| p.clone()))
                        .flatten()
                } else {
                    None
                };
                if let Err(err) = render_frame(rt, capture_path.as_deref()) {
                    tracing::error!("render error: {err:?}");
                }
                if capture_path.is_some()
                    && let Some(plan) = self.shot.as_mut()
                {
                    plan.current += 1;
                    plan.frame = 0;
                    match plan.items.get(plan.current) {
                        Some(&(idx, y, _)) => {
                            rt.gallery.select_page(idx);
                            rt.gallery.shot_scroll = Some(y);
                        }
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
    tasty_gallery::fonts::install(&egui_ctx);
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

    // present 전에 텍스처를 읽어 PNG로 저장한다.
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

/// BGRA 텍스처를 읽어 RGB PNG로 저장한다. 행은 GPU 복사 조건에 맞춰 256바이트로 정렬한다.
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
        let _ = tx.send(r); // 수신자가 사라지면 캡처 결과를 전달할 곳이 없으므로 무시한다.
    });
    let _ = device.poll(wgpu::Maintain::Wait); // map 콜백 완료를 기다린다. 반환된 큐 상태는 사용하지 않는다.
    if !matches!(rx.recv(), Ok(Ok(()))) {
        tracing::warn!("gallery screenshot capture failed");
        return;
    }

    let data = slice.get_mapped_range();
    let pixels = bgra_to_rgb_rows(&data, width, height, padded, bpp);
    drop(data);
    buffer.unmap();

    write_rgb_png(path, width, height, &pixels);
}

/// row-padded BGRA 버퍼 → 패딩 없는 RGB 픽셀 버퍼 변환.
fn bgra_to_rgb_rows(data: &[u8], width: u32, height: u32, padded: u32, bpp: u32) -> Vec<u8> {
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
    pixels
}

/// RGB 픽셀 버퍼를 PNG 파일로 저장.
fn write_rgb_png(path: &std::path::Path, width: u32, height: u32, pixels: &[u8]) {
    let Ok(file) = std::fs::File::create(path) else {
        return;
    };
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, width, height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let Ok(mut writer) = enc.write_header() else {
        return;
    };
    if let Err(e) = writer.write_image_data(pixels) {
        tracing::warn!(
            "gallery screenshot write failed for {}: {e}",
            path.display()
        );
    } else {
        tracing::info!("gallery screenshot saved to {}", path.display());
    }
}
