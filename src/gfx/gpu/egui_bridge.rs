use winit::window::Window;

use crate::adapters::ui;
use crate::model::PhysicalRect;
use crate::settings::EffectiveFont;
use crate::state::AppState;

use super::GpuState;

/// Build a single-line signature capturing every `EffectiveFont` field that
/// the GPU cell renderer cares about, plus the scale-resolved size. Used to
/// detect when `update_font` must be re-run.
///
/// Comparing the raw user-facing settings (not the resolved family name) is
/// intentional — `cosmic_text` normalizes `""` / `"monospace"` into
/// `FamilyOwned::Name("D2Coding ligature")`, so reading back the post-resolve
/// family from the renderer and comparing it against the raw settings would
/// always mismatch, causing a wasted atlas reset every frame.
pub(super) fn term_font_signature(font: &EffectiveFont, effective_size: f32) -> String {
    format!(
        "{}|{}|{}|{}",
        effective_size, font.font_family, font.custom_font_path, font.line_height
    )
}

impl GpuState {
    #[allow(clippy::too_many_arguments)] // reason: frame context 전체 전달
    pub(super) fn run_egui_frame(
        &mut self,
        state: &mut AppState,
        engine: &mut crate::core::CoreState,
        window: &Window,
        pane_rects: &[(u32, PhysicalRect)],
        dividers: &[PhysicalRect],
        terminal_rect: PhysicalRect,
        plugin_manager: Option<&crate::plugin::PluginManager>,
    ) -> egui::FullOutput {
        let raw_input = self.egui_state.take_egui_input(window);
        let scale_factor = self.scale_factor;
        let canvas_cache = &self.canvas_textures;
        let proxy = &self.proxy;

        self.egui_ctx.run(raw_input, |ctx| {
            // CSD 공통 titlebar — TopBottomPanel::top 이 먼저 등록되어야 사이드바
            // SidePanel 이 그 아래에서 시작한다. 드래그/더블클릭을 winit window 로 브리지.
            ui::titlebar::draw_titlebar(ctx, window, proxy);
            ui::draw_ui(ctx, state, engine, scale_factor);
            ui::draw_pane_dividers(ctx, dividers, scale_factor);
            ui::draw_surface_highlights(ctx, state, engine, terminal_rect, scale_factor);
            ui::draw_pane_tab_bars(ctx, state, engine, pane_rects, scale_factor);
            ui::draw_egui_panels(ctx, state, engine, pane_rects, scale_factor, canvas_cache);
            ui::draw_status_bar(ctx, state, engine, terminal_rect, scale_factor);
            // Context menus are now handled via native OS menus (see process_pending_native_menu)
            ui::draw_popups(ctx, state, engine, pane_rects, terminal_rect, scale_factor);
            // Plugin popup 인스턴스(동적 instance_id) — host PopupManager와 별도 경로.
            crate::plugin_bridge::popup_render::draw_plugin_popups(
                ctx,
                state,
                engine,
                plugin_manager,
                canvas_cache,
            );
            // 외부 drag&drop hover 시각 피드백 — 모든 레이어 위에 그린다.
            ui::drop_overlay::draw_drop_overlay(ctx, state, engine, terminal_rect, scale_factor);

            // Settings UI is now rendered in the modal window (ModalView)
        })
    }

    pub(super) fn post_egui_update(&mut self, engine: &crate::core::CoreState, _prev_theme: &str) {
        // Theme / ui_zoom 변경 감지는 더 이상 polling 으로 하지 않는다 —
        // settings cascade 와 단축키 (Z-7) 가 `UiIntent::AppearanceChanged` 를
        // 발화하면 `App::cascade_appearance_changed` 가 broadcast 로 갱신한다.
        // 본 함수는 매 프레임 egui style reapply 만 담당 (cheap; visuals/style
        // state 가 egui ctx 안에 있어 다른 set_style 호출에 덮어쓰일 가능성을
        // 대비한 idempotent re-application).
        tasty_egui_theme::apply_theme_to_egui(&crate::theme::theme(), &self.egui_ctx);

        // Surface font refresh is done in render() before run_egui_frame().

        let term_font = engine.settings.appearance.effective_terminal_font();
        let effective_font_size = term_font.effective_font_size(self.scale_factor);
        let new_sig = term_font_signature(&term_font, effective_font_size);
        if new_sig != self.last_term_font_sig {
            self.renderer.update_font(
                &self.device,
                &self.queue,
                effective_font_size,
                &term_font.font_family,
                &term_font.custom_font_path,
                term_font.line_height,
            );
            self.renderer
                .resize(&self.queue, self.size.width, self.size.height);
            self.last_term_font_sig = new_sig;
        }
    }

    /// Apply the theme to the egui context.
    pub(super) fn apply_theme(ctx: &egui::Context, _theme: &str) {
        tasty_egui_theme::apply_theme_to_egui(&crate::theme::theme(), ctx);
    }

    /// Re-apply the current global `Theme` to this window's egui context.
    /// `cascade_appearance_changed` broadcast 의 진입점 — main + modal 모두 같은
    /// 시그니처로 호출한다.
    pub fn refresh_theme(&self) {
        tasty_egui_theme::apply_theme_to_egui(&crate::theme::theme(), &self.egui_ctx);
    }

    // ── Generic egui helpers (for modal windows) ──

    /// Take egui input from a window.
    pub fn take_egui_input(&mut self, window: &Window) -> egui::RawInput {
        self.egui_state.take_egui_input(window)
    }

    /// Run an egui frame with a custom UI closure.
    pub fn run_egui(
        &self,
        raw_input: egui::RawInput,
        ui_fn: impl FnMut(&egui::Context),
    ) -> egui::FullOutput {
        self.egui_ctx.run(raw_input, ui_fn)
    }

    /// Finish an egui frame: tessellate, render, present.
    pub fn finish_egui_frame(&mut self, window: &Window, full_output: egui::FullOutput) {
        // egui-winit disables IME when no egui text field is focused
        // (calls set_ime_allowed(false) when self.allow_ime differs from
        // ime.is_some()). The terminal always needs IME active.
        // Pre-set allow_ime=false so that when egui computes allow_ime=false
        // (no text field), the check false!=false is false and it skips
        // the set_ime_allowed(false) call entirely.
        self.egui_state.set_allow_ime(false);
        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
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
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Clear
        let th = crate::theme::theme();
        let bg = th.base.to_float();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("modal_clear"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("modal_clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: 1.0,
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

        // Egui render
        for (id, image_delta) in &full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }
        let mut egui_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("modal_egui"),
                });
        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut egui_encoder,
            &paint_jobs,
            &screen_descriptor,
        );
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
            self.egui_renderer
                .render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }
        self.queue.submit(std::iter::once(egui_encoder.finish()));

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        output.present();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_font() -> EffectiveFont {
        EffectiveFont {
            font_family: String::new(),
            font_size: 14.0,
            custom_font_path: String::new(),
            line_height: 1.0,
            font_scale_mode: "auto".to_string(),
        }
    }

    #[test]
    fn signature_stable_for_identical_input() {
        let f = base_font();
        assert_eq!(term_font_signature(&f, 14.0), term_font_signature(&f, 14.0));
    }

    #[test]
    fn signature_differs_when_family_changes() {
        let a = base_font();
        let mut b = base_font();
        b.font_family = "Hack".into();
        assert_ne!(term_font_signature(&a, 14.0), term_font_signature(&b, 14.0));
    }

    #[test]
    fn signature_differs_when_custom_font_path_changes() {
        let a = base_font();
        let mut b = base_font();
        b.custom_font_path = "/tmp/x.ttf".into();
        assert_ne!(term_font_signature(&a, 14.0), term_font_signature(&b, 14.0));
    }

    #[test]
    fn signature_differs_when_line_height_changes() {
        let a = base_font();
        let mut b = base_font();
        b.line_height = 1.25;
        assert_ne!(term_font_signature(&a, 14.0), term_font_signature(&b, 14.0));
    }

    #[test]
    fn signature_differs_when_effective_size_changes() {
        let f = base_font();
        // scale_factor 변화(HiDPI 모니터 전환) 또는 font_scale_mode 토글의 결과.
        assert_ne!(term_font_signature(&f, 14.0), term_font_signature(&f, 28.0));
    }

    #[test]
    fn signature_collapses_empty_and_normalized_family() {
        // 정규화 mismatch(`""` ↔ `"D2Coding ligature"`) 가 신호 안 되도록,
        // signature 는 사용자가 입력한 raw family 만 본다.
        let f = base_font();
        let sig_empty = term_font_signature(&f, 14.0);
        // 같은 `""` 가 두 번 들어왔을 때 같은 signature 이어야 한다 — 매 frame
        // update_font 호출되던 G2 회귀의 회귀 방지.
        assert_eq!(sig_empty, term_font_signature(&f, 14.0));
    }
}
