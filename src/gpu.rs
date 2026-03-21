use std::sync::Arc;

use anyhow::Result;
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::model::Rect;
use crate::renderer::CellRenderer;
use crate::settings::AppearanceSettings;
use crate::settings_ui;
use crate::state::AppState;
use crate::ui;

pub struct GpuState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: PhysicalSize<u32>,
    renderer: CellRenderer,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    scale_factor: f32,
}

impl GpuState {
    pub async fn new(window: Arc<Window>, appearance: &AppearanceSettings) -> Result<Self> {
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
            .ok_or_else(|| anyhow::anyhow!("no compatible GPU adapter found"))?;

        tracing::info!(
            "GPU adapter: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("tasty_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    ..Default::default()
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
            .ok_or_else(|| anyhow::anyhow!("no supported surface format found"))?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create renderer with font settings from config
        let renderer = CellRenderer::new(
            &device,
            &queue,
            surface_format,
            appearance.font_size,
            &appearance.font_family,
        );

        // egui setup
        let egui_ctx = egui::Context::default();

        // Apply theme from settings
        Self::apply_theme(&egui_ctx, &appearance.theme);

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            &window,
            Some(scale_factor),
            None,
            Some(2048),
        );

        let egui_renderer =
            egui_wgpu::Renderer::new(&device, surface_format, None, 1, false);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            size,
            renderer,
            egui_ctx,
            egui_state,
            egui_renderer,
            scale_factor,
        })
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        self.renderer
            .resize(&self.queue, new_size.width, new_size.height);
    }

    /// Pass a winit event to egui. Returns true if egui consumed the event.
    pub fn handle_egui_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        let response = self.egui_state.on_window_event(window, event);
        response.consumed
    }

    /// Render the full frame: egui UI + terminal surfaces.
    pub fn render(&mut self, state: &mut AppState, window: &Window) -> Result<(), wgpu::SurfaceError> {
        // Sync sidebar_width from settings (in case it was changed in settings UI)
        state.sidebar_width = state.settings.appearance.sidebar_width;

        // Compute the terminal rect (area after sidebar) in physical pixels
        let surface_w = self.size.width as f32;
        let surface_h = self.size.height as f32;
        let terminal_rect = crate::model::compute_terminal_rect(surface_w, surface_h, state.sidebar_width, self.scale_factor);

        // Compute pane rects for per-pane tab bars
        let pane_rects: Vec<(u32, Rect)> = state
            .active_workspace()
            .pane_layout()
            .compute_rects(terminal_rect);

        // 1. Begin egui frame
        let raw_input = self.egui_state.take_egui_input(window);
        let scale_factor = self.scale_factor;
        let prev_theme = state.settings.appearance.theme.clone();
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            ui::draw_ui(ctx, state, scale_factor);
            ui::draw_pane_tab_bars(ctx, state, &pane_rects, scale_factor);
            ui::draw_notification_panel(ctx, state);
            if state.settings_open {
                let mut settings = state.settings.clone();
                let mut open = state.settings_open;
                settings_ui::draw_settings_window(
                    ctx,
                    &mut settings,
                    &mut open,
                    &mut state.settings_ui_state,
                );
                state.settings = settings;
                state.settings_open = open;
            }
        });

        // Refresh egui theme if it changed (e.g., after settings save)
        if state.settings.appearance.theme != prev_theme {
            self.refresh_theme(&state.settings.appearance.theme);
        }

        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        // Tessellate egui shapes
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        // Use the proper egui update path
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        // 2. Get render regions for terminals (per-pane)
        let regions = state.render_regions(terminal_rect);

        // 3. Get the output texture
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render_encoder"),
            });

        // 4. Clear pass (apply background_opacity from settings)
        let bg_alpha = state.settings.appearance.background_opacity as f64;
        let (clear_r, clear_g, clear_b) = if state.settings.appearance.theme == "light" {
            (0.941, 0.941, 0.957) // light theme bg
        } else {
            (0.102, 0.102, 0.118) // dark theme bg
        };
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_r,
                            g: clear_g,
                            b: clear_b,
                            a: bg_alpha,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        // 5. Render each terminal surface in its viewport rect
        for (_pane_id, _pane_rect, terminal_regions) in &regions {
            for (_surface_id, terminal, rect) in terminal_regions {
                self.renderer.prepare_viewport(
                    terminal.surface(),
                    &self.queue,
                    rect,
                    self.size.width,
                    self.size.height,
                );

                let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("terminal_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });

                self.renderer.render_scissored(&mut render_pass, rect, self.size.width, self.size.height);
            }
        }

        // 6. Render egui on top
        // Update egui textures and buffers
        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        // Submit terminal commands first
        self.queue.submit(std::iter::once(encoder.finish()));

        // Create a separate encoder for egui
        let mut egui_encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui_encoder"),
            });

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut egui_encoder,
            &paint_jobs,
            &screen_descriptor,
        );

        {
            let render_pass = egui_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // egui-wgpu requires RenderPass<'static>; we use forget_lifetime() to opt out
            // of the compile-time encoder guard since we manage the ordering manually.
            let mut render_pass = render_pass.forget_lifetime();
            self.egui_renderer
                .render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        // Free egui textures
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.queue.submit(std::iter::once(egui_encoder.finish()));
        output.present();

        Ok(())
    }

    /// Compute terminal grid dimensions from current window size.
    pub fn grid_size(&self) -> (usize, usize) {
        self.renderer.grid_size(self.size.width, self.size.height)
    }

    /// Compute grid size for a given rect.
    pub fn grid_size_for_rect(&self, rect: &Rect) -> (usize, usize) {
        self.renderer.grid_size_for_rect(rect)
    }

    pub fn cell_width(&self) -> f32 {
        self.renderer.cell_width()
    }

    pub fn cell_height(&self) -> f32 {
        self.renderer.cell_height()
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Update the scale factor (e.g., when the window moves between monitors with different DPI).
    pub fn update_scale_factor(&mut self, new_scale_factor: f32) {
        self.scale_factor = new_scale_factor;
        // Reconfigure egui with new scale factor
        self.egui_ctx.set_pixels_per_point(new_scale_factor);
    }

    /// Apply a theme ("dark" or "light") to the egui context.
    fn apply_theme(ctx: &egui::Context, theme: &str) {
        if theme == "light" {
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = egui::Color32::from_rgb(240, 240, 244);
            visuals.window_fill = egui::Color32::from_rgb(240, 240, 244);
            visuals.extreme_bg_color = egui::Color32::from_rgb(250, 250, 252);
            ctx.set_visuals(visuals);
        } else {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = egui::Color32::from_rgb(30, 30, 36);
            visuals.window_fill = egui::Color32::from_rgb(30, 30, 36);
            visuals.extreme_bg_color = egui::Color32::from_rgb(20, 20, 24);
            ctx.set_visuals(visuals);
        }
    }

    /// Re-apply the theme from settings. Called after settings are saved.
    pub fn refresh_theme(&self, theme: &str) {
        Self::apply_theme(&self.egui_ctx, theme);
    }
}
