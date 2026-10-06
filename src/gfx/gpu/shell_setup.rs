use winit::window::Window;

use crate::i18n::t;
use tasty_ui_widgets::{ShellSetupCheck, ShellSetupView, shell_setup_screen};

use super::{GpuState, ShellSetupAction};

/// 경로를 판정한다. 파일이 있고 이름에 bash 또는 zsh가 들어 있어야 쓸 수 있는 셸이다.
fn shell_check(path: &str) -> ShellSetupCheck {
    if path.is_empty() {
        return ShellSetupCheck::Empty;
    }
    let path_obj = std::path::Path::new(path);
    if !path_obj.exists() {
        return ShellSetupCheck::Missing;
    }
    let file_name = path_obj
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if file_name.contains("bash") || file_name.contains("zsh") {
        ShellSetupCheck::Valid
    } else {
        ShellSetupCheck::NotShell
    }
}

impl GpuState {
    /// 첫 실행 셸 설정 화면을 그린다. 터미널 없이 egui만 사용한다.
    pub fn render_shell_setup(
        &mut self,
        window: &Window,
        shell_path: &mut String,
    ) -> Result<ShellSetupAction, wgpu::SurfaceError> {
        let th = crate::theme::theme();
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let raw_input = self.egui_state.take_egui_input(window);
        let mut action = ShellSetupAction::None;

        let title = t("boot.shell_setup.title");
        let subtitle = t("boot.shell_setup.subtitle");
        // 이 화면은 설정 셸이 무효이고 bash 자동 탐지가 실패했을 때 뜬다. Git Bash 안내는 Windows에만 해당한다.
        let git_bash_notice = cfg!(windows).then(|| t("boot.shell_setup.git_bash_missing"));
        let missing = t("boot.shell_setup.check_missing");
        let not_shell = t("boot.shell_setup.check_not_shell");
        let valid = t("boot.shell_setup.check_valid");
        let quit = t("button.quit");
        let confirm = t("settings.terminal.shell_confirm");
        let placeholder = if cfg!(windows) {
            "C:/Program Files/Git/bin/bash.exe"
        } else {
            "/bin/zsh"
        };

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            tasty_egui_theme::apply_theme_to_egui(&th, ctx);
            let setup = ShellSetupView {
                title,
                subtitle,
                git_bash_notice,
                placeholder,
                missing,
                not_shell,
                valid,
                quit,
                confirm,
                check: shell_check(shell_path),
            };
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(th.bg_app().into()))
                .show(ctx, |ui| {
                    let out = shell_setup_screen(ui, &th, &setup, shell_path);
                    if out.confirm {
                        action = ShellSetupAction::Confirmed;
                    } else if out.quit {
                        action = ShellSetupAction::Exit;
                    }
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
            // 부팅 화면과 같이 화면 채움인 bg-app을 GPU clear 색으로도 쓴다.
            let gpu_bg = th.bg_app().to_gpu_rgba();
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shell_setup_pass"),
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

        Ok(action)
    }
}
