use crate::model::PhysicalRect;
use crate::renderer::RenderPreedit;
use crate::state::AppState;

use super::GpuState;

impl GpuState {
    pub(super) fn render_clear_pass(
        &self,
        view: &wgpu::TextureView,
        _state: &AppState,
        engine: &crate::core::CoreState,
    ) {
        let bg_alpha = engine.settings.appearance.background_opacity as f64;
        let th = crate::theme::theme();
        let bg = th.bg_panel().to_gpu_rgba();

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

    #[allow(clippy::too_many_arguments)] // reason: 터미널 렌더 컨텍스트 전체
    pub(super) fn render_terminals(
        &mut self,
        view: &wgpu::TextureView,
        regions: &[(u32, PhysicalRect, Vec<crate::model::SurfaceRegion<'_>>)],
        engine: &crate::core::CoreState,
        focused_surface_id: Option<u32>,
        selection: Option<&crate::selection::TextSelection>,
        vi_cursor: Option<(u32, crate::selection::SelectionPoint)>,
        _settings: &crate::settings::AppearanceSettings,
        preedit: Option<&super::ImePreeditState>,
        link_hover: Option<(u32, &crate::terminal_link::LinkHighlight)>,
        search: Option<&crate::search_state::SearchState>,
    ) {
        let theme = crate::theme::theme();
        let term_surface = theme.surface("terminal");
        // ANSI 16 팔레트는 *프레임당 1회* 만 추출 — 셀별 lock 비용 제거.
        let ansi = theme.ansi_palette();
        // DECSCNM 렌더 허용 여부 — 프레임당 1회 읽어 모든 surface 에 동일 적용.
        let reverse_screen_enabled = engine.settings.general.reverse_screen_enabled;

        // Accumulate instance data for every surface into the renderer's
        // shared vecs, recording per-surface (rect, bg range, glyph range).
        self.renderer.begin_frame();

        for (_pane_id, _pane_rect, surface_regions) in regions {
            for region in surface_regions {
                // attach/detach 작업 J (decisions 정정): 점유된 surface 는 서버측에서
                // **readonly 뷰**로 보인다 — 숨김이 아니라 내용 보임, 조작만 차단.
                // live grid 대신 3초 cadence 로 갱신되는 display-only mirror
                // (`readonly_view`)를 렌더한다(plan §2.3). 입력 차단은
                // `apply_send_to_surface` 가 담당하고, 점유 표시(주황 테두리)는 egui
                // 오버레이가 그린다. client mirror 는 자기 engine 에 lock 이 없어
                // (is_hard_occupied=false) live terminal 을 정상 렌더한다(G).
                let is_readonly = engine.attach.is_hard_occupied(region.id);
                // 첫 AttachPoll tick 전이면 mirror 가 아직 없다 — 다음 tick 에 채워진다.
                let Some(terminal) = engine.visible_terminal(region.id) else {
                    continue;
                };
                let surface_id = &region.id;
                let rect = &region.rect;
                // readonly 뷰는 사용자가 조작할 수 없으므로 포커스 커서/선택/IME/링크/
                // 검색 오버레이를 그리지 않는다(보기 전용).
                let is_focused = !is_readonly && focused_surface_id == Some(*surface_id);
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

                // readonly 뷰는 IME/vi-cursor/링크/검색처럼 *PTY 앱과의 상호작용*
                // 오버레이는 그리지 않는다(보기 전용). selection 만은 예외 — PTY 로
                // 아무것도 보내지 않는 tasty 로컬 UI 동작(드래그 선택→복사)이라
                // hard 점유(readonly)에서도 계속 표시한다(ADR-0049).
                let sel_info = selection
                    .filter(|s| s.surface_id == *surface_id && !s.is_empty())
                    .map(|s| (s.normalized(), theme.selection_bg.to_gpu_rgba()));
                let sel_ref = sel_info.as_ref();

                let vi_cursor_info = vi_cursor
                    .filter(|_| !is_readonly)
                    .filter(|(sid, _)| sid == surface_id)
                    .map(|(_, pt)| (pt, theme.vi_cursor_bg.to_gpu_rgba()));
                let vi_cursor_ref = vi_cursor_info.as_ref();

                let render_preedit = preedit
                    .filter(|_| !is_readonly)
                    .filter(|ime| ime.surface_id == *surface_id && !ime.text.is_empty())
                    .map(|ime| RenderPreedit {
                        text: ime.text.clone(),
                        anchor_col: ime.anchor_col,
                        anchor_row: ime.anchor_row,
                        bg_color: theme.accent_primary().to_gpu_rgba(),
                        // accent 위 preedit 텍스트 — 값-동일 bg_panel()(=base). text_on_accent()=crust 와 값 달라 값-보존 유지.
                        fg_color: theme.bg_panel().to_gpu_rgba(),
                    });
                let render_preedit_ref = render_preedit.as_ref();

                let link_for_this = link_hover
                    .filter(|_| !is_readonly)
                    .filter(|(sid, _)| sid == surface_id)
                    .map(|(_, h)| h);

                let search_highlights = search
                    .filter(|_| !is_readonly)
                    .filter(|s| s.surface_id == *surface_id && !s.matches.is_empty())
                    .map(|s| crate::renderer::SearchHighlights {
                        matches: &s.matches,
                        active_index: s.current_index,
                        inactive_bg: theme.search_match_bg.to_gpu_rgba(),
                        active_bg: theme.search_match_active_bg.to_gpu_rgba(),
                    });
                let search_ref = search_highlights.as_ref();

                self.renderer.append_terminal_viewport(
                    terminal,
                    &self.queue,
                    rect,
                    &ansi,
                    bg,
                    fg,
                    is_focused,
                    sel_ref,
                    vi_cursor_ref,
                    render_preedit_ref,
                    link_for_this,
                    search_ref,
                    reverse_screen_enabled,
                );
            }
        }

        // Single buffer upload (auto-grows if needed) + single encoder/submit.
        self.renderer.flush_buffers(&self.device, &self.queue);

        let mut encoder = self
            .device
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
            self.renderer
                .render_all(&mut render_pass, self.size.width, self.size.height);
        }
        self.queue.submit(std::iter::once(encoder.finish()));
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
