use winit::window::Window;

// ── 디자인 스케일 밖 폰트 크기 ──────────────────────────────────────────────
//
// `Theme` 의 UI 폰트 스케일(micro 10 · caption 11 · body 13 · max 14)에도, DTCG
// primitive(10·11·12·13·14·16·17·20)에도 없는 값들이다. 토큰으로 스냅하면 픽셀이
// 바뀌므로 조용히 반올림하지 않고 이름만 붙인다(스냅 여부는 디자인 판단 항목).
// 토큰이 아니라 `ui_scale` 줌을 타지 않는 것도 현행 유지다. 그 대가와 재검토
// 조건은 `docs/adr/0126-off-scale-font-values-are-not-snapped-to-tokens.md` 에
// 있다 — 위 문단은 원인이고, 근거·대안·철회 조건은 그 ADR 이 든다.

/// 첫 실행 셸 설정 카드의 "Tasty" 브랜드 타이틀. 스케일 밖(30) — primitive 최댓값
/// 20 보다도 크고, brand-wordmark semantic 은 17 이다.
const SETUP_BRAND_TITLE_SIZE: LogicalPx = LogicalPx(30.0);
/// "셸을 찾을 수 없음" 경고 본문. 스케일 밖(12.5).
const SETUP_WARNING_SIZE: LogicalPx = LogicalPx(12.5);
/// 입력 라벨. DTCG primitive `font-size-12` 는 있으나 semantic role 이 없어
/// `Theme` 필드가 없다 — ADR-0126 대로 **이름에 primitive 임을 남긴다**. 호출 자리에서
/// "토큰인가 미배정 primitive 인가" 가 이름만으로 갈리도록 하는 것이 규칙의 목적이다.
const SETUP_INPUT_LABEL_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

use crate::i18n::t;
use tasty_ui_widgets::{hspace, margin_all, margin_sym, vspace};

use super::{GpuState, ShellSetupAction};
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;

/// 검증 라벨(font-size-11)이 없을 때(valid/미입력) 그 line-box 높이를 예약하는 구조
/// 상수 — spacing 리듬이 아니라 라벨 높이 미러라 토큰 대신 명명 const 로 둔다. 폰트
/// line-box 높이는 font metric 에서 나오는 값이라 4px 그리드에 맞을 이유가 없다(간격
/// 리듬 값이 아니므로 그리드 규칙 적용 대상도 아님) — 예약 안 하면 라벨이 나타나고
/// 사라질 때마다 그 아래 레이아웃이 흔들린다(layout jump 방지).
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

            // Apply theme from theme module
            let th = crate::theme::theme();
            tasty_egui_theme::apply_theme_to_egui(&th, ctx);

            // Local aliases for this function
            let bg_panel = th.bg_app();
            let bg_card = th.bg_sidebar();
            let border = th.border_default();
            let text_dim = th.text_muted();
            let amber = th.accent_warning();
            let red_err = th.accent_danger();
            let accent_ok = th.accent_success();
            // 비활성 버튼 채움 — 값-동일 surface_hover()(=surface1). role 은 disabled-accent 이나 전용 토큰 부재.
            let accent_dis = th.surface_hover();

            // Dark background panel
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(bg_panel.into()))
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
                        .fill(bg_card.into())
                        .stroke(egui::Stroke::new(th.border_width.value(), border))
                        .corner_radius(tasty_ui_widgets::tokens::BOOT_CARD_CORNER_RADIUS)
                        .inner_margin(margin_all(th.spacing_xl))
                        .shadow(th.shadow_popover().to_egui()),
                )
                .show(ctx, |ui| {
                    // ── Title ──────────────────────────────────────
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Tasty")
                                .size(SETUP_BRAND_TITLE_SIZE.value())
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

                    // ── Warning ────────────────────────────────────
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
                                        .size(SETUP_WARNING_SIZE.value())
                                        .color(amber),
                                )
                                .wrap(),
                            );
                        });

                    vspace(ui, th.spacing_lg);

                    // ── Input ──────────────────────────────────────
                    ui.label(
                        egui::RichText::new(t("settings.terminal.shell_label"))
                            .size(SETUP_INPUT_LABEL_PRIMITIVE_12.value())
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

                    // ── Error / success hint ──────────────────────
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

                    // ── Buttons ────────────────────────────────────
                    ui.vertical_centered(|ui| {
                        ui.horizontal(|ui| {
                            let btn_size = egui::vec2(110.0, 34.0);

                            // Cancel
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

                            // 디자인 버튼 간격 10px 은 off-grid — 4px 그리드의 가장 가까운
                            // 값인 spacing_md(12)로 snap.
                            hspace(ui, th.spacing_md);

                            // OK
                            let (ok_fill, ok_stroke, ok_text) = if is_valid {
                                (
                                    th.accent_success(),
                                    egui::Stroke::new(th.border_width.value(), th.accent_success()),
                                    // accent 위 텍스트 — 값-동일 bg_panel()(=base). text_on_accent()=crust 와 값 달라 값-보존 유지.
                                    th.bg_panel(),
                                )
                            } else {
                                // divergence: 비활성 보더에 surface2 — border-role 전용 토큰 부재, 값-동일 surface_active().
                                // overlay0 dim 텍스트 — 값-동일 text_placeholder()(=placeholder=overlay0 값).
                                (
                                    accent_dis,
                                    egui::Stroke::new(th.border_width.value(), th.surface_active()),
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
