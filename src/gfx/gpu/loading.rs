//! 로딩 프레임 — 부팅 상태 머신(`BootPhase`)과 종료 상태 머신(`ShutdownPhase`)이
//! 대기 동안 매 프레임 present 하는 화면. 워드마크 + 스피너 + phase 문구 중앙
//! 스택을 그린다. 구조는 `shell_setup.rs` 의 pre-app egui 프레임 선례를 따르되,
//! 배경은 raw 값이 아니라 theme 의 앱 배경 토큰(`bg_app`)에서 유도한다.
//!
//! **부팅과 종료가 같은 락업을 쓴다.** 다른 것은 phase 문구 하나뿐이라
//! [`GpuState::render_loading`] 은 phase 타입이 아니라 **i18n 키**를 받는다 — 두
//! 상태 머신이 각자의 phase → 키 매핑을 소유하고, 렌더 스택은 한 벌로 남는다.
//! 근거는 [`docs/adr/0077-shutdown-loading-screen.md`].

use winit::window::Window;

use super::GpuState;
use crate::app::boot_machine::BootPhase;
// 워드마크 락업 렌더는 위젯 크레이트가 단일 출처다(갤러리 specimen 과 공유).
// 락업 치수는 `Theme` 의 `loading_screen_*` 에서 온다 — 여기 리터럴로 적으면 같은
// 스택의 간격·문구만 배율을 타서 어긋난다(ADR-0135).

/// 부팅 phase → i18n 문구 키. `WaitingEngine`(S-7 추가) 은 별도 확정 문구가 없어
/// 선행 단계 `GpuInit` 과 같은 문구로 묶는다(디자인 확정값 부재 시 가장 가까운
/// 단계로 근사 — S-17 검증 정정 권고 #2).
pub fn boot_phase_text_key(phase: &BootPhase) -> &'static str {
    match phase {
        BootPhase::GpuInit | BootPhase::WaitingEngine { .. } => "boot.phase_gpu_init",
        BootPhase::WaitingPlugins { .. } => "boot.phase_waiting_plugins",
        BootPhase::RestoringLayout { .. } => "boot.phase_restoring_layout",
    }
}

impl GpuState {
    /// 로딩 프레임 1장 렌더: `get_current_texture` → egui 프레임(워드마크·스피너·
    /// phase 문구 중앙 스택) → theme 배경 clear → present.
    ///
    /// `phase_text_key` 는 문구 슬롯에 넣을 i18n 키다 — 부팅은
    /// [`boot_phase_text_key`], 종료는 `ShutdownPhase::text_key` 가 만든다.
    pub fn render_loading(
        &mut self,
        window: &Window,
        phase_text_key: &str,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let th = crate::theme::theme();
        let bg = th.bg_app();

        // egui 프레임 — 워드마크 → 스피너 → phase 문구 중앙 스택.
        let raw_input = self.egui_state.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            tasty_egui_theme::apply_theme_to_egui(&th, ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(bg.into()))
                .show(ctx, |ui| {
                    let content_height = th.loading_screen_wordmark_icon_size().value()
                        + th.spacing_xl.value()
                        + th.loading_screen_spinner_size().value()
                        + th.spacing_lg.value()
                        + th.loading_screen_phase_slot_height().value();
                    let top_pad = ((ui.available_height() - content_height) / 2.0).max(0.0);
                    ui.add_space(top_pad);
                    ui.vertical_centered(|ui| {
                        crate::adapters::ui::brand::draw_wordmark(
                            ui,
                            &th,
                            th.loading_screen_wordmark_icon_size(),
                            th.loading_screen_wordmark_font_size(),
                        );
                        ui.add_space(th.spacing_xl.value());
                        tasty_ui_widgets::Spinner::new()
                            .size(th.loading_screen_spinner_size().value())
                            .color(th.accent_primary().to_egui())
                            .show(ui, &th);
                        ui.add_space(th.spacing_lg.value());
                        let (slot_rect, _) = ui.allocate_exact_size(
                            egui::vec2(
                                ui.available_width(),
                                th.loading_screen_phase_slot_height().value(),
                            ),
                            egui::Sense::hover(),
                        );
                        ui.painter().text(
                            slot_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            crate::i18n::t(phase_text_key),
                            egui::FontId::proportional(th.font_size_body.value()),
                            th.text_muted().to_egui(),
                        );
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
                    label: Some("loading_update"),
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
                label: Some("loading_encoder"),
            });
        {
            // 배경 clear — theme 앱 배경 토큰에서 유도 (shell_setup 의 raw 값
            // 선례를 따르지 않는다 — theme 규칙).
            let gpu_bg = bg.to_gpu_rgba();
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("loading_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: gpu_bg.r() as f64,
                            g: gpu_bg.g() as f64,
                            b: gpu_bg.b() as f64,
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

        Ok(())
    }
}
