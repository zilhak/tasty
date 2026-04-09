mod egui_bridge;
mod fonts;
mod render_pass;
mod screenshot;
mod shell_setup;

use std::sync::Arc;

use anyhow::Result;
use winit::dpi::PhysicalSize;
use winit::window::Window;

use winit::event_loop::EventLoopProxy;

use crate::model::Rect;
use crate::renderer::CellRenderer;
use crate::settings::AppearanceSettings;
use crate::state::AppState;
use crate::AppEvent;

pub struct ImePreeditState {
    pub text: String,
    pub cursor: Option<(usize, usize)>,
    pub anchor_col: usize,
    pub anchor_row: usize,
    pub surface_id: u32,
}

/// Actions returned by the shell setup dialog.
pub enum ShellSetupAction {
    None,
    Confirmed,
    Exit,
}

pub struct GpuState {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) size: PhysicalSize<u32>,
    pub(super) renderer: CellRenderer,
    pub(super) egui_ctx: egui::Context,
    pub(super) egui_state: egui_winit::State,
    pub(super) egui_renderer: egui_wgpu::Renderer,
    pub(super) scale_factor: f32,
    /// When set, the next render will capture the frame to this path as PNG.
    pub pending_screenshot: Option<std::path::PathBuf>,
}

impl GpuState {
    pub async fn new(window: Arc<Window>, appearance: &AppearanceSettings, proxy: EventLoopProxy<AppEvent>) -> Result<Self> {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if surface_caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::Fifo
            },
            alpha_mode: surface_caps.alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create renderer with font settings from config
        let effective_font_size = appearance.effective_font_size(scale_factor);
        let renderer = CellRenderer::new(
            &device,
            &queue,
            surface_format,
            effective_font_size,
            &appearance.font_family,
        );

        // egui setup
        let egui_ctx = egui::Context::default();

        // Disable egui's built-in Ctrl+/- zoom — it only affects egui widgets
        // but not the terminal renderer, causing inconsistent scaling.
        egui_ctx.options_mut(|opts| {
            opts.zoom_with_keyboard = false;
        });

        // Load system CJK font so Korean/Japanese/Chinese glyphs render in egui UI
        Self::setup_egui_cjk_fonts(&egui_ctx);

        // Connect egui's repaint requests to the winit event loop.
        // Without this, egui's internal repaints (new window registration,
        // cursor blink, animations) are silently dropped, causing the
        // Settings window to appear only after the next user input.
        let repaint_proxy = proxy;
        egui_ctx.set_request_repaint_callback(move |_| {
            let _ = repaint_proxy.send_event(AppEvent::EguiRepaint);
        });

        // Apply theme from settings
        Self::apply_theme(&egui_ctx, &appearance.theme, appearance.ui_scale_factor());

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
            pending_screenshot: None,
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

    /// Pass a winit event to egui. Returns (consumed, repaint).
    pub fn handle_egui_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> (bool, bool) {
        let response = self.egui_state.on_window_event(window, event);
        (response.consumed, response.repaint)
    }

    /// Render the full frame: egui UI + terminal surfaces.
    pub fn render(
        &mut self,
        state: &mut AppState,
        window: &Window,
        preedit: Option<&ImePreeditState>,
        selection: Option<&crate::selection::TextSelection>,
    ) -> Result<(), wgpu::SurfaceError> {
        // 0. Re-sync scale factor — macOS may not fire ScaleFactorChanged
        // reliably during monitor hot-swap (e.g., 4K → 1080p).
        self.sync_scale_factor(window);

        // 1. Prepare layout
        state.sidebar_width = if !state.sidebar_visible {
            0.0
        } else if state.sidebar_collapsed {
            48.0 // Compact mode: narrow width for collapse button
        } else {
            state.engine.settings.appearance.scaled_sidebar_width()
        };
        let terminal_rect = self.compute_terminal_rect(state.sidebar_width);
        state.resize_all(terminal_rect, self.renderer.cell_width(), self.renderer.cell_height());

        let (pane_rects, dividers, focused_surface_id) = self.prepare_layout(state, terminal_rect);

        // Clear notification highlight on the currently focused surface
        if let Some(sid) = focused_surface_id {
            state.engine.notifications.clear_surface_highlight(sid);
        }

        // 2. Run egui frame (UI drawing)
        let prev_theme = state.engine.settings.appearance.theme.clone();
        let full_output = self.run_egui_frame(state, window, &pane_rects, &dividers, terminal_rect);

        // 3. Post-egui updates (theme/font refresh)
        self.post_egui_update(state, &prev_theme);
        self.egui_state.handle_platform_output(window, full_output.platform_output);

        // 4. Tessellate egui
        let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        // 5. GPU render
        let regions = state.render_regions(terminal_rect);
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        self.render_clear_pass(&view, state);
        self.render_terminals(&view, &regions, focused_surface_id, selection, &state.engine.settings.appearance, preedit);
        self.render_egui_pass(&view, &full_output.textures_delta, &paint_jobs, &screen_descriptor);

        // 6. Screenshot + present
        if let Some(path) = self.pending_screenshot.take() {
            self.capture_frame_to_png(&output.texture, &path);
        }
        output.present();
        Ok(())
    }

    fn compute_terminal_rect(&self, sidebar_width: f32) -> Rect {
        crate::model::compute_terminal_rect(
            self.size.width as f32, self.size.height as f32,
            sidebar_width, self.scale_factor,
        )
    }

    fn prepare_layout(&self, state: &AppState, terminal_rect: Rect) -> (Vec<(u32, Rect)>, Vec<Rect>, Option<u32>) {
        let pane_layout = state.active_workspace().pane_layout();
        let pane_rects: Vec<(u32, Rect)> = pane_layout.compute_rects(terminal_rect);
        let mut dividers: Vec<Rect> = pane_layout.collect_dividers(terminal_rect);

        let focused_surface_id = state.focused_surface_id();
        for (pane_id, pane_rect) in &pane_rects {
            if let Some(pane) = pane_layout.find_pane(*pane_id) {
                let tab_bar_h = state.tab_bar_height;
                let content_rect = Rect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(1.0),
                };
                if let Some(panel) = pane.active_panel() {
                    if let crate::model::Panel::SurfaceGroup(group) = panel {
                        dividers.extend(group.layout().collect_dividers(content_rect));
                    }
                }
            }
        }
        (pane_rects, dividers, focused_surface_id)
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

    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub fn egui_frame_nr(&self) -> u64 {
        self.egui_ctx.cumulative_pass_nr()
    }

    /// Update the scale factor (e.g., when the window moves between monitors with different DPI).
    pub fn update_scale_factor(&mut self, new_scale_factor: f32) {
        self.scale_factor = new_scale_factor;
        // Reconfigure egui with new scale factor
        self.egui_ctx.set_pixels_per_point(new_scale_factor);
    }

    /// Re-sync scale factor from the window and resize if it changed.
    /// Returns true if scale factor was updated.
    pub fn sync_scale_factor(&mut self, window: &Window) -> bool {
        let current_sf = window.scale_factor() as f32;
        if (current_sf - self.scale_factor).abs() > f32::EPSILON {
            self.update_scale_factor(current_sf);
            let new_size = window.inner_size();
            self.resize(new_size);
            true
        } else {
            false
        }
    }
}
