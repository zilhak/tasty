use winit::window::Window;

use crate::i18n::t;

use super::{GpuState, ShellSetupAction};

impl GpuState {
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
}
