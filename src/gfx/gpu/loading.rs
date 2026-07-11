//! 부팅 로딩 프레임 — 부팅 상태 머신(`BootPhase`) 동안 매 프레임 present 하는
//! 최소 화면. 이 단계는 **배경 단색 clear 만** 그린다 (콘텐츠·스피너·문구는 3부
//! 로딩 화면 UI 가 얹는다). 구조는 `shell_setup.rs` 의 pre-app egui 프레임 선례를
//! 따르되, 배경은 raw 값이 아니라 theme 의 앱 배경 토큰(`bg_app`)에서 유도한다.

use winit::window::Window;

use super::GpuState;
use crate::app::boot_machine::BootPhase;

impl GpuState {
    /// 로딩 프레임 1장 렌더: `get_current_texture` → 빈 egui 프레임(CentralPanel,
    /// 콘텐츠 없음) → theme 배경 clear → present.
    ///
    /// `phase` 는 현재 미사용 — 3부(로딩 화면 UI)가 phase 별 문구·스피너를 얹을 수
    /// 있도록 시그니처만 미리 확보한다.
    pub fn render_loading(
        &mut self,
        window: &Window,
        _phase: &BootPhase,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let th = crate::theme::theme();
        let bg = th.bg_app();

        // 빈 egui 프레임 — 배경 fill 만 있는 CentralPanel. 3부가 이 closure 안에
        // 로딩 콘텐츠를 추가한다.
        let raw_input = self.egui_state.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            tasty_egui_theme::apply_theme_to_egui(&th, ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(bg.into()))
                .show(ctx, |_| {});
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
                    label: Some("boot_loading_update"),
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
                label: Some("boot_loading_encoder"),
            });
        {
            // 배경 clear — theme 앱 배경 토큰에서 유도 (shell_setup 의 raw 값
            // 선례를 따르지 않는다 — theme 규칙).
            let gpu_bg = bg.to_gpu_rgba();
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("boot_loading_pass"),
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
