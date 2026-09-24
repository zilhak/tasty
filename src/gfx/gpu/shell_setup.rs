use winit::window::Window;

/// 입력·경고용 primitive 글꼴 크기. 대응 semantic role이 없어 별도로 사용한다.
const SETUP_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

use crate::i18n::t;
use tasty_ui_widgets::{hspace, margin_all, margin_sym, vspace};

use super::{GpuState, ShellSetupAction};
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;

/// 검증 문구가 없을 때도 같은 높이를 확보해 입력 아래 레이아웃이 움직이지 않게 한다.
const RESERVE_LABEL_H: LogicalPx = LogicalPx(14.0);

impl GpuState {
    /// Render the shell setup dialog (no terminal, just egui).
    pub fn render_shell_setup(
        &mut self,
        window: &Window,
        shell_path: &mut String,
    ) -> Result<ShellSetupAction, wgpu::SurfaceError> {
        let _th = crate::theme::theme();
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

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

            let th = crate::theme::theme();
            tasty_egui_theme::apply_theme_to_egui(&th, ctx);

            let bg_panel = th.bg_app();
            let bg_card = th.bg_sidebar();
            let border = th.border_default();
            let text_dim = th.text_muted();
            let amber = th.accent_warning();
            let red_err = th.accent_danger();
            let accent_ok = th.accent_success();
            let accent_dis = th.surface_hover();

            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(bg_panel.into()))
                .show(ctx, |_| {});

            let content_w = 440.0;
            egui::Window::new("shell_setup")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .fixed_size(egui::vec2(content_w, 0.0))
                .frame(
                    egui::Frame::new()
                        .fill(bg_card.into())
                        .stroke(egui::Stroke::new(th.border_width.value(), border))
                        .corner_radius(tasty_ui_widgets::tokens::BOOT_CARD_CORNER_RADIUS)
                        .inner_margin(margin_all(th.spacing_xl))
                        // 화면 중앙의 설정 카드에 모달 그림자를 사용한다.
                        .shadow(th.shadow_modal().to_egui()),
                )
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Tasty")
                                .size(th.font_size_brand_display.value())
                                .strong()
                                .color(th.text_primary()),
                        );
                        vspace(ui, STRUCT_GAP_2);
                        ui.label(
                            egui::RichText::new(t("settings.terminal.setup_subtitle"))
                                .size(th.font_size_caption.value())
                                .color(text_dim),
                        );
                    });

                    vspace(ui, th.spacing_lg);
                    ui.separator();
                    vspace(ui, th.spacing_md);

                    egui::Frame::new()
                        .fill(th.surface_raised().into())
                        .stroke(egui::Stroke::new(
                            th.border_width.value(),
                            th.border_strong(),
                        ))
                        .corner_radius(tasty_ui_widgets::tokens::BOOT_CHROME_CORNER_RADIUS)
                        .inner_margin(margin_sym(th.spacing_md, th.spacing_sm))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(t("settings.terminal.shell_not_found"))
                                        .size(SETUP_PRIMITIVE_12.value())
                                        .color(amber),
                                )
                                .wrap(),
                            );
                        });

                    vspace(ui, th.spacing_lg);

                    ui.label(
                        egui::RichText::new(t("settings.terminal.shell_label"))
                            .size(SETUP_PRIMITIVE_12.value())
                            .color(text_dim),
                    );
                    vspace(ui, th.spacing_xs);

                    let response = ui.add_sized(
                        [ui.available_width(), 32.0],
                        egui::TextEdit::singleline(shell_path)
                            .hint_text(tasty_egui_theme::hint_text(
                                &th,
                                "C:/Program Files/Git/bin/bash.exe",
                            ))
                            .font(egui::TextStyle::Monospace),
                    );

                    vspace(ui, th.spacing_xs);
                    if show_error {
                        ui.label(
                            egui::RichText::new(t("settings.terminal.shell_invalid_path"))
                                .size(th.font_size_caption.value())
                                .color(red_err),
                        );
                    } else if is_valid {
                        ui.label(
                            egui::RichText::new(t("settings.terminal.shell_valid"))
                                .size(th.font_size_caption.value())
                                .color(accent_ok),
                        );
                    } else {
                        vspace(ui, RESERVE_LABEL_H); // 라벨 부재 시 높이 예약
                    }

                    vspace(ui, th.spacing_lg);

                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            let btn_size = egui::vec2(110.0, 34.0);

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new(t("button.cancel"))
                                            .size(th.button_font_size().value())
                                            .color(text_dim),
                                    )
                                    .min_size(btn_size)
                                    .fill(th.bg_panel())
                                    .stroke(egui::Stroke::new(th.border_width.value(), border))
                                    .corner_radius(
                                        tasty_ui_widgets::tokens::BOOT_CHROME_CORNER_RADIUS,
                                    ),
                                )
                                .clicked()
                            {
                                action = ShellSetupAction::Exit;
                            }

                            hspace(ui, th.spacing_md);

                            let (ok_fill, ok_stroke, ok_text) = if is_valid {
                                (
                                    th.accent_success(),
                                    egui::Stroke::new(th.border_width.value(), th.accent_success()),
                                    th.bg_panel(),
                                )
                            } else {
                                (
                                    accent_dis,
                                    egui::Stroke::new(th.border_width.value(), th.border_frame()),
                                    th.text_placeholder(),
                                )
                            };

                            let ok_resp = ui.add_enabled(
                                is_valid,
                                egui::Button::new(
                                    egui::RichText::new("OK")
                                        .size(th.button_font_size().value())
                                        .strong()
                                        .color(ok_text),
                                )
                                .min_size(btn_size)
                                .fill(ok_fill)
                                .stroke(ok_stroke)
                                .corner_radius(tasty_ui_widgets::tokens::BOOT_CHROME_CORNER_RADIUS),
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
        let tris = self
            .egui_ctx
            .tessellate(full_output.shapes, self.scale_factor);
        for (id, delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }

        let mut update_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("egui_update"),
                });
        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut update_encoder,
            &tris,
            &screen_descriptor,
        );
        self.queue.submit(std::iter::once(update_encoder.finish()));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shell_setup_encoder"),
            });
        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shell_setup_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.12,
                            g: 0.12,
                            b: 0.14,
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
            self.egui_renderer
                .render(&mut render_pass, &tris, &screen_descriptor);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        Ok(action)
    }
}
