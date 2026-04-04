use std::sync::Arc;

use anyhow::Result;
use winit::dpi::PhysicalSize;
use winit::window::Window;

use winit::event_loop::EventLoopProxy;

use crate::i18n::t;
use crate::model::Rect;
use crate::renderer::CellRenderer;
use crate::settings::AppearanceSettings;
use crate::state::AppState;
use crate::ui;
use crate::AppEvent;

/// Actions returned by the shell setup dialog.
pub enum ShellSetupAction {
    None,
    Confirmed,
    Exit,
}

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
        let renderer = CellRenderer::new(
            &device,
            &queue,
            surface_format,
            appearance.font_size,
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

    /// Render the shell setup dialog (no terminal, just egui).
    pub fn render_shell_setup(
        &mut self,
        window: &Window,
        shell_path: &mut String,
    ) -> Result<ShellSetupAction, wgpu::SurfaceError> {
        let _th = crate::theme::theme();
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let raw_input = self.egui_state.take_egui_input(window);
        let mut action = ShellSetupAction::None;

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            let path_obj = std::path::Path::new(shell_path.as_str());
            let file_name = path_obj
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let is_valid = !shell_path.is_empty()
                && path_obj.exists()
                && (file_name.contains("bash") || file_name.contains("zsh"));
            let show_error = !shell_path.is_empty() && !is_valid;

            // Apply theme from theme module
            let th = crate::theme::theme();
            th.apply_to_egui(ctx, 1.0);

            // Local aliases for this function
            let bg_panel   = th.crust;
            let bg_card    = th.mantle;
            let border     = th.surface0;
            let text_dim   = th.subtext0;
            let amber      = th.yellow;
            let red_err    = th.red;
            let accent_ok  = th.green;
            let accent_dis = th.surface1;

            // Dark background panel
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(bg_panel))
                .show(ctx, |_| {});

            // Centered window dialog
            let content_w = 440.0;
            egui::Window::new("shell_setup")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .fixed_size(egui::vec2(content_w, 0.0))
                .frame(
                    egui::Frame::new()
                        .fill(bg_card)
                        .stroke(egui::Stroke::new(1.0, border))
                        .corner_radius(egui::CornerRadius::same(12))
                        .inner_margin(egui::Margin::symmetric(32, 28))
                        .shadow(egui::Shadow {
                            offset: [0, 8],
                            blur: 24,
                            spread: 0,
                            color: th.crust,
                        }),
                )
                .show(ctx, |ui| {
                    // ── Title ──────────────────────────────────────
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Tasty")
                                .size(30.0)
                                .strong()
                                .color(th.text),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            egui::RichText::new(t("settings.general.setup_subtitle"))
                                .size(11.0)
                                .color(text_dim),
                        );
                    });

                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(12.0);

                    // ── Warning ────────────────────────────────────
                    egui::Frame::new()
                        .fill(th.surface0)
                        .stroke(egui::Stroke::new(1.0, th.surface1))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(12, 10))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(t("settings.general.shell_not_found"))
                                        .size(12.5)
                                        .color(amber),
                                ).wrap(),
                            );
                        });

                    ui.add_space(16.0);

                    // ── Input ──────────────────────────────────────
                    ui.label(
                        egui::RichText::new(t("settings.general.shell_label"))
                            .size(12.0)
                            .color(text_dim),
                    );
                    ui.add_space(4.0);

                    let response = ui.add_sized(
                        [ui.available_width(), 32.0],
                        egui::TextEdit::singleline(shell_path)
                            .hint_text("C:/Program Files/Git/bin/bash.exe")
                            .font(egui::TextStyle::Monospace),
                    );

                    // ── Error / success hint ──────────────────────
                    ui.add_space(4.0);
                    if show_error {
                        ui.label(
                            egui::RichText::new(t("settings.general.shell_invalid_path"))
                                .size(11.5)
                                .color(red_err),
                        );
                    } else if is_valid {
                        ui.label(
                            egui::RichText::new(t("settings.general.shell_valid"))
                                .size(11.5)
                                .color(accent_ok),
                        );
                    } else {
                        ui.add_space(14.0); // reserve space
                    }

                    ui.add_space(16.0);

                    // ── Buttons ────────────────────────────────────
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            let btn_size = egui::vec2(110.0, 34.0);

                            // Cancel
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(t("button.cancel")).size(13.0).color(text_dim),
                                )
                                .min_size(btn_size)
                                .fill(th.base)
                                .stroke(egui::Stroke::new(1.0, border))
                                .corner_radius(egui::CornerRadius::same(6)),
                            ).clicked() {
                                action = ShellSetupAction::Exit;
                            }

                            ui.add_space(10.0);

                            // OK
                            let (ok_fill, ok_stroke, ok_text) = if is_valid {
                                (
                                    th.green,
                                    egui::Stroke::new(1.0, th.green),
                                    th.base,
                                )
                            } else {
                                (
                                    accent_dis,
                                    egui::Stroke::new(1.0, th.surface2),
                                    th.overlay0,
                                )
                            };

                            let ok_resp = ui.add_enabled(is_valid,
                                egui::Button::new(
                                    egui::RichText::new("OK").size(13.0).strong().color(ok_text),
                                )
                                .min_size(btn_size)
                                .fill(ok_fill)
                                .stroke(ok_stroke)
                                .corner_radius(egui::CornerRadius::same(6)),
                            );
                            if ok_resp.clicked()
                                || (response.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    && is_valid)
                            {
                                action = ShellSetupAction::Confirmed;
                            }
                        });
                    });
                });
        });

        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: self.scale_factor,
        };
        let tris = self.egui_ctx.tessellate(full_output.shapes, self.scale_factor);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, delta);
        }

        let mut update_encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("egui_update") },
        );
        self.egui_renderer.update_buffers(
            &self.device, &self.queue, &mut update_encoder, &tris, &screen_descriptor,
        );
        self.queue.submit(std::iter::once(update_encoder.finish()));

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("shell_setup_encoder"),
        });
        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shell_setup_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.12, g: 0.12, b: 0.14, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut render_pass = render_pass.forget_lifetime();
            self.egui_renderer.render(&mut render_pass, &tris, &screen_descriptor);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        Ok(action)
    }

    /// Render the full frame: egui UI + terminal surfaces.
    pub fn render(
        &mut self,
        state: &mut AppState,
        window: &Window,
        preedit: &str,
        selection: Option<&crate::selection::TextSelection>,
    ) -> Result<(), wgpu::SurfaceError> {
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
        let full_output = self.run_egui_frame(state, window, &pane_rects, &dividers, terminal_rect, preedit);

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
        self.render_terminals(&view, &regions, focused_surface_id, selection, &state.engine.settings.appearance);
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

    fn run_egui_frame(
        &mut self,
        state: &mut AppState,
        window: &Window,
        pane_rects: &[(u32, Rect)],
        dividers: &[Rect],
        terminal_rect: Rect,
        preedit: &str,
    ) -> egui::FullOutput {
        let raw_input = self.egui_state.take_egui_input(window);
        let scale_factor = self.scale_factor;
        let cell_w = self.renderer.cell_width();
        let cell_h = self.renderer.cell_height();

        self.egui_ctx.run(raw_input, |ctx| {
            ui::draw_ui(ctx, state, scale_factor);
            ui::draw_ws_rename_dialog(ctx, state);
            ui::draw_pane_dividers(ctx, dividers, scale_factor);
            ui::draw_surface_highlights(ctx, state, terminal_rect, scale_factor);
            ui::draw_pane_tab_bars(ctx, state, pane_rects, scale_factor);
            ui::draw_non_terminal_panels(ctx, state, pane_rects, scale_factor);
            ui::draw_pane_context_menu(ctx, state, scale_factor);
            ui::draw_markdown_path_dialog(ctx, state);
            ui::draw_notification_panel(ctx, state);

            // IME preedit overlay — draw at the focused surface's actual position
            if !preedit.is_empty() {
                let focused_sid = state.focused_surface_id();
                if let Some(terminal) = state.focused_terminal() {
                    let (cx, cy) = terminal.surface().cursor_position();

                    // Find the actual rect of the focused surface within the layout
                    let regions = state.render_regions(terminal_rect);
                    let mut surface_rect = None;
                    for (_pane_id, _pane_rect, terminal_regions) in &regions {
                        for (sid, _term, rect) in terminal_regions {
                            if Some(*sid) == focused_sid {
                                surface_rect = Some(*rect);
                                break;
                            }
                        }
                        if surface_rect.is_some() { break; }
                    }

                    if let Some(rect) = surface_rect {
                        let px = (rect.x + cx as f32 * cell_w) / scale_factor;
                        let py = (rect.y + cy as f32 * cell_h) / scale_factor;

                        let th = crate::theme::theme();
                        let painter = ctx.layer_painter(egui::LayerId::new(
                            egui::Order::Foreground,
                            egui::Id::new("ime_preedit"),
                        ));
                        let font_id = egui::FontId::monospace(cell_h / scale_factor);
                        let galley = painter.layout_no_wrap(preedit.to_string(), font_id, th.base);
                        let text_rect = egui::Rect::from_min_size(egui::pos2(px, py), galley.size());
                        painter.rect_filled(text_rect, 0.0, th.blue);
                        painter.galley(egui::pos2(px, py), galley, th.base);
                    }
                }
            }

            // Settings UI is now rendered in the modal window (ModalWindow)
        })
    }

    fn post_egui_update(&mut self, state: &AppState, prev_theme: &str) {
        let ui_scale = state.engine.settings.appearance.ui_scale_factor();
        if state.engine.settings.appearance.theme != prev_theme {
            self.refresh_theme(&state.engine.settings.appearance.theme, ui_scale);
        }
        // Always re-apply UI scale (in case it changed)
        crate::theme::theme().apply_to_egui(&self.egui_ctx, ui_scale);

        let current_font_size = self.renderer.font_config.metrics.font_size;
        let current_font_family = match &self.renderer.font_config.font_family {
            cosmic_text::FamilyOwned::Monospace => String::new(),
            cosmic_text::FamilyOwned::Name(name) => name.to_string(),
            _ => String::new(),
        };
        if state.engine.settings.appearance.font_size != current_font_size
            || state.engine.settings.appearance.font_family != current_font_family
        {
            self.renderer.update_font(
                &self.device, &self.queue,
                state.engine.settings.appearance.font_size,
                &state.engine.settings.appearance.font_family,
            );
            self.renderer.resize(&self.queue, self.size.width, self.size.height);
        }
    }

    fn render_clear_pass(&self, view: &wgpu::TextureView, state: &AppState) {
        let bg_alpha = state.engine.settings.appearance.background_opacity as f64;
        let th = crate::theme::theme();
        let bg = crate::theme::Theme::to_float(th.base);

        let mut encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("clear_pass") },
        );
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: bg_alpha,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    fn render_terminals(
        &mut self,
        view: &wgpu::TextureView,
        regions: &[(u32, Rect, Vec<(u32, &tasty_terminal::Terminal, Rect)>)],
        focused_surface_id: Option<u32>,
        selection: Option<&crate::selection::TextSelection>,
        settings: &crate::settings::AppearanceSettings,
    ) {
        let theme = crate::theme::theme();
        for (_pane_id, _pane_rect, terminal_regions) in regions {
            for (surface_id, terminal, rect) in terminal_regions {
                let is_focused = focused_surface_id == Some(*surface_id);
                let bg = if is_focused {
                    settings.focused_surface_bg_float()
                } else {
                    crate::renderer::DEFAULT_BG
                };

                // Build selection info for this surface
                let sel_info = selection
                    .filter(|s| s.surface_id == *surface_id && !s.is_empty())
                    .map(|s| (s.normalized(), theme.selection_bg));
                let sel_ref = sel_info.as_ref();

                self.renderer.prepare_terminal_viewport(
                    terminal, &self.queue, rect,
                    self.size.width, self.size.height, bg, is_focused,
                    sel_ref,
                );

                let mut term_encoder = self.device.create_command_encoder(
                    &wgpu::CommandEncoderDescriptor { label: Some("terminal_pass") },
                );
                {
                    let mut render_pass = term_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("terminal_pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
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
                self.queue.submit(std::iter::once(term_encoder.finish()));
            }
        }
    }

    fn render_egui_pass(
        &mut self,
        view: &wgpu::TextureView,
        textures_delta: &egui::TexturesDelta,
        paint_jobs: &[egui::ClippedPrimitive],
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
    ) {
        for (id, image_delta) in &textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let mut egui_encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor { label: Some("egui_encoder") },
        );

        self.egui_renderer.update_buffers(
            &self.device, &self.queue, &mut egui_encoder, paint_jobs, screen_descriptor,
        );

        {
            let render_pass = egui_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
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
            let mut render_pass = render_pass.forget_lifetime();
            self.egui_renderer.render(&mut render_pass, paint_jobs, screen_descriptor);
        }

        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.queue.submit(std::iter::once(egui_encoder.finish()));
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

    /// Load a system CJK font into egui so that Korean/Japanese/Chinese text
    /// renders correctly in the UI (e.g., language selector in Settings).
    fn setup_egui_cjk_fonts(ctx: &egui::Context) {
        let font_bytes = Self::load_system_cjk_font();
        let Some(bytes) = font_bytes else {
            tracing::warn!("no system CJK font found; UI may show □ for CJK text");
            return;
        };

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );

        // Append as fallback so Latin text still uses egui's default fonts
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("system_cjk".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("system_cjk".to_owned());

        ctx.set_fonts(fonts);
    }

    fn load_system_cjk_font() -> Option<Vec<u8>> {
        #[cfg(target_os = "windows")]
        {
            // Malgun Gothic (맑은 고딕) — bundled with Windows Vista+
            let path = "C:/Windows/Fonts/malgun.ttf";
            if let Ok(data) = std::fs::read(path) {
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

    /// Apply the theme to the egui context.
    fn apply_theme(ctx: &egui::Context, _theme: &str, ui_scale: f32) {
        crate::theme::theme().apply_to_egui(ctx, ui_scale);
    }

    /// Re-apply the theme from settings. Called after settings are saved.
    pub fn refresh_theme(&self, theme: &str, ui_scale: f32) {
        Self::apply_theme(&self.egui_ctx, theme, ui_scale);
    }

    /// Capture the current frame texture to a PNG file.
    fn capture_frame_to_png(&self, texture: &wgpu::Texture, path: &std::path::Path) {
        let width = self.size.width;
        let height = self.size.height;
        let bytes_per_pixel = 4u32;
        // wgpu requires rows to be aligned to 256 bytes
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let padded_bytes_per_row = (unpadded_bytes_per_row + 255) & !255;

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot_buffer"),
            size: (padded_bytes_per_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("screenshot_encoder"),
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
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        // Map the buffer and read pixels
        let buffer_slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);

        if let Ok(Ok(())) = rx.recv() {
            let data = buffer_slice.get_mapped_range();

            // Convert BGRA -> RGB for PNG encoding
            let mut pixels = Vec::with_capacity((width * height * 3) as usize);
            for row in 0..height {
                let offset = (row * padded_bytes_per_row) as usize;
                for col in 0..width {
                    let px = offset + (col * bytes_per_pixel) as usize;
                    // BGRA → RGB
                    pixels.push(data[px + 2]); // R
                    pixels.push(data[px + 1]); // G
                    pixels.push(data[px]);     // B
                }
            }
            drop(data);
            buffer.unmap();

            // Write as PNG
            if let Ok(file) = std::fs::File::create(path) {
                let writer = std::io::BufWriter::new(file);
                let mut encoder = png::Encoder::new(writer, width, height);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                if let Ok(mut writer) = encoder.write_header() {
                    let _ = writer.write_image_data(&pixels);
                    tracing::info!("screenshot saved to {}", path.display());
                }
            }
        } else {
            tracing::warn!("failed to capture screenshot");
        }
    }

    // ── Generic egui helpers (for modal windows) ──

    /// Take egui input from a window.
    pub fn take_egui_input(&mut self, window: &Window) -> egui::RawInput {
        self.egui_state.take_egui_input(window)
    }

    /// Run an egui frame with a custom UI closure.
    pub fn run_egui(&self, raw_input: egui::RawInput, ui_fn: impl FnMut(&egui::Context)) -> egui::FullOutput {
        self.egui_ctx.run(raw_input, ui_fn)
    }

    /// Finish an egui frame: tessellate, render, present.
    pub fn finish_egui_frame(&mut self, window: &Window, full_output: egui::FullOutput) {
        self.egui_state.handle_platform_output(window, full_output.platform_output);

        let paint_jobs = self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        let output = match self.surface.get_current_texture() {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!("modal surface error: {e}");
                return;
            }
        };
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Clear
        let th = crate::theme::theme();
        let bg = crate::theme::Theme::to_float(th.base);
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("modal_clear") });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("modal_clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        // Egui render
        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, *id, image_delta);
        }
        let mut egui_encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("modal_egui") });
        self.egui_renderer.update_buffers(&self.device, &self.queue, &mut egui_encoder, &paint_jobs, &screen_descriptor);
        {
            let render_pass = egui_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("modal_egui_pass"),
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
            let mut render_pass = render_pass.forget_lifetime();
            self.egui_renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }
        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
        self.queue.submit(std::iter::once(egui_encoder.finish()));

        output.present();
    }
}
