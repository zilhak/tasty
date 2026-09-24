//! 엔진 부팅 실패 진단을 창에 표시한다. GPU나 창 생성 자체가 실패한 경우에는 사용할 수 없다.

use winit::window::Window;

use super::{BootErrorInfo, GpuState};
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::vspace;

impl GpuState {
    /// 부팅 실패 진단을 그린다. 반환 `Ok(true)` = 사용자가 종료를 눌렀다(caller 가
    /// `exit` 한다). 터미널 없이 egui 만 그린다 — `render_shell_setup` 과 같은 구조다.
    pub fn render_boot_error(
        &mut self,
        window: &Window,
        info: &BootErrorInfo,
    ) -> Result<bool, wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let raw_input = self.egui_state.take_egui_input(window);
        let mut quit = false;

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            let th = crate::theme::theme();
            tasty_egui_theme::apply_theme_to_egui(&th, ctx);

            let bg_panel = th.bg_app();
            let text_dim = th.text_muted();
            let danger = th.accent_danger();

            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(bg_panel.into()))
                .show(ctx, |_| {});

            let content_w = 460.0;
            egui::Window::new("boot_error")
                .title_bar(false)
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .fixed_size(egui::vec2(content_w, 0.0))
                .frame(
                    egui::Frame::new()
                        .fill(th.bg_sidebar().into())
                        .stroke(egui::Stroke::new(
                            th.border_width.value(),
                            th.border_default(),
                        ))
                        .corner_radius(th.corner_radius_lg.value())
                        .inner_margin(tasty_ui_widgets::margin_all(th.spacing_lg))
                        // 화면 중앙의 진단 카드에는 모달 그림자를 사용한다.
                        .shadow(th.shadow_modal().to_egui()),
                )
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(&info.title)
                            .size(th.font_size_heading.value())
                            .strong()
                            .color(danger),
                    );
                    vspace(ui, STRUCT_GAP_2);

                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&info.body)
                                .size(th.font_size_body.value())
                                .color(th.text_primary()),
                        )
                        .wrap(),
                    );
                    vspace(ui, th.spacing_md);

                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&info.hint)
                                .size(th.font_size_caption.value())
                                .color(text_dim),
                        )
                        .wrap(),
                    );

                    vspace(ui, th.spacing_lg);

                    ui.vertical_centered(|ui| {
                        let btn_size = egui::vec2(120.0, 34.0);
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new(crate::i18n::t("button.quit"))
                                        .size(th.font_size_body.value())
                                        .strong()
                                        .color(th.bg_panel()),
                                )
                                .min_size(btn_size)
                                .fill(danger)
                                // 채움과 같은 색의 stroke가 버튼 면적을 넓힌다.
                                // border_width가 바뀌면 테두리뿐 아니라 이 버튼 크기도 함께 확인해야 한다.
                                .stroke(egui::Stroke::new(th.border_width.value(), danger))
                                .corner_radius(tasty_ui_widgets::tokens::BOOT_CHROME_CORNER_RADIUS),
                            )
                            .clicked()
                            || ui.input(|i| {
                                i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)
                            })
                        {
                            quit = true;
                        }
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
                    label: Some("boot_error_update"),
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
                label: Some("boot_error_encoder"),
            });
        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("boot_error_pass"),
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

        Ok(quit)
    }
}
