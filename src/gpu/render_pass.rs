use crate::model::Rect;
use crate::renderer::RenderPreedit;
use crate::state::AppState;

use super::GpuState;

impl GpuState {
    pub(super) fn render_clear_pass(&self, view: &wgpu::TextureView, state: &AppState) {
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

    pub(super) fn render_terminals(
        &mut self,
        view: &wgpu::TextureView,
        regions: &[(u32, Rect, Vec<(u32, &tasty_terminal::Terminal, Rect)>)],
        focused_surface_id: Option<u32>,
        selection: Option<&crate::selection::TextSelection>,
        settings: &crate::settings::AppearanceSettings,
        preedit: Option<&super::ImePreeditState>,
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

                let render_preedit = preedit
                    .filter(|ime| ime.surface_id == *surface_id && !ime.text.is_empty())
                    .map(|ime| RenderPreedit {
                        text: ime.text.clone(),
                        cursor: ime.cursor,
                        anchor_col: ime.anchor_col,
                        anchor_row: ime.anchor_row,
                        bg_color: crate::theme::Theme::to_float(theme.blue),
                        fg_color: crate::theme::Theme::to_float(theme.base),
                    });
                let render_preedit_ref = render_preedit.as_ref();

                self.renderer.prepare_terminal_viewport(
                    terminal, &self.queue, rect,
                    self.size.width, self.size.height, bg, is_focused,
                    sel_ref,
                    render_preedit_ref,
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

    pub(super) fn render_egui_pass(
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
}
