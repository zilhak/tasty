use crate::model::PhysicalRect;
use crate::renderer::RenderPreedit;
use crate::state::AppState;

use super::GpuState;

impl GpuState {
    pub(super) fn render_clear_pass(
        &self,
        view: &wgpu::TextureView,
        _state: &AppState,
        engine: &crate::engine_state::CoreState,
    ) {
        let bg_alpha = engine.settings.appearance.background_opacity as f64;
        let th = crate::theme::theme();
        let bg = th.base.to_gpu_rgba();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("clear_pass"),
            });
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg.r() as f64,
                            g: bg.g() as f64,
                            b: bg.b() as f64,
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
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    pub(super) fn render_terminals(
        &mut self,
        view: &wgpu::TextureView,
        regions: &[(u32, PhysicalRect, Vec<crate::model::SurfaceRegion<'_>>)],
        focused_surface_id: Option<u32>,
        selection: Option<&crate::selection::TextSelection>,
        _settings: &crate::settings::AppearanceSettings,
        preedit: Option<&super::ImePreeditState>,
        link_hover: Option<(u32, &crate::terminal_link::LinkHighlight)>,
        search: Option<&crate::search_state::SearchState>,
    ) {
        let theme = crate::theme::theme();
        let term_surface = theme.surface("terminal");

        // Each terminal needs its own encoder + submit cycle because
        // prepare_terminal_viewport writes to shared GPU buffers via queue.write_buffer().
        // write_buffer writes are applied at submit time, so batching multiple terminals
        // into one encoder would cause only the last terminal's data to be visible.
        for (_pane_id, _pane_rect, surface_regions) in regions {
            for region in surface_regions {
                let Some(terminal) = region.surface.focused_terminal() else {
                    continue;
                };
                let surface_id = &region.id;
                let rect = &region.rect;
                let is_focused = focused_surface_id == Some(*surface_id);
                let bg = if is_focused {
                    term_surface.focused_bg.to_gpu_rgba()
                } else {
                    term_surface.unfocused_bg.to_gpu_rgba()
                };
                let fg = if is_focused {
                    term_surface.focused_fg.to_gpu_rgba()
                } else {
                    term_surface.unfocused_fg.to_gpu_rgba()
                };

                // Build selection info for this surface
                let sel_info = selection
                    .filter(|s| s.surface_id == *surface_id && !s.is_empty())
                    .map(|s| (s.normalized(), theme.selection_bg.to_gpu_rgba()));
                let sel_ref = sel_info.as_ref();

                let render_preedit = preedit
                    .filter(|ime| ime.surface_id == *surface_id && !ime.text.is_empty())
                    .map(|ime| RenderPreedit {
                        text: ime.text.clone(),
                        anchor_col: ime.anchor_col,
                        anchor_row: ime.anchor_row,
                        bg_color: theme.blue.to_gpu_rgba(),
                        fg_color: theme.base.to_gpu_rgba(),
                    });
                let render_preedit_ref = render_preedit.as_ref();

                let link_for_this = link_hover
                    .filter(|(sid, _)| sid == surface_id)
                    .map(|(_, h)| h);

                // Build search match highlights for this surface.
                let search_highlights = search
                    .filter(|s| s.surface_id == *surface_id && !s.matches.is_empty())
                    .map(|s| crate::renderer::SearchHighlights {
                        matches: &s.matches,
                        active_index: s.current_index,
                        inactive_bg: theme.search_match_bg.to_gpu_rgba(),
                        active_bg: theme.search_match_active_bg.to_gpu_rgba(),
                    });
                let search_ref = search_highlights.as_ref();

                self.renderer.prepare_terminal_viewport(
                    terminal,
                    &self.queue,
                    rect,
                    self.size.width,
                    self.size.height,
                    bg,
                    fg,
                    is_focused,
                    sel_ref,
                    render_preedit_ref,
                    link_for_this,
                    search_ref,
                );

                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("terminal_pass"),
                        });
                {
                    let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
                    self.renderer.render_scissored(
                        &mut render_pass,
                        rect,
                        self.size.width,
                        self.size.height,
                    );
                }
                self.queue.submit(std::iter::once(encoder.finish()));
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
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let mut egui_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("egui_encoder"),
                });

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut egui_encoder,
            paint_jobs,
            screen_descriptor,
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
            self.egui_renderer
                .render(&mut render_pass, paint_jobs, screen_descriptor);
        }

        self.queue.submit(std::iter::once(egui_encoder.finish()));

        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }
    }
}
