mod boot_error;
mod egui_bridge;
mod egui_mesh_prepare;
mod fonts;
pub(crate) mod loading;
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
use crate::stall_watchdog;
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

/// 부팅 실패 화면에 그릴 진단(제목/본문/힌트). GPU 는 살아있으나 엔진 생성이 실패했을
/// 때, 런처(dock/시작 메뉴)로 실행한 사용자는 stderr 를 못 봐 "창이 깜빡이고 사라지는
/// 것" 이 전부다 — 그 진단을 창에 그려 보인다. i18n 해석은 App 층에서 하고 여기엔 해석된
/// 문자열만 담는다(gpu 층은 i18n 을 모른다). 근거:
/// `docs/adr/0117-window-and-modal-creation-failure-policy.md` 재검토 트리거.
pub struct BootErrorInfo {
    pub title: String,
    pub body: String,
    pub hint: String,
}

/// surface configure 치수를 물리 유효 범위로 clamp 한다.
///
/// wgpu 는 `surface.configure` 에 `max_texture_dimension_2d` 를 넘는 width/height 가
/// 오면 panic 한다(TD-7 crash 근본원인: 외부 `SetWindowPos` 등이 winit `Resized`
/// 이벤트로 65535 를 유입). tasty 자기 코드(IPC/CLI/시작단/split)로 상한 초과를
/// 주입하는 진입점은 없으므로(research §3·§4), 방어는 winit 경계에서의 clamp 하나로
/// 일원화한다 — 거부 계층은 막을 진입점이 없어 추가하지 않는다.
///
/// 하한 `1`(configure 는 0 불가), 상한 `max`(어댑터별 실제 한계, 런타임 조회).
/// 0 은 최소화 신호로 호출 전 early-return 이 처리하므로 이 함수 진입 전 걸러진다.
fn clamp_surface_dims(w: u32, h: u32, max: u32) -> (u32, u32) {
    (w.clamp(1, max), h.clamp(1, max))
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
    /// textures_delta 체인 단절이 감지된 egui-mesh surface — full 재전송 요청 대기열.
    /// 렌더 prepare 가 적재하고, [`Self::take_egui_mesh_full_requests`] 로 forward 측이
    /// 다음 tick 에 소비해 `need_full_textures` set_context 를 보낸다.
    pub(in crate::gfx::gpu) egui_mesh_full_requests: std::collections::HashSet<u32>,
    /// popup 대응 full 재전송 요청 대기열 (instance_id).
    pub(in crate::gfx::gpu) egui_mesh_popup_full_requests: std::collections::HashSet<u64>,
    /// banner 대응 full 재전송 요청 대기열 (instance_id).
    pub(in crate::gfx::gpu) egui_mesh_banner_full_requests: std::collections::HashSet<u64>,
    /// attach mesh mirror surface(`AttachMeshSurface`) local surface_id → 전용
    /// `egui_wgpu::Renderer` + 디코드 캐시 (`docs/dev-guide/attach-behavior.md` "mesh mirror 채널").
    /// `egui_mesh_targets`와 별도 맵인 이유는
    /// [`egui_mesh_prepare::GpuState::render_attach_mesh_surfaces`] 문서 참조.
    pub(in crate::gfx::gpu) attach_mesh_targets:
        std::collections::HashMap<u32, egui_mesh_prepare::EguiMeshRenderTarget>,
    /// attach mesh mirror surface 의 텍스처 delta 체인 단절 → full 재전송 요청 대기열.
    /// [`Self::take_attach_mesh_full_requests`]로 drain, `attach_client`가 소비해
    /// `StreamControl::MeshFullResendRequest`를 서버로 보낸다.
    pub(in crate::gfx::gpu) attach_mesh_full_requests: std::collections::HashSet<u32>,
    /// When set, the next render will capture the full window frame to this path as PNG.
    pub pending_screenshot: Option<std::path::PathBuf>,
    /// When set, the next render will capture the given terminal surface to this
    /// path as PNG via an offscreen pass — independent of the swapchain, visible
    /// tab, and focus (the surface is rendered at its own grid size). See
    /// [`Self::capture_surface_to_png`].
    pub pending_surface_screenshot: Option<(u32, std::path::PathBuf)>,
    /// Frame timing 집계기. `TASTY_LOG=tasty::gfx::perf=info` 일 때만 출력.
    pub(super) perf: PerfAggregator,
    /// Set when a frame hid the focused terminal cursor during an output burst.
    /// The view consumes this to request one follow-up redraw so the cursor
    /// reappears after the burst settles even if no more PTY bytes arrive.
    pub(super) terminal_cursor_restore_pending: bool,
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
        theme_runtime: tasty_themes::ThemeRuntime,
        wheel_line_scroll: f32,
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

        // startup 방어 일관성: resize 와 동일한 clamp 로 상한도 승격한다.
        // 고정 720p 요청이라 상한 초과는 현실적으로 불가하나, resize 와 같은
        // 헬퍼를 재사용해 물리 유효 범위를 한 곳에서 보장한다(research §6-2).
        let max_dim = device.limits().max_texture_dimension_2d;
        let (config_width, config_height) = clamp_surface_dims(size.width, size.height, max_dim);
        // 실제로 어느 present mode 가 선택됐는지는 프레임 상한을 판단할 때 필요한
        // 정보인데(`src/view/repaint.rs`), 가상 디스플레이 환경에서는 `Fifo` 여도
        // 하드웨어 vblank 가 없어 상한 역할을 못 할 수 있어 역산이 불가능하다.
        // 그래서 선택 결과를 그대로 남긴다.
        let present_mode = if surface_caps
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };
        tracing::info!(
            "surface present_mode={present_mode:?} (available: {:?})",
            surface_caps.present_modes
        );
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            format: surface_format,
            width: config_width,
            height: config_height,
            present_mode,
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
            // 휠 1노치 거리는 tasty 가 정한다 — egui 는 이 값을 native 40 / web 8 로
            // 갈라 두고 왜 달라야 하는지 자기 소스에 TODO 로 남겼다. 이 컨텍스트를 쓰는
            // 모든 `ScrollArea` 와 plugin 표면이 이 한 값을 공유한다(ADR-0130).
            opts.line_scroll_speed = wheel_line_scroll;
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
        tasty_themes::install_global_with_runtime(appearance, theme_runtime);
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
            egui_mesh_full_requests: std::collections::HashSet::new(),
            egui_mesh_popup_full_requests: std::collections::HashSet::new(),
            egui_mesh_banner_full_requests: std::collections::HashSet::new(),
            attach_mesh_targets: std::collections::HashMap::new(),
            attach_mesh_full_requests: std::collections::HashSet::new(),
            pending_screenshot: None,
            pending_surface_screenshot: None,
            perf: PerfAggregator::new(),
            terminal_cursor_restore_pending: false,
            proxy,
        })
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        // 0 은 최소화 신호 — configure 를 스킵한다(early-return 유지).
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        // 상한 clamp: 외부 SetWindowPos 등이 winit Resized 로 max 초과 치수를 유입하면
        // surface.configure 가 panic 한다(TD-7). 어댑터별 실제 한계를 런타임 조회해
        // clamp 하고, 실제로 clamp 가 걸리면 warn 으로 남긴다(하드코딩 상한 금지).
        let max = self.device.limits().max_texture_dimension_2d;
        let (w, h) = clamp_surface_dims(new_size.width, new_size.height, max);
        if w != new_size.width || h != new_size.height {
            tracing::warn!(
                req_w = new_size.width,
                req_h = new_size.height,
                max,
                "surface resize request exceeds max_texture_dimension_2d; clamping"
            );
        }
        let clamped = PhysicalSize::new(w, h);
        self.size = clamped;
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.renderer.resize(&self.queue, w, h);
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

    /// egui 입력 큐에 `PointerGone` 을 직접 밀어 넣는다.
    ///
    /// winit 마우스 이벤트를 egui 에 **먹이지 않고 삼키는** 경로(네이티브 컨텍스트
    /// 메뉴를 닫는 바깥 클릭 — `view/main/mouse.rs::menu_dismiss_swallow_step`)에서
    /// 쓴다. feed 를 건너뛰는 것만으로도 그 사이클의 press/release 쌍은 완성되지
    /// 않지만, 직전 프레임까지 잡혀 있던 hover 하이라이트가 남는 것을 막고 혹시
    /// 흘러든 press 상태가 있으면 함께 끊는다.
    pub fn push_egui_pointer_gone(&mut self) {
        self.egui_state
            .egui_input_mut()
            .events
            .push(egui::Event::PointerGone);
    }

    /// 렌더 prepare 가 적재한 egui-mesh full 재전송 요청(surface_id)을 비우며 가져간다.
    /// MainView 가 render 직후 소비해, 다음 tick 의 forward 에서 해당 surface 에
    /// `need_full_textures` set_context 를 보낸다.
    pub fn take_egui_mesh_full_requests(&mut self) -> std::collections::HashSet<u32> {
        std::mem::take(&mut self.egui_mesh_full_requests)
    }

    /// popup 대응 full 재전송 요청(instance_id) drain — [`Self::take_egui_mesh_full_requests`].
    pub fn take_egui_mesh_popup_full_requests(&mut self) -> std::collections::HashSet<u64> {
        std::mem::take(&mut self.egui_mesh_popup_full_requests)
    }

    /// banner 대응 full 재전송 요청(instance_id) drain — [`Self::take_egui_mesh_full_requests`].
    pub fn take_egui_mesh_banner_full_requests(&mut self) -> std::collections::HashSet<u64> {
        std::mem::take(&mut self.egui_mesh_banner_full_requests)
    }

    /// attach mesh mirror surface 의 full 재전송 요청(local surface_id) drain —
    /// `attach_client`가 매 tick 소비해 owning 세션에 `MeshFullResendRequest`를 보낸다.
    pub fn take_attach_mesh_full_requests(&mut self) -> std::collections::HashSet<u32> {
        std::mem::take(&mut self.attach_mesh_full_requests)
    }

    /// Whether the last render hid a terminal cursor and needs one follow-up
    /// frame to restore it after the output burst quiets down.
    pub fn take_terminal_cursor_restore_pending(&mut self) -> bool {
        std::mem::take(&mut self.terminal_cursor_restore_pending)
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

        // 0. Offscreen surface screenshot (agent action, focus-independent).
        // Rendered to its own texture at the surface's grid size — never touches
        // the swapchain, visible tab, present, or focus. Runs before the live
        // frame so the shared renderer accumulator (reset by `render_terminals`'
        // `begin_frame`) and the projection uniform (restored inside the helper)
        // stay coherent for the visible frame that follows.
        self.handle_pending_surface_screenshot(engine);

        // 0-b. 전체화면 무대 분기 — **이 위치가 계약이다.** 위아래로 한 칸씩 밀면
        // 조용히 죽는 기능이 있다:
        //
        // - 더 위(`render()` 최상단, 위 offscreen 캡처 **앞**)로 옮기면
        //   `ui.screenshot --surface <id>` 요청이 큐에 남아 영구 대기한다. 그건
        //   release 에이전트 기능이고 포커스 독립이어야 한다
        //   (`docs/design/policies/focus.md`) — 무대 때문에 죽으면 안 된다.
        // - 더 위(`MainView::render_if_dirty` 조기 반환)로 옮기면 attach mesh relay 가
        //   죽는다. 로컬 사용자가 전체화면을 켰다고 원격 사용자 화면이 멈추는 것은
        //   `docs/identity.md` §동시성(주체 간 비침범) 위반이다.
        // - 더 아래로 밀면 레이아웃/`resize_all` 이 먼저 돌아 무대 중에도 PTY grid 가
        //   재계산된다 — "원본은 진입 시점 그대로" 계약이 깨진다.
        //
        // 그래서 이것은 "조기 반환" 이 아니라 background live-frame 과 stage frame 의
        // **분리**다. 무대 경로도 window 캡처 + `present` 는 그대로 수행한다
        // (`render_fullscreen_stage`). 근거 전체: `docs/design/systems/fullscreen-stage.md`.
        if state.fullscreen_stage_active() {
            return self.render_fullscreen_stage(state, engine, window);
        }

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
            self.scale_factor,
        );

        let (pane_rects, dividers, focused_surface_id) =
            self.prepare_layout(state, engine, terminal_rect);

        // Clear surface attention on the currently focused surface. `focused_surface_id`
        // 는 실제 렌더 시점 포커스(에이전트 주입 아님)라 불가침 원칙 1 에 안전하다.
        if let Some(sid) = focused_surface_id {
            // `clear_attention` 이 아니라 로컬 축 진입점을 쓴다 — 하드 점유(attach) 중인
            // surface 는 홀더만 해제할 수 있으므로 이 로컬 포커스는 건너뛴다(ADR-0109).
            engine.clear_attention_local(sid);
            // soft 점유 지연 청소(ADR-0040 §수명): 실-포커스 surface 의 soft 주체(parent)가
            // 사라졌으면 이 시점에 점유 해제. attention clear 와 같은 실-포커스 블록이라
            // 원칙1 안전. **위 게이트와 무관하다** — soft 점유 청소는 attention 해제 권한과
            // 별개 동작이고, hard 점유 surface 는 이 함수가 자체적으로 조기 반환한다.
            engine.reconcile_soft_occupancy_on_focus(sid);
        }

        let layout_ms = render_start.elapsed().as_secs_f64() * 1000.0;

        // 2. Pre-egui updates: register surface fonts before drawing.
        // Host-rendered panels that reference a per-kind named family ("font_<kind>")
        // need it bound before run_egui_frame, or the first frame panics with
        // "FontFamily::Name(...) is not bound to any fonts". Registration is generic
        // over the registered override kinds (no specific kind hardcoded).
        let prev_theme = engine.settings.appearance.theme.clone();
        crate::adapters::ui::font_registry::refresh_surface_fonts(
            &self.egui_ctx,
            &engine.settings.appearance,
            &mut self.surface_font_state,
        );

        // host popup(`PopupManager`)과 plugin popup(egui-mesh) 사이의 통합 z-order 판정
        // (`docs/design/systems/popup.md` 규칙 7) — 셸 등록 순서(`run_egui_frame` 내부)와
        // GPU 콘텐츠 합성 순서(아래 `render_egui_pass`/`render_egui_mesh_popups`) 양쪽이
        // 같은 프레임 안에서 같은 결정을 따라야 하므로 한 번만 계산해 재사용한다.
        let host_top_z_seq = state.popups.max_open_z_seq();
        let plugin_top_z_seq =
            plugin_manager.and_then(|m| m.popup_instances().map(|(_, inst)| inst.z_seq).max());
        let host_popup_on_top =
            egui_bridge::host_popup_should_render_on_top(host_top_z_seq, plugin_top_z_seq);

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
            host_popup_on_top,
        );
        let egui_frame_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // 3. Cursor decision: egui first, then winit area (dividers + surfaces)
        // Resize-border cursor takes priority: when the pointer is on a window
        // resize border, `pending_resize_cursor` is Some and the egui frame has
        // already set a ResizeXxx icon. Skip the surface/link overrides so the
        // border cursor is not overwritten by the terminal surface I-beam.
        // (macOS never sets this field, so the guard is a no-op there.)
        if let Some(icon) = self.resolve_cursor_icon(state, engine, terminal_rect, link_hover) {
            full_output.platform_output.cursor_icon = icon;
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
        // 나머지 판정(popup focus + 텍스트 입력 위젯 focus 여부)은
        // `apply_platform_output` 문서 참조.
        let t0 = std::time::Instant::now();
        self.apply_platform_output(window, state, full_output.platform_output);
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
        let regions = state.surface_regions(engine, terminal_rect, self.scale_factor);
        stall_watchdog::set_phase(stall_watchdog::Phase::Acquire);
        let output = self.surface.get_current_texture()?;
        stall_watchdog::set_phase(stall_watchdog::Phase::Submit);
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
        // existing(비활성 탭/workspace 포함)이 비어도 호출해 닫힌 surface 의 GPU 자원을
        // retain 으로 정리한다(빈 target 게이팅은 `render_egui_mesh_surfaces` 내부에서 처리).
        if let Some(mgr) = plugin_manager {
            let mesh_targets = egui_mesh_prepare::collect_egui_mesh_targets(
                state,
                engine,
                terminal_rect,
                self.scale_factor,
            );
            let mesh_existing = state.egui_mesh_surfaces_existing(engine);
            self.render_egui_mesh_surfaces(&view, &mesh_targets, &mesh_existing, mgr);
        }

        // attach mesh mirror surface 합성(`docs/dev-guide/attach-behavior.md` 참고): 위
        // egui-mesh 합성과 동형이되
        // `PluginManager` 없이(원격에만 plugin 이 있음) `AttachMeshFrameStore`(TCP 로 받은
        // 최신 바이트)를 읽는다. `plugin_manager` 게이트가 없다 — attach 는 이 데이터에
        // 의존하지 않는다.
        let attach_mesh_targets = egui_mesh_prepare::collect_attach_mesh_targets(
            state,
            engine,
            terminal_rect,
            self.scale_factor,
        );
        let attach_mesh_existing = state.attach_mesh_surfaces_existing(engine);
        if !attach_mesh_targets.is_empty()
            || !attach_mesh_existing.is_empty()
            || !self.attach_mesh_targets.is_empty()
        {
            self.render_attach_mesh_surfaces(
                &view,
                &attach_mesh_targets,
                &attach_mesh_existing,
                &engine.attach_mesh_frames,
            );
        }

        // egui-mesh popup 합성(A2) + host egui pass — host popup ↔ plugin popup z-order
        // (`host_popup_on_top`) 에 따라 둘의 순서를 정한다. 상세는
        // `render_egui_pass_and_mesh_popups` 문서 참고.
        let t0 = std::time::Instant::now();
        self.render_egui_pass_and_mesh_popups(
            &view,
            &full_output.textures_delta,
            &paint_jobs,
            &screen_descriptor,
            state.plugin_mesh_popup_regions.as_slice(),
            plugin_manager,
            host_popup_on_top,
        );
        let egui_pass_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // egui-mesh banner 합성 (A3): popup 과 동형 — host egui pass *후* content_rect 에
        // plugin mesh 를 얹는다. 셸(컨테이너/border/close X/카운트다운)은 host egui(banner
        // manager)가 그렸고, content 만 여기서 합성된다. `draw_plugin_banners` 가 적재한 영역.
        // popup 과 동일 — 잔존 target prune 을 위해 빈 regions 에서도 호출.
        if let Some(mgr) = plugin_manager {
            let regions = state.plugin_mesh_banner_regions.clone();
            self.render_egui_mesh_banners(&view, &regions, mgr);
        }

        // 6. Screenshot + present
        let t0 = std::time::Instant::now();
        if let Some(path) = self.pending_screenshot.take() {
            self.capture_frame_to_png(&output.texture, self.size.width, self.size.height, &path);
        }
        stall_watchdog::set_phase(stall_watchdog::Phase::Present);
        // debug 결함 주입 지점 — release 는 no-op.
        stall_watchdog::take_debug_stall();
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

    /// Pending offscreen surface screenshot(agent action, focus-independent) 소비.
    /// A hard-occupied surface shows a readonly mirror server-side; capture what
    /// the user would see (mirror), else the live terminal.
    fn handle_pending_surface_screenshot(&mut self, engine: &crate::core::CoreState) {
        let Some((surface_id, path)) = self.pending_surface_screenshot.take() else {
            return;
        };
        let reverse_screen = engine.settings.general.reverse_screen_enabled;
        match engine.visible_terminal(surface_id) {
            Some(t) => self.capture_surface_to_png(t, reverse_screen, &path),
            None => {
                tracing::warn!(
                    "surface screenshot: surface {surface_id} has no terminal to capture"
                )
            }
        }
    }

    /// Cursor decision: egui first, then winit area (dividers + surfaces), then
    /// link hover. Resize-border cursor takes priority: when the pointer is on a
    /// window resize border, `pending_resize_cursor` is Some and the egui frame
    /// has already set a ResizeXxx icon, so the surface/link overrides below are
    /// skipped to avoid overwriting the border cursor with the terminal I-beam.
    /// (macOS never sets this field, so the guard is a no-op there.) Returns the
    /// icon to apply, or `None` to leave egui's own decision untouched.
    fn resolve_cursor_icon(
        &self,
        state: &AppState,
        engine: &crate::core::CoreState,
        terminal_rect: PhysicalRect,
        link_hover: Option<(u32, &crate::terminal_link::LinkHighlight)>,
    ) -> Option<egui::CursorIcon> {
        let mut icon = None;
        if state.pending_resize_cursor.is_none()
            && !self.egui_ctx.is_pointer_over_area()
            && !state.popup_hovered
            && !state.banner_hovered
            && !state.modifier_hint_hovered
            && let Some(pos) = self.egui_ctx.input(|i| i.pointer.hover_pos())
        {
            // egui 가 준 hover 좌표는 논리, `winit_cursor_icon_at` 은 물리를 받는다.
            let px = LogicalPx(pos.x).to_physical(self.scale_factor).value();
            let py = LogicalPx(pos.y).to_physical(self.scale_factor).value();
            icon = state.winit_cursor_icon_at(
                engine,
                px,
                py,
                terminal_rect,
                crate::state::mouse::divider_hit_threshold_physical(self.scale_factor),
            );
        }
        // Link hover overrides cursor to pointing-hand (unless on a resize border).
        if link_hover.is_some() && state.pending_resize_cursor.is_none() {
            icon = Some(egui::CursorIcon::PointingHand);
        }
        icon
    }

    /// egui `PlatformOutput` 적용 + IME 허용 여부 갱신.
    ///
    /// popup이 focused면서 그 안의 텍스트 입력 위젯은 focus되어 있지 않을 때만 IME를
    /// 비활성화하여 KeyboardInput이 직접 발생하도록 한다. 이렇게 하면 한글 IME 활성
    /// 상태에서도 popup 단축키(Escape/화살표 등)가 physical_key로 매칭된다.
    ///
    /// "텍스트 입력 위젯이 focus되어 있는가"는 popup id를 열거하는 대신 egui가 매
    /// 프레임 계산해 주는 `PlatformOutput::ime`(IME가 필요한 위젯이 실제로 focus
    /// 중일 때만 `Some`)로 판정한다 — search_bar/command_palette/port_scanner/
    /// approval/remote_tool/rename 등 텍스트 입력을 가진 모든 popup을 한 번에 커버하고,
    /// remote_tool처럼 폼 화면과 목록/네비게이션 화면이 한 popup 안에 공존해도 프레임
    /// 단위로 정확하다. `platform_output`은 아래에서 `handle_platform_output`에 통째로
    /// move되므로, `ime` 필드는 그 전에 먼저 읽어 둔다.
    ///
    /// Windows 예외: winit Windows의 set_ime_allowed는 ImmAssociateContextEx(IACE_DEFAULT/
    /// IACE_CHILDREN)로 IMC를 매번 attach/detach시킨다. 이 association churn이 한/영 키
    /// (VK_HANGUL) 토글을 가끔 망가뜨린다(다른 앱으로 갔다 오면 풀리는 증상의 원인).
    /// Windows winit은 IME 활성 상태에서도 KeyboardInput과 physical_key를 정상 emit하므로,
    /// popup 단축키 매칭에 IME 비활성화가 필요 없다. 따라서 Windows는 항상 IME를 허용한다.
    /// 무대 프레임 — 전체화면 무대가 켜져 있을 때 [`Gpu::render`] 대신 도는 경로.
    ///
    /// clear pass + 무대 egui 레이어만 그린다. 터미널 글리프 · egui-mesh surface ·
    /// attach mesh 합성 · host chrome(사이드바/탭바/상태바/popup/오버레이)은 이
    /// 프레임에 **아예 그려지지 않는다** — 무대가 뒤를 가리고 있으므로 redraw 할
    /// 이유가 없다는 것이 이 기능의 모델이다.
    ///
    /// 반대로 **반드시 유지**하는 것: 마지막의 `pending_screenshot` 캡처 +
    /// `present`. `ui.screenshot`(window) 은 무대가 제대로 그려졌는지 확인하는 유일한
    /// 자동 검증 수단이고, 이 구간을 건너뛰면 요청이 영구 대기한다.
    fn render_fullscreen_stage(
        &mut self,
        state: &mut AppState,
        engine: &mut crate::core::CoreState,
        window: &Window,
    ) -> Result<(), wgpu::SurfaceError> {
        // egui 입력은 무대 프레임에서도 계속 take 한다 — 안 그러면 이벤트가 쌓여
        // 무대를 나올 때 한꺼번에 밀려든다.
        let raw_input = self.egui_state.take_egui_input(window);
        let egui::FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output: _,
        } = self.egui_ctx.run(raw_input, |ctx| {
            crate::adapters::ui::draw_fullscreen_stage(ctx, state, engine);
        });
        self.apply_platform_output(window, state, platform_output);

        let paint_jobs = self.egui_ctx.tessellate(shapes, pixels_per_point);
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point,
        };

        stall_watchdog::set_phase(stall_watchdog::Phase::Acquire);
        let output = self.surface.get_current_texture()?;
        stall_watchdog::set_phase(stall_watchdog::Phase::Submit);
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.render_clear_pass(&view, state, engine);
        self.render_egui_pass(&view, &textures_delta, &paint_jobs, &screen_descriptor);

        if let Some(path) = self.pending_screenshot.take() {
            self.capture_frame_to_png(&output.texture, self.size.width, self.size.height, &path);
        }
        stall_watchdog::set_phase(stall_watchdog::Phase::Present);
        output.present();
        Ok(())
    }

    fn apply_platform_output(
        &mut self,
        window: &Window,
        state: &AppState,
        platform_output: egui::PlatformOutput,
    ) {
        self.egui_state.set_allow_ime(false);

        #[cfg(not(windows))]
        let ime_widget_focused = platform_output.ime.is_some();

        self.egui_state
            .handle_platform_output(window, platform_output);

        #[cfg(not(windows))]
        {
            // plugin egui-mesh popup 은 여기 넣지 않는다 — host egui 에는 대응 위젯이
            // 없어 `ime_widget_focused` 가 항상 false 라, 포함하면 popup 의 텍스트
            // 입력에 IME 를 못 쓰게 된다. 조합 문자가 터미널로 새는 것은 IME 라우팅
            // 게이트(`view::main::ime`)가 막는다.
            let disable_ime = state.popups.has_focused() && !ime_widget_focused;
            window.set_ime_allowed(!disable_ime);
        }
        #[cfg(windows)]
        window.set_ime_allowed(true);
    }

    /// GPU 리소스 카운트 스냅샷 — `system.gpu_stats` IPC 가 창 단위로 노출한다.
    /// 메모리 누수 soak 검증용 read-only 조회: egui-mesh target 맵 3종은 retain
    /// 방식으로 정리되므로(§4-3), close/reopen 반복 후에도 len 이 단조 증가하면
    /// 그것이 GPU 리소스 누수 신호다. 렌더 상태를 변경하지 않는다.
    pub(crate) fn resource_stats(&self) -> serde_json::Value {
        let (bg_draws, glyph_draws, total_draws) = self.renderer.draw_call_count();
        serde_json::json!({
            "egui_mesh_targets": self.egui_mesh_targets.len(),
            "egui_mesh_popup_targets": self.egui_mesh_popup_targets.len(),
            "egui_mesh_banner_targets": self.egui_mesh_banner_targets.len(),
            "attach_mesh_targets": self.attach_mesh_targets.len(),
            "atlas": {
                "eviction_count": self.renderer.atlas.eviction_count(),
                "active_pages": self.renderer.atlas.active_page_count(),
                "entry_count_sum": self.renderer.atlas.entry_count_sum(),
            },
            "draw_calls": { "bg": bg_draws, "glyph": glyph_draws, "total": total_draws },
            "active_surfaces": self.renderer.active_surface_count(),
        })
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
        let pane_rects: Vec<(u32, PhysicalRect)> =
            pane_layout.compute_rects(terminal_rect, self.scale_factor);
        let mut dividers: Vec<PhysicalRect> =
            pane_layout.collect_dividers(terminal_rect, self.scale_factor);

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
                    dividers.extend(
                        tab.layout()
                            .collect_dividers(content_rect, self.scale_factor),
                    );
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

    /// Get egui's actual pixels_per_point (what it uses for rendering).
    // 이유: 호출부가 debug_info.rs/debug_input.rs(개발자 로컬 디버그 전용) 뿐이라
    // release 빌드에서 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
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
    // 이유: 호출부가 debug_info.rs/debug_input.rs(개발자 로컬 디버그 전용) 뿐이라
    // release 빌드에서 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn egui_zoom_factor(&self) -> f32 {
        self.egui_ctx.zoom_factor()
    }

    /// Whether egui-winit currently allows IME on the window.
    // 이유: 호출부가 debug_info.rs(개발자 로컬 디버그 전용) 뿐이라 release 빌드에서
    // 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn egui_ime_allowed(&self) -> bool {
        self.egui_state.allow_ime()
    }

    /// Get the wgpu surface config dimensions.
    // 이유: 호출부가 debug_info.rs/debug_input.rs(개발자 로컬 디버그 전용) 뿐이라
    // release 빌드에서 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
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

#[cfg(test)]
mod tests {
    use super::clamp_surface_dims;

    const MAX: u32 = 8192;

    #[test]
    fn passes_through_valid_dims() {
        assert_eq!(clamp_surface_dims(1, 1, MAX), (1, 1));
        assert_eq!(clamp_surface_dims(MAX, MAX, MAX), (MAX, MAX));
        assert_eq!(clamp_surface_dims(1280, 720, MAX), (1280, 720));
    }

    #[test]
    fn clamps_upper_bound() {
        assert_eq!(clamp_surface_dims(MAX + 1, 720, MAX), (MAX, 720));
        // TD-7 crash 재현 치수: 1100x65535, max 8192 → 높이만 clamp.
        assert_eq!(clamp_surface_dims(1100, 65535, MAX), (1100, MAX));
        assert_eq!(clamp_surface_dims(65535, 65535, MAX), (MAX, MAX));
    }

    #[test]
    fn raises_zero_to_lower_bound() {
        // early-return 이 0 을 먼저 거르지만, 함수 자체의 하한도 방어적으로 보장한다.
        assert_eq!(clamp_surface_dims(0, 720, MAX), (1, 720));
        assert_eq!(clamp_surface_dims(1280, 0, MAX), (1280, 1));
    }
}
