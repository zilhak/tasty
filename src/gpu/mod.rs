mod canvas_prepare;
pub mod canvas_texture;
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

use crate::AppEvent;
use crate::model::{LogicalPx, PhysicalPx, Rect};
use crate::renderer::CellRenderer;
use crate::settings::AppearanceSettings;
use crate::state::AppState;

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
    /// Tracks per-surface egui font signatures so we re-register only on change.
    pub(super) surface_font_state: crate::ui::font_registry::SurfaceFontState,
    /// Plugin Canvas SharedBuffer → wgpu texture cache.
    pub(super) canvas_textures: canvas_texture::CanvasTextureCache,
    /// When set, the next render will capture the frame to this path as PNG.
    pub pending_screenshot: Option<std::path::PathBuf>,
}

impl GpuState {
    pub async fn new(
        window: Arc<Window>,
        appearance: &AppearanceSettings,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Result<Self> {
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
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
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
            present_mode: if surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
            {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::Fifo
            },
            alpha_mode: surface_caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create renderer with effective terminal font settings.
        let term_font = appearance.effective_terminal_font();
        let effective_font_size = term_font.effective_font_size(scale_factor);
        let renderer = CellRenderer::new(
            &device,
            &queue,
            surface_format,
            effective_font_size,
            &term_font.font_family,
        );

        // egui setup
        let egui_ctx = egui::Context::default();

        // Disable egui's built-in Ctrl+/- zoom — it only affects egui widgets
        // but not the terminal renderer, causing inconsistent scaling.
        egui_ctx.options_mut(|opts| {
            opts.zoom_with_keyboard = false;
        });

        // Register bundled D2Coding (primary monospace) and system CJK fallback in egui.
        Self::setup_egui_fonts(&egui_ctx);

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

        let egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1, false);

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
            surface_font_state: crate::ui::font_registry::SurfaceFontState::default(),
            canvas_textures: canvas_texture::CanvasTextureCache::new(),
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
    pub fn handle_egui_event(
        &mut self,
        window: &Window,
        event: &winit::event::WindowEvent,
    ) -> (bool, bool) {
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
        link_hover: Option<(u32, &crate::terminal_link::LinkHighlight)>,
        plugin_manager: Option<&crate::plugin::PluginManager>,
    ) -> Result<(), wgpu::SurfaceError> {
        let render_start = std::time::Instant::now();

        // 1. Prepare layout
        state.sidebar_width = if !state.sidebar_visible {
            LogicalPx(0.0)
        } else if state.sidebar_collapsed {
            LogicalPx(48.0) // Compact mode: narrow width for collapse button
        } else {
            state.engine.settings.appearance.scaled_sidebar_width()
        };
        let terminal_rect = self.compute_terminal_rect(state.sidebar_width);
        state.resize_all(
            terminal_rect,
            self.renderer.cell_width(),
            self.renderer.cell_height(),
        );

        let (pane_rects, dividers, focused_surface_id) = self.prepare_layout(state, terminal_rect);

        // Clear notification highlight on the currently focused surface
        if let Some(sid) = focused_surface_id {
            state.engine.notifications.clear_surface_highlight(sid);
        }

        let layout_ms = render_start.elapsed().as_secs_f64() * 1000.0;

        // 2. Pre-egui updates: register surface fonts before drawing.
        // Markdown panels reference the named font family "font_markdown". It must
        // be registered before run_egui_frame, or the first frame panics with
        // "FontFamily::Name(...) is not bound to any fonts".
        let prev_theme = state.engine.settings.appearance.theme.clone();
        crate::ui::font_registry::refresh_surface_fonts(
            &self.egui_ctx,
            &state.engine.settings.appearance,
            &mut self.surface_font_state,
        );

        // 2b. Plugin Canvas 텍스처 prepare. egui 프레임 시작 전 GPU 자원 갱신.
        if let Some(mgr) = plugin_manager {
            self.prepare_plugin_canvases(state, mgr);
        }

        // 3. Run egui frame (UI drawing)
        let t0 = std::time::Instant::now();
        let mut full_output = self.run_egui_frame(
            state,
            window,
            &pane_rects,
            &dividers,
            terminal_rect,
            plugin_manager,
        );
        let egui_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 3. Cursor decision: egui first, then winit area (dividers + surfaces)
        if !self.egui_ctx.is_pointer_over_area() && !state.popup_hovered {
            if let Some(pos) = self.egui_ctx.input(|i| i.pointer.hover_pos()) {
                let px = pos.x * self.scale_factor;
                let py = pos.y * self.scale_factor;
                if let Some(icon) = state.winit_cursor_icon_at(px, py, terminal_rect, 4.0) {
                    full_output.platform_output.cursor_icon = icon;
                }
            }
        }
        // Link hover overrides cursor to pointing-hand.
        if link_hover.is_some() {
            full_output.platform_output.cursor_icon = egui::CursorIcon::PointingHand;
        }

        // 4. Post-egui updates (theme/font refresh)
        let t0 = std::time::Instant::now();
        self.post_egui_update(state, &prev_theme);
        let post_egui_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // egui-winit disables IME when no egui text field is focused
        // (calls set_ime_allowed(false) when self.allow_ime differs from
        // ime.is_some()). The terminal always needs IME active.
        // Pre-set allow_ime=false so that when egui computes allow_ime=false
        // (no text field), the check false!=false is false and it skips
        // the set_ime_allowed(false) call entirely.
        let t0 = std::time::Instant::now();
        self.egui_state.set_allow_ime(false);
        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        // popup이 focused면 IME를 비활성화하여 KeyboardInput이 직접 발생하도록 한다.
        // 이렇게 하면 한글 IME 활성 상태에서도 popup 단축키가 physical_key로 매칭된다.
        //
        // Windows 예외: winit Windows의 set_ime_allowed는 ImmAssociateContextEx(IACE_DEFAULT/
        // IACE_CHILDREN)로 IMC를 매번 attach/detach시킨다. 이 association churn이 한/영 키
        // (VK_HANGUL) 토글을 가끔 망가뜨린다(다른 앱으로 갔다 오면 풀리는 증상의 원인).
        // Windows winit은 IME 활성 상태에서도 KeyboardInput과 physical_key를 정상 emit하므로,
        // popup 단축키 매칭에 IME 비활성화가 필요 없다. 따라서 Windows는 항상 IME를 허용한다.
        #[cfg(not(windows))]
        {
            let disable_ime = state.popups.has_focused() && !state.has_input_dialog_open();
            window.set_ime_allowed(!disable_ime);
        }
        #[cfg(windows)]
        window.set_ime_allowed(true);
        let platform_output_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 5. Tessellate egui
        let t0 = std::time::Instant::now();
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let tessellate_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 5. GPU render
        let t0 = std::time::Instant::now();
        let regions = state.surface_regions(terminal_rect);
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.render_clear_pass(&view, state);
        let clear_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t0 = std::time::Instant::now();
        let search_state = if state.search.matches.is_empty() {
            None
        } else {
            Some(&state.search)
        };
        self.render_terminals(
            &view,
            &regions,
            focused_surface_id,
            selection,
            &state.engine.settings.appearance,
            preedit,
            link_hover,
            search_state,
        );
        let terminals_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t0 = std::time::Instant::now();
        self.render_egui_pass(
            &view,
            &full_output.textures_delta,
            &paint_jobs,
            &screen_descriptor,
        );
        let egui_pass_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 6. Screenshot + present
        let t0 = std::time::Instant::now();
        if let Some(path) = self.pending_screenshot.take() {
            self.capture_frame_to_png(&output.texture, &path);
        }
        output.present();
        let present_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // --- GPU render timing ---
        let gpu_total_ms = render_start.elapsed().as_secs_f64() * 1000.0;
        const SLOW_RENDER_MS: f64 = 30.0;
        if gpu_total_ms > SLOW_RENDER_MS {
            tracing::warn!(
                "slow gpu render: {gpu_total_ms:.1}ms \
                 [layout={layout_ms:.1}, egui_frame={egui_frame_ms:.1}, \
                 post_egui={post_egui_ms:.1}, platform_output={platform_output_ms:.1}, \
                 tessellate={tessellate_ms:.1}, clear={clear_ms:.1}, \
                 terminals={terminals_ms:.1}, egui_pass={egui_pass_ms:.1}, \
                 present={present_ms:.1}]"
            );
        }

        Ok(())
    }

    fn compute_terminal_rect(&self, sidebar_width: LogicalPx) -> Rect {
        crate::model::compute_terminal_rect(
            PhysicalPx(self.size.width as f32),
            PhysicalPx(self.size.height as f32),
            sidebar_width,
            self.scale_factor,
        )
    }

    fn prepare_layout(
        &self,
        state: &AppState,
        terminal_rect: Rect,
    ) -> (Vec<(u32, Rect)>, Vec<Rect>, Option<u32>) {
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
                    height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
                };
                if let Some(tab) = pane.tabs.get(pane.active_tab) {
                    dividers.extend(tab.layout().collect_dividers(content_rect));
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

    /// Get egui's actual pixels_per_point (what it uses for rendering).
    pub fn egui_pixels_per_point(&self) -> f32 {
        self.egui_ctx.pixels_per_point()
    }

    /// Get egui's zoom factor.
    pub fn egui_zoom_factor(&self) -> f32 {
        self.egui_ctx.zoom_factor()
    }

    /// Whether egui-winit currently allows IME on the window.
    pub fn egui_ime_allowed(&self) -> bool {
        self.egui_state.allow_ime()
    }

    /// Get the wgpu surface config dimensions.
    pub fn surface_config_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Update the scale factor (e.g., when the window moves between monitors with different DPI).
    pub fn update_scale_factor(&mut self, new_scale_factor: f32) {
        self.scale_factor = new_scale_factor;
        // Reset egui zoom to 1.0 so that egui_winit's native_pixels_per_point
        // (provided via take_egui_input each frame) is used directly.
        // DO NOT call set_pixels_per_point() here — it computes
        // zoom = ppp / native_ppp, and if native_ppp hasn't been updated yet
        // (e.g., during macOS auto-restore), zoom gets a stale value like 0.5
        // that persists forever.
        self.egui_ctx.set_zoom_factor(1.0);
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
