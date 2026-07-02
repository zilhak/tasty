mod egui_bridge;
mod egui_mesh_prepare;
mod fonts;
mod render_pass;
mod screenshot;
mod shell_setup;

use std::sync::Arc;

use anyhow::Result;
use winit::dpi::PhysicalSize;
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

use crate::AppEvent;
use crate::gfx::perf::{FrameSample, PerfAggregator};
use crate::model::{LogicalPx, PhysicalPx, PhysicalRect};
use crate::renderer::CellRenderer;
use crate::settings::AppearanceSettings;
use crate::state::AppState;

pub struct ImePreeditState {
    pub text: String,
    /// IME pre-edit composing cursor (row, col). 향후 caret 렌더링 추가 시 사용.
    #[allow(dead_code)] // 구조체 필드 — 향후 IME caret 렌더용 보존, 현재 미read
    pub cursor: Option<(usize, usize)>,
    pub anchor_col: usize,
    pub anchor_row: usize,
    pub surface_id: u32,
}

/// Actions returned by the shell setup dialog.
pub enum ShellSetupAction {
    None,
    Confirmed,
    Exit,
}

pub struct GpuState {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) config: wgpu::SurfaceConfiguration,
    pub(super) size: PhysicalSize<u32>,
    pub(super) renderer: CellRenderer,
    pub(crate) egui_ctx: egui::Context,
    pub(super) egui_state: egui_winit::State,
    pub(super) egui_renderer: egui_wgpu::Renderer,
    pub(super) scale_factor: f32,
    /// Last-applied terminal font signature (see `egui_bridge::term_font_signature`).
    /// post_egui_update re-runs `update_font` only when this string changes,
    /// covering every `EffectiveFont` field plus the scale-resolved size — closes
    /// the holes around `custom_font_path` and the empty-string font_family
    /// normalization mismatch.
    pub(super) last_term_font_sig: String,
    /// Tracks per-surface egui font signatures so we re-register only on change.
    pub(super) surface_font_state: crate::adapters::ui::font_registry::SurfaceFontState,
    /// egui-mesh surface_id → 전용 `egui_wgpu::Renderer` + 디코드 캐시 (A1-S5).
    /// surface 단위 전용 Renderer 로 plugin/host 간 TextureId 충돌을 격리한다(§4-3).
    /// surface 가 layout 에서 사라지면 정리돼 GPU 자원이 해제된다.
    pub(in crate::gfx::gpu) egui_mesh_targets:
        std::collections::HashMap<u32, egui_mesh_prepare::EguiMeshRenderTarget>,
    /// egui-mesh popup instance_id → 전용 `egui_wgpu::Renderer` + 디코드 캐시 (A2).
    /// surface 와 동형이되 popup 은 host egui pass *후* 합성된다. popup 이 닫히면 정리.
    pub(in crate::gfx::gpu) egui_mesh_popup_targets:
        std::collections::HashMap<u64, egui_mesh_prepare::EguiMeshRenderTarget>,
    /// egui-mesh banner instance_id → 전용 `egui_wgpu::Renderer` + 디코드 캐시 (A3).
    /// popup 과 동형 — host egui pass *후* content_rect 에 합성된다. banner 가 닫히면 정리.
    pub(in crate::gfx::gpu) egui_mesh_banner_targets:
        std::collections::HashMap<u64, egui_mesh_prepare::EguiMeshRenderTarget>,
    /// When set, the next render will capture the frame to this path as PNG.
    pub pending_screenshot: Option<std::path::PathBuf>,
    /// Frame timing 집계기. `RUST_LOG=tasty::gfx::perf=info` 일 때만 출력.
    pub(super) perf: PerfAggregator,
    /// winit 이벤트 루프 proxy — CSD titlebar close 버튼이 per-window 닫기
    /// (`AppEvent::CloseWindow`)를 발화하는 경로. egui repaint callback 과 별개 사본.
    pub(super) proxy: EventLoopProxy<AppEvent>,
}

impl GpuState {
    /// Per-window GPU 초기화. `instance`/`adapter` 는 App 이 부트 시 1회 생성해
    /// 모든 윈도우가 공유하는 컨텍스트(`Arc`)를 주입받는다 — 창마다 `Instance::new`
    /// (~50ms) + `request_adapter`(다중 백엔드 어댑터 열거, ~137ms) 를 반복하지 않는다.
    /// surface/device/config/CellRenderer/egui 는 per-window 로 새로 만든다.
    ///
    /// ⚠️ wgpu 제약: 모든 surface 는 동일 `Instance` 에서 생성돼야 하고, surface 는
    /// 그 `Instance` 수명에 의존한다 → App 이 `Arc<Instance>` 를 모든 창보다 오래
    /// 소유하므로 충족된다. `Backends::all()` 은 instance 생성 측(App)에서 유지되어
    /// 백엔드 자동 선택은 불변이다.
    pub(crate) async fn new_shared(
        instance: &Arc<wgpu::Instance>,
        adapter: &Arc<wgpu::Adapter>,
        window: Arc<Window>,
        appearance: &AppearanceSettings,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Result<Self> {
        let size = window.inner_size();
        let scale_factor = window.scale_factor() as f32;

        let surface = instance.create_surface(window.clone())?;

        tracing::info!(
            "GPU adapter: {} ({:?})",
            adapter.get_info().name,
            adapter.get_info().backend
        );

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("tasty_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                },
                None,
            )
            .await?;

        let surface_caps = surface.get_capabilities(adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or_else(|| anyhow::anyhow!("no supported surface format found"))?;

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: if surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
            {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::Fifo
            },
            alpha_mode: surface_caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create renderer with effective terminal font settings.
        let term_font = appearance.effective_terminal_font();
        let effective_font_size = term_font.effective_font_size(scale_factor);
        let renderer = CellRenderer::new(
            &device,
            &queue,
            surface_format,
            effective_font_size,
            &term_font.font_family,
        );

        // egui setup
        let egui_ctx = egui::Context::default();

        // Disable egui's built-in Ctrl+/- zoom — it only affects egui widgets
        // but not the terminal renderer, causing inconsistent scaling.
        egui_ctx.options_mut(|opts| {
            opts.zoom_with_keyboard = false;
        });

        // egui_extras image loaders (SVG / PNG / ...). 정적 SVG 아이콘 (chevron 등) 을
        // `egui::include_image!` 로 임베드한 뒤 `egui::Image` 위젯에서 사용한다.
        egui_extras::install_image_loaders(&egui_ctx);

        // Register bundled D2Coding (primary monospace) and system CJK fallback in egui.
        Self::setup_egui_fonts(&egui_ctx);

        // Connect egui's repaint requests to the winit event loop.
        // Without this, egui's internal repaints (new window registration,
        // cursor blink, animations) are silently dropped, causing the
        // Settings window to appear only after the next user input.
        let repaint_proxy = proxy.clone();
        // window_id 로 라우팅한다 — 모든 egui Context 는 root viewport 만 쓰므로
        // info.viewport_id 는 항상 ROOT 라 멀티 윈도우(모달 등)에서 윈도우를 구분할 수 없다.
        let repaint_window_id = window.id();
        egui_ctx.set_request_repaint_callback(move |info: egui::RequestRepaintInfo| {
            // delay 가 0 인 즉시 repaint 만 winit 큐로 보낸다.
            // delay > 0 (cursor blink, hover delay 등) 은 drop — 그렇지 않으면 매 frame 끝마다
            // 무조건 다음 frame 이 깨워져 idle 시 ~10fps continuous loop 가 생긴다.
            // 진행 중인 animation 은 ctx.request_repaint() 가 별도로 즉시 repaint 를 발화하므로
            // drop 해도 동작에 영향 없음.
            if info.delay.is_zero() {
                crate::shortcuts::send_app_event(
                    &repaint_proxy,
                    AppEvent::EguiRepaint {
                        window_id: repaint_window_id,
                    },
                );
            }
        });

        // Apply theme from settings
        let ui_zoom = appearance.ui_scale_factor();
        tasty_themes::install_global_with_zoom(appearance, ui_zoom);
        Self::apply_theme(&egui_ctx, &appearance.theme);

        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui_ctx.viewport_id(),
            &window,
            Some(scale_factor),
            None,
            Some(2048),
        );

        let egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1, false);

        let last_term_font_sig = egui_bridge::term_font_signature(&term_font, effective_font_size);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            size,
            renderer,
            egui_ctx,
            egui_state,
            egui_renderer,
            scale_factor,
            last_term_font_sig,
            surface_font_state: crate::adapters::ui::font_registry::SurfaceFontState::default(),
            egui_mesh_targets: std::collections::HashMap::new(),
            egui_mesh_popup_targets: std::collections::HashMap::new(),
            egui_mesh_banner_targets: std::collections::HashMap::new(),
            pending_screenshot: None,
            perf: PerfAggregator::new(),
            proxy,
        })
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        self.renderer
            .resize(&self.queue, new_size.width, new_size.height);
    }

    /// Pass a winit event to egui. Returns (consumed, repaint).
    pub fn handle_egui_event(
        &mut self,
        window: &Window,
        event: &winit::event::WindowEvent,
    ) -> (bool, bool) {
        let response = self.egui_state.on_window_event(window, event);
        (response.consumed, response.repaint)
    }

    /// Render the full frame: egui UI + terminal surfaces.
    #[allow(clippy::too_many_arguments)] // reason: 프레임 렌더 컨텍스트 전체
    pub fn render(
        &mut self,
        state: &mut AppState,
        engine: &mut crate::core::CoreState,
        window: &Window,
        preedit: Option<&ImePreeditState>,
        selection: Option<&crate::selection::TextSelection>,
        vi_cursor: Option<(u32, crate::selection::SelectionPoint)>,
        link_hover: Option<(u32, &crate::terminal_link::LinkHighlight)>,
        plugin_manager: Option<&crate::plugin::PluginManager>,
    ) -> Result<(), wgpu::SurfaceError> {
        let render_start = std::time::Instant::now();

        // 1. Prepare layout
        state.sidebar_width = if !state.sidebar_visible {
            LogicalPx(0.0)
        } else if state.sidebar_collapsed {
            LogicalPx(48.0) // Compact mode: narrow width for collapse button
        } else {
            engine.settings.appearance.scaled_sidebar_width()
        };
        let terminal_rect = self.compute_terminal_rect(state.sidebar_width);
        // Single display-point reify: any deferred placeholder about to be drawn
        // (active workspace → each pane's active tab) gets its PTY spawned here,
        // before resize_all/render. Covers every exposure path (keyboard tab
        // switch, tab close, pane focus, ws switch, restore) without per-handler
        // hooks. No-op when nothing is deferred.
        state.reify_displayed_surfaces(engine);
        state.resize_all(
            engine,
            terminal_rect,
            self.renderer.cell_width(),
            self.renderer.cell_height(),
        );

        let (pane_rects, dividers, focused_surface_id) =
            self.prepare_layout(state, engine, terminal_rect);

        // Clear notification highlight on the currently focused surface
        if let Some(sid) = focused_surface_id {
            engine.notifications.clear_surface_highlight(sid);
        }

        let layout_ms = render_start.elapsed().as_secs_f64() * 1000.0;

        // 2. Pre-egui updates: register surface fonts before drawing.
        // Markdown panels reference the named font family "font_markdown". It must
        // be registered before run_egui_frame, or the first frame panics with
        // "FontFamily::Name(...) is not bound to any fonts".
        let prev_theme = engine.settings.appearance.theme.clone();
        crate::adapters::ui::font_registry::refresh_surface_fonts(
            &self.egui_ctx,
            &engine.settings.appearance,
            &mut self.surface_font_state,
        );

        // 3. Run egui frame (UI drawing)
        let t0 = std::time::Instant::now();
        let mut full_output = self.run_egui_frame(
            state,
            engine,
            window,
            &pane_rects,
            &dividers,
            terminal_rect,
            plugin_manager,
        );
        let egui_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 3. Cursor decision: egui first, then winit area (dividers + surfaces)
        // Resize-border cursor takes priority: when the pointer is on a window
        // resize border, `pending_resize_cursor` is Some and the egui frame has
        // already set a ResizeXxx icon. Skip the surface/link overrides so the
        // border cursor is not overwritten by the terminal surface I-beam.
        // (macOS never sets this field, so the guard is a no-op there.)
        if state.pending_resize_cursor.is_none()
            && !self.egui_ctx.is_pointer_over_area()
            && !state.popup_hovered
            && !state.banner_hovered
            && let Some(pos) = self.egui_ctx.input(|i| i.pointer.hover_pos())
        {
            let px = pos.x * self.scale_factor;
            let py = pos.y * self.scale_factor;
            if let Some(icon) = state.winit_cursor_icon_at(engine, px, py, terminal_rect, 4.0) {
                full_output.platform_output.cursor_icon = icon;
            }
        }
        // Link hover overrides cursor to pointing-hand (unless on a resize border).
        if link_hover.is_some() && state.pending_resize_cursor.is_none() {
            full_output.platform_output.cursor_icon = egui::CursorIcon::PointingHand;
        }

        // 4. Post-egui updates (theme/font refresh)
        let t0 = std::time::Instant::now();
        self.post_egui_update(engine, &prev_theme);
        let post_egui_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // egui-winit disables IME when no egui text field is focused
        // (calls set_ime_allowed(false) when self.allow_ime differs from
        // ime.is_some()). The terminal always needs IME active.
        // Pre-set allow_ime=false so that when egui computes allow_ime=false
        // (no text field), the check false!=false is false and it skips
        // the set_ime_allowed(false) call entirely.
        let t0 = std::time::Instant::now();
        self.egui_state.set_allow_ime(false);
        self.egui_state
            .handle_platform_output(window, full_output.platform_output);

        // popup이 focused면 IME를 비활성화하여 KeyboardInput이 직접 발생하도록 한다.
        // 이렇게 하면 한글 IME 활성 상태에서도 popup 단축키가 physical_key로 매칭된다.
        //
        // Windows 예외: winit Windows의 set_ime_allowed는 ImmAssociateContextEx(IACE_DEFAULT/
        // IACE_CHILDREN)로 IMC를 매번 attach/detach시킨다. 이 association churn이 한/영 키
        // (VK_HANGUL) 토글을 가끔 망가뜨린다(다른 앱으로 갔다 오면 풀리는 증상의 원인).
        // Windows winit은 IME 활성 상태에서도 KeyboardInput과 physical_key를 정상 emit하므로,
        // popup 단축키 매칭에 IME 비활성화가 필요 없다. 따라서 Windows는 항상 IME를 허용한다.
        #[cfg(not(windows))]
        {
            let disable_ime = state.popups.has_focused() && !state.has_input_dialog_open();
            window.set_ime_allowed(!disable_ime);
        }
        #[cfg(windows)]
        window.set_ime_allowed(true);
        let platform_output_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 5. Tessellate egui
        let t0 = std::time::Instant::now();
        let paint_jobs = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: full_output.pixels_per_point,
        };
        let tessellate_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 5. GPU render
        let t0 = std::time::Instant::now();
        let regions = state.surface_regions(engine, terminal_rect);
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.render_clear_pass(&view, state, engine);
        let clear_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t0 = std::time::Instant::now();
        let search_state = if state.search.matches.is_empty() {
            None
        } else {
            Some(&state.search)
        };
        self.render_terminals(
            &view,
            &regions,
            engine,
            focused_surface_id,
            selection,
            vi_cursor,
            &engine.settings.appearance,
            preedit,
            link_hover,
            search_state,
        );
        let terminals_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // egui-mesh surface 합성 (A1-S5): terminal 콘텐츠와 같은 layer(host chrome 아래).
        // plugin 이 자기 프로세스에서 tessellate 한 mesh 를 전용 Renderer 로 영역 합성한다.
        if let Some(mgr) = plugin_manager {
            let mesh_targets =
                egui_mesh_prepare::collect_egui_mesh_targets(state, engine, terminal_rect);
            if !mesh_targets.is_empty() {
                self.render_egui_mesh_surfaces(&view, &mesh_targets, mgr);
            }
        }

        let t0 = std::time::Instant::now();
        self.render_egui_pass(
            &view,
            &full_output.textures_delta,
            &paint_jobs,
            &screen_descriptor,
        );
        let egui_pass_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // egui-mesh popup 합성 (A2): host egui pass *후* — 셸(scrim/bg/border)을 egui 가
        // 그린 뒤 content_rect 에 plugin mesh 를 얹는다. `draw_plugin_popups` 가 egui frame
        // 중 채운 영역을 읽는다. mesh 는 content_rect 로 clip 되어 셸을 덮지 않는다.
        if let Some(mgr) = plugin_manager
            && !state.plugin_mesh_popup_regions.is_empty()
        {
            let regions = state.plugin_mesh_popup_regions.clone();
            self.render_egui_mesh_popups(&view, &regions, mgr);
        }

        // egui-mesh banner 합성 (A3): popup 과 동형 — host egui pass *후* content_rect 에
        // plugin mesh 를 얹는다. 셸(컨테이너/border/close X/카운트다운)은 host egui(banner
        // manager)가 그렸고, content 만 여기서 합성된다. `draw_plugin_banners` 가 적재한 영역.
        if let Some(mgr) = plugin_manager
            && !state.plugin_mesh_banner_regions.is_empty()
        {
            let regions = state.plugin_mesh_banner_regions.clone();
            self.render_egui_mesh_banners(&view, &regions, mgr);
        }

        // 6. Screenshot + present
        let t0 = std::time::Instant::now();
        if let Some(path) = self.pending_screenshot.take() {
            self.capture_frame_to_png(&output.texture, &path);
        }
        output.present();
        let present_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // --- GPU render timing ---
        let gpu_total_ms = render_start.elapsed().as_secs_f64() * 1000.0;
        const SLOW_RENDER_MS: f64 = 30.0;
        if gpu_total_ms > SLOW_RENDER_MS {
            tracing::warn!(
                "slow gpu render: {gpu_total_ms:.1}ms \
                 [layout={layout_ms:.1}, egui_frame={egui_frame_ms:.1}, \
                 post_egui={post_egui_ms:.1}, platform_output={platform_output_ms:.1}, \
                 tessellate={tessellate_ms:.1}, clear={clear_ms:.1}, \
                 terminals={terminals_ms:.1}, egui_pass={egui_pass_ms:.1}, \
                 present={present_ms:.1}]"
            );
        }

        let (_, _, draw_total) = self.renderer.draw_call_count();
        self.perf.push(FrameSample {
            gpu_total_ms,
            terminals_ms,
            draw_calls_total: draw_total,
            surfaces: self.renderer.active_surface_count(),
            atlas_evictions: self.renderer.atlas.eviction_count(),
            atlas_active_pages: self.renderer.atlas.active_page_count(),
            atlas_entry_count_sum: self.renderer.atlas.entry_count_sum(),
        });

        Ok(())
    }

    fn compute_terminal_rect(&self, sidebar_width: LogicalPx) -> PhysicalRect {
        crate::model::compute_terminal_rect(
            PhysicalPx(self.size.width as f32),
            PhysicalPx(self.size.height as f32),
            sidebar_width,
            crate::adapters::ui::titlebar::top_inset(self.scale_factor),
            crate::adapters::ui::status_bar_bottom_inset(self.scale_factor),
            self.scale_factor,
        )
    }

    fn prepare_layout(
        &self,
        state: &AppState,
        engine: &crate::core::CoreState,
        terminal_rect: PhysicalRect,
    ) -> (Vec<(u32, PhysicalRect)>, Vec<PhysicalRect>, Option<u32>) {
        let pane_layout = state.active_workspace(engine).pane_layout();
        let pane_rects: Vec<(u32, PhysicalRect)> = pane_layout.compute_rects(terminal_rect);
        let mut dividers: Vec<PhysicalRect> = pane_layout.collect_dividers(terminal_rect);

        let focused_surface_id = state.focused_surface_id(engine);
        for (pane_id, pane_rect) in &pane_rects {
            if let Some(pane) = pane_layout.find_pane(*pane_id) {
                let tab_bar_h = state.tab_bar_height;
                let content_rect = PhysicalRect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
                };
                if let Some(tab) = pane.tabs.get(pane.active_tab) {
                    dividers.extend(tab.layout().collect_dividers(content_rect));
                }
            }
        }
        (pane_rects, dividers, focused_surface_id)
    }

    /// Compute grid size for a given rect.
    pub fn grid_size_for_rect(&self, rect: &PhysicalRect) -> (usize, usize) {
        self.renderer.grid_size_for_rect(rect)
    }

    pub fn cell_width(&self) -> f32 {
        self.renderer.cell_width()
    }

    pub fn cell_height(&self) -> f32 {
        self.renderer.cell_height()
    }

    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// debug overlay 후보 — egui frame counter 노출용.
    #[allow(dead_code)] // pub 진단 accessor — debug overlay 배선 시 사용, 현 호출처 0
    pub fn egui_frame_nr(&self) -> u64 {
        self.egui_ctx.cumulative_pass_nr()
    }

    /// Get egui's actual pixels_per_point (what it uses for rendering).
    pub fn egui_pixels_per_point(&self) -> f32 {
        self.egui_ctx.pixels_per_point()
    }

    /// debug 전용: 다음 frame 의 egui 입력 큐에 합성 이벤트를 주입한다(egui-mesh popup
    /// 입력 forward 의 헤드리스 검증용). release 미노출 — 사용자 입력 재현은 debug 격리.
    #[cfg(debug_assertions)]
    pub fn debug_push_egui_events(&mut self, events: Vec<egui::Event>) {
        self.egui_state.egui_input_mut().events.extend(events);
        self.egui_ctx.request_repaint();
    }

    /// Get egui's zoom factor.
    pub fn egui_zoom_factor(&self) -> f32 {
        self.egui_ctx.zoom_factor()
    }

    /// Whether egui-winit currently allows IME on the window.
    pub fn egui_ime_allowed(&self) -> bool {
        self.egui_state.allow_ime()
    }

    /// Get the wgpu surface config dimensions.
    pub fn surface_config_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Update the scale factor (e.g., when the window moves between monitors with different DPI).
    pub fn update_scale_factor(&mut self, new_scale_factor: f32) {
        self.scale_factor = new_scale_factor;
        // Reset egui zoom to 1.0 so that egui_winit's native_pixels_per_point
        // (provided via take_egui_input each frame) is used directly.
        // DO NOT call set_pixels_per_point() here — it computes
        // zoom = ppp / native_ppp, and if native_ppp hasn't been updated yet
        // (e.g., during macOS auto-restore), zoom gets a stale value like 0.5
        // that persists forever.
        self.egui_ctx.set_zoom_factor(1.0);
    }

    /// Re-sync scale factor from the window and resize if it changed.
    /// Returns true if scale factor was updated.
    pub fn sync_scale_factor(&mut self, window: &Window) -> bool {
        let current_sf = window.scale_factor() as f32;
        if (current_sf - self.scale_factor).abs() > f32::EPSILON {
            self.update_scale_factor(current_sf);
            let new_size = window.inner_size();
            self.resize(new_size);
            true
        } else {
            false
        }
    }
}
