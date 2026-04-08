use winit::window::Window;

use crate::model::Rect;
use crate::state::AppState;
use crate::ui;

use super::GpuState;

impl GpuState {
    pub(super) fn run_egui_frame(
        &mut self,
        state: &mut AppState,
        window: &Window,
        pane_rects: &[(u32, Rect)],
        dividers: &[Rect],
        terminal_rect: Rect,
    ) -> egui::FullOutput {
        let raw_input = self.egui_state.take_egui_input(window);
        let scale_factor = self.scale_factor;

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

            // Settings UI is now rendered in the modal window (ModalWindow)
        })
    }

    pub(super) fn post_egui_update(&mut self, state: &AppState, prev_theme: &str) {
        let ui_scale = state.engine.settings.appearance.ui_scale_factor();
        if state.engine.settings.appearance.theme != prev_theme {
            self.refresh_theme(&state.engine.settings.appearance.theme, ui_scale);
        }
        // Always re-apply UI scale (in case it changed)
        crate::theme::theme().apply_to_egui(&self.egui_ctx, ui_scale);

        let effective_font_size = state.engine.settings.appearance.effective_font_size(self.scale_factor);
        let current_font_size = self.renderer.font_config.metrics.font_size;
        let current_font_family = match &self.renderer.font_config.font_family {
            cosmic_text::FamilyOwned::Monospace => String::new(),
            cosmic_text::FamilyOwned::Name(name) => name.to_string(),
            _ => String::new(),
        };
        if effective_font_size != current_font_size
            || state.engine.settings.appearance.font_family != current_font_family
        {
            self.renderer.update_font(
                &self.device, &self.queue,
                effective_font_size,
                &state.engine.settings.appearance.font_family,
                &state.engine.settings.appearance.custom_font_path,
            );
            self.renderer.resize(&self.queue, self.size.width, self.size.height);
        }
    }

    /// Apply the theme to the egui context.
    pub(super) fn apply_theme(ctx: &egui::Context, _theme: &str, ui_scale: f32) {
        crate::theme::theme().apply_to_egui(ctx, ui_scale);
    }

    /// Re-apply the theme from settings. Called after settings are saved.
    pub fn refresh_theme(&self, theme: &str, ui_scale: f32) {
        Self::apply_theme(&self.egui_ctx, theme, ui_scale);
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
