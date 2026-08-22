use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::view::ui::View;
use crate::view::{ViewAction, ViewCtx};
use crate::{App, AppEvent};

impl ApplicationHandler<AppEvent> for App {
    #[allow(clippy::cognitive_complexity)] // complexity-exempt: winit AppEvent 최상위 dispatch — 20여개 variant 대부분이 이미 추출된 핸들러로 1~5줄 위임하는 평면 match, cfg 게이트까지 섞여 있어 더 쪼개면 "이벤트→핸들러" 매치 테이블의 가독성이 오히려 떨어짐
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        // 종료 상태 머신 진행 중 — 모든 AppEvent 를 버린다. 종료는 되돌릴 수 없고
        // (`begin_shutdown` 은 멱등), 이미 close cascade 로 surface 를 정리한 뒤라
        // 지금 들어오는 이벤트는 대상이 사라진 상태에서 실행될 뿐이다. 부팅 가드처럼
        // 미뤄뒀다 재생할 시점도 없다 — 다음은 프로세스 종료다.
        if self.shutdown.is_some() {
            return;
        }

        // 부팅 상태 머신 진행 중 — 종료 계열 외 AppEvent 는 부팅 완료 후 재생하도록
        // 지연한다. 구 코드에서 resumed() 가 블로킹하는 동안 winit 큐에 쌓이던 것과
        // 등가이며, 특히 TerminalOutput 을 지금 소비하면 대상 engine 이 아직 views
        // 밖(App.core_state)이라 waker dedup 게이트가 닫힌 채 wake 가 유실된다.
        if let Some(boot) = self.boot.as_mut()
            && !matches!(event, AppEvent::Shutdown | AppEvent::QuitRequested)
        {
            boot.pending_events.push(event);
            return;
        }
        match event {
            AppEvent::CreateWindow => {
                self.create_new_window(event_loop);
            }
            AppEvent::RunLuaScript { source, name } => {
                if let Some(engine) = self.lua_engine.as_ref() {
                    engine.run_script(&source, Some(&name));
                } else {
                    tracing::warn!(target: "tasty_lua", "RunLuaScript dropped — lua engine unavailable");
                }
            }
            AppEvent::OpenSettings => {
                self.open_settings_modal(event_loop);
            }
            AppEvent::OpenPlugins => {
                self.open_plugins_modal(event_loop);
            }
            AppEvent::TerminalOutput(surface_id) => self.handle_terminal_output(surface_id),
            AppEvent::IpcReady => {
                if self.process_ipc()
                    && let Some(w) = self.focused_window_mut()
                {
                    w.mark_dirty();
                }
            }
            AppEvent::StreamReady => {
                // 스트림 클라 inbound 를 분류해 attach 결선(단계 4). 렌더 상태와 무관해
                // dirty 처리 불필요. 끊긴 client lock 은 전 engine 에서 자동 free 환원.
                let outcome = self.stream_hub.pump_inbound(&self.stream_inbound_rx);
                self.apply_stream_outcome(outcome);
            }
            #[cfg(all(windows, feature = "gui"))]
            AppEvent::SystemResumed => {
                self.resume_health_pass();
            }
            AppEvent::EguiRepaint { window_id } => {
                // 요청한 윈도우 한 개만 dirty 처리. window_id 직접 조회 —
                // 과거엔 egui_ctx.viewport_id() 로 매칭했으나 모든 Context 가 root
                // viewport(=ROOT)만 써서 모달이 열리면 두 윈도우가 같은 id 로 충돌,
                // find() 가 첫 윈도우로만 repaint 를 라우팅해 다른 윈도우(보통 main)의
                // egui repaint 가 새어나가 렌더가 멈췄다.
                if let Some(w) = self.view.views.get_mut(&window_id) {
                    w.mark_dirty();
                }
                // 매칭 실패 (shell_setup gpu 등 view 에 등록되지 않은 윈도우가 callback 을 보낸 경우)
                // 는 silently drop — 본 핸들러는 등록된 view 만 책임진다.
            }
            AppEvent::Shutdown => {
                self.begin_shutdown(event_loop);
            }
            AppEvent::Minimize => self.handle_minimize(),
            AppEvent::QuitRequested => {
                self.handle_quit_requested(event_loop);
            }
            AppEvent::CloseWindow(id) => {
                // CSD titlebar close 버튼 — 네이티브 CloseRequested 와 동일 라우팅.
                self.request_close_window(id, event_loop);
            }
            // Windows / Linux: windows still exist while hidden to tray, so re-show
            // them. (macOS parks state on background and re-creates a window via
            // CreateWindow instead — the tray routes "Show Window" there.)
            #[cfg(any(windows, target_os = "linux"))]
            AppEvent::TrayShowWindow => {
                for w in self.view.views.values() {
                    w.base().winit.set_visible(true);
                    w.base().winit.set_minimized(false);
                    w.base().winit.focus_window();
                }
                tracing::info!(
                    "restored {} window(s) from system tray",
                    self.view.views.len()
                );
            }
            AppEvent::BusyPoll => {
                self.poll_busy_states();
                self.poll_global_hooks();
                self.poll_idle_timeout_hooks();
            }
            AppEvent::AttachPoll => {
                self.poll_attach_views();
            }
            // client mirror 실시간 갱신 — reader thread 가 원격 출력을 받을 때마다 깨운다.
            // 서버 readonly 의 3초 cadence(AttachPoll)와 달리 즉시 적용/repaint.
            AppEvent::AttachClientData => {
                self.apply_attach_client_output();
            }
            // 단계 7 — 자동 attach 워커가 SSH 터널 수립을 마쳤다(wake). 결과를 drain 해
            // mirror 를 띄운다(idle 상태에서도 즉시 반영).
            AppEvent::AutoAttachReady => {
                self.drain_auto_attach_results();
            }
            // (03) 스크린샷→클립보드 캡처 워커가 완료됐다(wake). 결과를 drain 해
            // 로컬 클립보드 기록 또는 mirror 세션 업로드를 적용한다.
            AppEvent::ScreenshotCaptureReady => {
                self.drain_screenshot_capture_results();
            }
            // (08) mirror 이미지 paste 업로드 워커가 완료됐다(wake). 결과를 drain 해
            // 원격 경로 삽입 또는 실패 toast 를 적용한다.
            AppEvent::ImageUploadReady => {
                self.drain_image_upload_results();
            }
            // (09) mirror 파일 전송 진행 이벤트(청크 전송)가 도착했다. 진행 채널을 drain 해
            // 진행 팝업 행(바이트/속도/determinate bar)을 갱신한다.
            AppEvent::TransferProgressTick => {
                self.drain_transfer_progress();
            }
            AppEvent::IdentifyDone {
                request_id,
                target,
                detector,
                origin_surface_id,
                ignore_size_limit,
            } => self.handle_identify_done(
                request_id,
                target,
                detector,
                origin_surface_id,
                ignore_size_limit,
            ),
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // 재진입 가드 — macOS 등은 resumed() 를 재호출할 수 있다. 부팅 상태 머신
        // 진행 중(boot Some)에는 views 가 아직 비어 있으므로 boot 조건이 없으면
        // 창이 중복 생성된다.
        if !self.view.views.is_empty() || self.shell_setup_gpu.is_some() || self.boot.is_some() {
            return;
        }

        #[cfg(target_os = "macos")]
        crate::macos_delegate::inject_delegate_methods();

        // 부팅 구간 계측 (T1~T7, target: "tasty::boot") — 흰 화면 시간 분해용 상시 계측.
        let boot_t0 = std::time::Instant::now();

        // 축A: hidden 생성 — 첫 로딩 프레임 present 후에야 표시해 OS 기본 배경(흰)
        // 프레임을 제거한다. 3 OS 공통 winit API (표시는 begin_boot / shell setup
        // 분기가 수행, 실패 시에도 fallback 으로 반드시 표시).
        let window = Self::boot_create_hidden_window(event_loop, boot_t0);

        let mut init_settings = Self::boot_load_normalized_settings();

        let gpu = self.try_init_boot_gpu(&window, &init_settings.appearance);

        let (window, gpu) = match self.enter_shell_setup_if_needed(&mut init_settings, window, gpu)
        {
            Some(pair) => pair,
            None => return,
        };

        window.set_ime_allowed(true);
        // 부팅 상태 머신 시작 — theme/db 초기화 + 첫 로딩 프레임 present 후 즉시
        // 반환한다. 엔진·plugin·layout 복원은 이후 프레임 스텝(drive_boot_frame)이
        // 진행하고, Ready 도달 시 finish_boot 가 MainView 로 합류한다.
        self.begin_boot(window, gpu, init_settings, boot_t0, true);

        #[cfg(windows)]
        crate::jump_list::setup_jump_list();

        // System tray / status item (best-effort, ADR-0001). Create once and keep
        // it alive across window create/destroy (macOS parks state and recreates
        // windows, so guard against re-creating the tray each time). `None` =
        // tray unavailable; the app degrades to taskbar/dock minimize.
        #[cfg(all(
            any(windows, target_os = "macos", target_os = "linux"),
            feature = "gui"
        ))]
        self.ensure_tray_icon_once();

        tracing::info!(
            target: "tasty::boot",
            ms = boot_t0.elapsed().as_secs_f64() * 1000.0,
            "resumed_total (T1~T2 + T2.5 + 첫 로딩 프레임 — 상태 머신 전개로 T2.6~T6 은 boot_total 로 이동)"
        );
        // T7 기준 시각(mark_resumed_done)은 부팅 상태 머신의 finish_boot(Ready)가
        // 기록한다 — 구 코드의 resumed() 말미와 등가 시점 (boot::trace 주석 참조).
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // Shell setup mode — handled by App directly
        if self.shell_setup_mode {
            self.handle_shell_setup_window_event(event_loop, event);
            return;
        }

        // 종료 상태 머신 진행 중 — 종료 화면을 그리는 창들의 이벤트를 여기서 전부
        // 소비한다 (RedrawRequested = 스텝 구동, 그 외 입력은 core 에 닿지 않게 가드).
        if self.shutdown.is_some() {
            self.handle_shutdown_window_event(event_loop, id, event);
            return;
        }

        // 부팅 상태 머신 진행 중 — 부팅 창의 이벤트를 여기서 전부 소비한다
        // (RedrawRequested = 스텝 구동, 그 외 입력은 core 에 닿지 않게 가드).
        if self.boot.is_some() {
            self.handle_boot_window_event(event_loop, event);
            return;
        }

        // Modal handling — 활성 모달을 대상으로 한 이벤트
        if let Some(modal_id) = self.view.active_modal_id
            && id == modal_id
        {
            self.handle_active_modal_window_event(event_loop, id, event);
            return;
        }

        // Normal mode — find the window by ID and delegate
        if let WindowEvent::CloseRequested = &event {
            self.request_close_window(id, event_loop);
            return;
        }

        // Track focused window on focus events
        if let WindowEvent::Focused(true) = &event {
            self.handle_window_focused(id);
        }

        // Trigger modal shake when clicking on a non-modal window while modal is active
        if self.view.is_modal_active() {
            let is_mouse_press = matches!(
                &event,
                WindowEvent::MouseInput {
                    state: winit::event::ElementState::Pressed,
                    ..
                }
            );
            if is_mouse_press {
                self.trigger_modal_shake();
            }
        }

        // Plugin shortcut interception (단계 F): focused surface가 plugin
        // RemoteSurface면 host action 매칭 전에 plugin command와 비교한다.
        // 매칭 시 plugin에 dispatch + 이벤트 소모 → window.handle_event로 흐르지 않음.
        let plugin_consumed = if let WindowEvent::KeyboardInput { event: ke, .. } = &event {
            self.try_plugin_shortcut(id, ke)
        } else {
            false
        };
        if plugin_consumed {
            return;
        }

        self.dispatch_window_event_to_view(event_loop, id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

        // 종료 상태 머신 구동 — 부팅 가드와 같은 이유로 steady-state 파이프라인보다
        // 먼저 가로챈다. 여기서 return 하므로 종료 중에는 `process_ipc()` 가 돌지
        // 않는다 — 외부에서 들어온 명령이 정리 중인 상태를 다시 건드리지 않는다.
        // 대신 `drive_shutdown_frame` 이 큐잉된 요청을 "host is shutting down" 으로
        // 회신한다(무시하면 클라이언트가 무한 대기한다).
        //
        // 가드는 `event_loop.exit()` **이후**에도 유지된다 — winit 이 exit 요청 뒤에
        // 이 콜백을 한 번 더 돌릴 수 있고, 그때 가드가 풀려 있으면 이미 정리가 끝난
        // 상태로 steady-state 파이프라인을 타게 된다.
        //
        // WaitUntil 재예약은 부팅과 동일한 워치독 (창이 RedrawRequested 를 못 받아도
        // 스텝이 진행되도록).
        if self.shutdown.is_some() {
            self.drive_shutdown_frame(event_loop);
            if self.shutdown_needs_frames() {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    std::time::Instant::now()
                        + crate::app::shutdown_machine::SHUTDOWN_FRAME_INTERVAL,
                ));
            }
            return;
        }

        // 부팅 상태 머신 구동 — 미완 동안 steady-state 파이프라인(plugin pump /
        // intent drain / IPC 등)은 태우지 않는다 (부팅 가드; IPC 서버는 어차피
        // finish_boot 에서 시작). WaitUntil 재예약은 hidden/표시 직후 창이
        // RedrawRequested 를 못 받는 플랫폼에서도 스텝 진행을 보장하는 워치독.
        if self.boot.is_some() {
            self.drive_boot_frame(event_loop);
            if self.boot.is_some() {
                event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    std::time::Instant::now() + crate::app::boot_machine::BOOT_FRAME_INTERVAL,
                ));
            }
            return;
        }

        // Lua 자동실행 재진입 가드 정산 — 프레임당 1회, 모든 fire 경로보다 먼저.
        // (완료는 두 checkpoint 를 지나야 반영 — cascade 이벤트가 완료보다 먼저
        // 큐잉되는 창을 닫기 위한 의도된 1 프레임 지연. autofire.rs 참조.)
        self.lua_autofire.checkpoint();

        if self.process_ipc()
            && let Some(w) = self.focused_window_mut()
        {
            w.mark_dirty();
        }

        // attach/detach 작업 J — IPC 가 쌓은 GUI attach 트리거 실행(원격 워크스페이스
        // mirror 재구성). process_ipc 직후라야 같은 frame 에 반영된다.
        self.dispatch_pending_gui_attach();

        // 사용자가 mirror 워크스페이스 자체를 닫았으면(context menu/단축키 close),
        // 남은 attach 세션을 정리해 원격에 Detach 통지 → 원격 점유 해제. 미정리 시
        // 소켓이 열린 채라 재연결 시 "사용 중"으로 잔류한다.
        self.detach_orphaned_mirror_sessions();

        // 2단계 — mirror 워크스페이스 구조 op forward 큐 drain(원격 전송). Core::apply 가
        // 이번 프레임에 쌓은 op 를 같은 프레임에 원격으로 보낸다.
        self.dispatch_pending_structural_forwards();

        // ADR-0045 — client-driven mirror geometry: redraw 의 로컬 레이아웃 스윕이
        // mirror pane 목표 grid 를 쌓은 큐를 drain 해 원격 PTY 로 forward 한다. 원격
        // reflow 결과는 기존 server→client Resize echo 로 mirror 에 반영된다.
        self.dispatch_pending_resize_forwards();

        // (04) 파일 피커 — popup wrapper 가 쌓은 원격 디렉토리 목록 forward 큐를
        // drain 해 원격에 전송한다. 응답은 reader thread 가 `MirrorEvent::ListDirResult`
        // 로 비동기 수신(아래 apply_attach_client_output 경로).
        self.dispatch_pending_list_dir_forwards();
        // git-viewer(원격, `docs/adr/0056-git-viewer-remote-attach-git-query-channel.md`)
        // — `git_viewer.query` IPC 핸들러가 쌓은 원격 git 조회 forward 큐를 drain 해
        // 원격에 전송한다. 응답은 reader thread 가 `MirrorEvent::GitQueryResult` 로
        // 비동기 수신(아래 apply_attach_client_output 경로) → `emit_host_event_to_plugin`
        // 으로 plugin 에 push.
        self.dispatch_pending_git_query_forwards();
        // attach mesh mirror surface 의 텍스처 delta 체인 단절을 GPU 렌더 prepare 가
        // 감지해 쌓은 큐를 drain 해 원격에 full 재전송을 요청한다(상세
        // `docs/dev-guide/egui-mesh-channel.md` "텍스처 상태 수명 + delta 체인").
        self.dispatch_pending_mesh_full_resend_forwards();

        // attach mesh mirror pane 의 geometry/theme/focus 변경 + 누적 입력을 drain 해
        // 원격에 forward한다(구독 유지 + 인터랙티브 입력, 상세
        // `docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-소비-경로`).
        self.dispatch_pending_mesh_context_forwards();
        self.dispatch_pending_mesh_input_forwards();

        // attach/detach 단계 7 — 매핑된 워크스페이스 자동 attach. 활성 ws 가 매핑 Some &
        // 미attach 면 SSH 터널 워커를 spawn(무블록)하고, 완료된 결과를 drain 해 mirror
        // 를 띄운다(원격 워크스페이스 = 로컬 워크스페이스 매핑의 종착점).
        self.poll_auto_attach();

        // (03) 스크린샷→클립보드: 신규 키바인딩이 쌓은 트리거 큐를 drain 해 캡처
        // 워커를 spawn 하고, 완료된 결과를 로컬 클립보드/mirror 세션에 적용한다.
        self.poll_screenshot_captures();

        // (08) mirror 이미지 paste: 트리거 큐를 drain 해 백그라운드 bulk 업로드를 spawn
        // 하고, 완료된 결과(원격 경로 삽입/실패 toast)를 적용한다.
        self.poll_image_uploads();

        // Plugin host pump — process plugin events, run health checks, restart unresponsive.
        let hello_pairs = if let Some(ref mut mgr) = self.plugin_manager {
            mgr.pump()
        } else {
            Vec::new()
        };
        // hello 직후 surface_kind 등록 + PluginLoaded / PluginSurfaceKindRegistered
        // CoreEvent 발화. 큐 우회 sync 호출 (cascade 즉시).
        self.finalize_plugin_hello(hello_pairs);
        self.record_plugin_rss_samples_if_present();
        self.forward_mesh_frames_for_parked();
        self.mark_invalidated_surfaces_dirty();
        self.mark_invalidated_popups_dirty();
        // plugin이 보낸 IPC 호출들을 라우터로 디스패치 (권한 게이트 적용).
        self.process_plugin_ipc_calls();
        // surface close lifecycle 알림 drain → 구독 plugin에 broadcast.
        self.dispatch_pending_surface_lifecycle();
        // Event Bus 1.0 호스트 자동 발화 큐 drain (focus 변화 감지 포함).
        self.dispatch_pending_host_events();
        // tasty-memory regular 변경 → memory.changed host event.
        self.dispatch_pending_memory_changes();
        // 도구 메뉴 클릭으로 enqueue된 이벤트 publish.
        self.dispatch_pending_tool_events();
        // Command palette에서 plugin 전역 command 실행으로 enqueue된 큐 drain.
        self.dispatch_pending_palette_plugin_commands();
        // 호스트 내부 Intent 큐 drain — UI Intent 와 Domain Intent (Intent::Domain
        // wrapper) 모두 매 frame 일관 처리 (intent-ui-vs-domain.md §4.4).
        // dispatch_pending_intents 가 domain_batch 를 따로 모아 cascade 까지 일괄.
        self.dispatch_pending_intents();
        // Lua 워커에 최신 읽기전용 트리 스냅샷 발행 (ADR-0031 읽기 = 스냅샷).
        self.publish_lua_snapshot();
        // Lua 워커가 발행한 HostCommand drain·적용 (ADR-0031 쓰기 = 커맨드 큐).
        self.dispatch_pending_lua_commands();
        // 도구 메뉴 ToolAction::OpenPopup 클릭으로 enqueue된 popup open dispatch.
        self.dispatch_pending_popup_opens();
        // 파일 핸들러 IPC action 큐 drain (Phase C1: warn 로그만, Phase C3: 본격 dispatch).
        self.dispatch_pending_handler_ipc();
        // 파일 handler picker popup 의 result 슬롯 drain (D.3.C.G.3.c).
        self.dispatch_pending_picker_results();
        // Native file picker(04) popup 의 result 슬롯 drain — 로컬은 DispatchFile,
        // 원격은 클립보드 복사 + toast.
        self.dispatch_pending_file_picker_results();
        // Lua 스크립트 TOFU 변경 확인 팝업의 결정 슬롯 drain (ADR-0031).
        self.dispatch_pending_script_confirm();
        // 직전 프레임 plugin popup 렌더로 수집된 사용자 입력 / close 사유 forward.
        self.dispatch_plugin_popup_events();
        // PluginsView 모달의 사용자 액션을 manager에 적용 + 모달 snapshot 갱신.
        self.process_plugins_window_actions();
        // PresetView 열기 + (Intent::SavePreset cascade 시) 선택. preset 저장/적용/삭제/이름변경
        // 자체는 Intent 큐 (`dispatch_pending_intents`) 가 처리한다.
        self.process_pending_open_preset_window(event_loop);

        self.poll_tray_menu_events();

        // 열려 있는 네이티브 컨텍스트 메뉴를 펌프한다(비블로킹). redraw 경로와
        // 이중이지만, 메뉴가 떠 있는 동안은 아래 WaitUntil 이 이 경로를 8ms 주기로
        // 확실히 굴려 준다 — redraw 이벤트가 안 오는 순간에도 폴링이 이어진다.
        self.poll_pending_native_menus();

        // 아직 안 닫힌 메뉴가 남았으면 다음 폴링 tick 을 예약한다. 이걸 빠뜨리면
        // 메뉴가 열린 상태에서 아무 이벤트도 안 오는 순간 폴링 자체가 멈춰
        // 메뉴가 화면에서 얼어붙는다.
        if let Some(flow) =
            pending_menu_control_flow(self.any_pending_native_menu(), std::time::Instant::now())
        {
            event_loop.set_control_flow(flow);
        }

        // Tick modal shake animation.
        self.tick_modal_shake();

        // Flush layout persistence (debounced).
        self.flush_layout_persistence(false);

        self.flush_pending_pty_resizes();
    }
}

impl App {
    /// `resumed()` 축A: hidden 상태로 첫 winit window 를 만든다. 표시는 begin_boot /
    /// shell setup 분기가 담당(첫 로딩 프레임 present 후에야 표시해 OS 기본 배경(흰)
    /// 프레임을 제거한다).
    fn boot_create_hidden_window(
        event_loop: &ActiveEventLoop,
        boot_t0: std::time::Instant,
    ) -> std::sync::Arc<winit::window::Window> {
        use winit::window::WindowAttributes;
        let mut attrs = WindowAttributes::default()
            .with_visible(false)
            .with_title(if cfg!(debug_assertions) {
                "Tasty (Debug)"
            } else {
                "Tasty"
            })
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640, 480));
        if let Some(icon) = crate::app_icon::winit_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }
        // CSD: macOS 는 fullsize-content-view(네이티브 신호등 유지). 그 외 OS no-op.
        attrs = crate::platform::window_chrome::apply_csd_attributes(attrs);
        let window = std::sync::Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        tracing::info!(
            target: "tasty::boot",
            ms = boot_t0.elapsed().as_secs_f64() * 1000.0,
            "T1 window_create (resumed enter -> create_window return)"
        );
        window
    }

    /// `resumed()` 의 설정 로드+정규화 단계. enum-like 필드를 GPU init 전에 정규화해
    /// 다운스트림이 정규화된 값을 보게 하고, normalize 결과가 바뀌었으면 즉시
    /// 디스크에 반영해 다음 부팅에 같은 popup/잘못된 동작이 재발하지 않게 한다.
    fn boot_load_normalized_settings() -> crate::settings::Settings {
        let mut settings = crate::settings::Settings::load();
        let normalize_report = settings.normalize();
        if normalize_report.changed
            && let Err(e) = settings.save()
        {
            tracing::warn!("failed to persist normalized settings: {e}");
        }
        settings
    }

    /// `resumed()` 의 GPU 초기화. 어댑터가 아예 없으면(드라이버 미설치 등 정상적으로
    /// 발생 가능한 환경 문제) panic(크래시 리포트 대상) 대신 사람이 읽을 안내를
    /// stderr 로 내고 조용히 종료한다. 그 외 에러는 예상 밖 실패이므로 panic 시켜
    /// 크래시 리포팅 경로를 유지한다.
    fn try_init_boot_gpu(
        &mut self,
        window: &std::sync::Arc<winit::window::Window>,
        appearance: &crate::settings::AppearanceSettings,
    ) -> crate::gpu::GpuState {
        let t2 = std::time::Instant::now();
        let gpu = match self.create_gpu_state(window.clone(), appearance) {
            Ok(gpu) => gpu,
            Err(e) if e.downcast_ref::<crate::app::NoGpuAdapter>().is_some() => {
                eprintln!("{}", crate::i18n::t("boot.gpu_error.title"));
                eprintln!("{}", crate::i18n::t("boot.gpu_error.body"));
                eprintln!("{}", crate::i18n::t("boot.gpu_error.hint"));
                std::process::exit(1);
            }
            Err(e) => panic!("failed to initialize GPU: {e}"),
        };
        tracing::info!(
            target: "tasty::boot",
            ms = t2.elapsed().as_secs_f64() * 1000.0,
            "T2 gpu_init (create_gpu_state)"
        );
        gpu
    }

    /// `resumed()` 의 shell-setup 진입 판정. 설정된 shell 이 유효하면(또는 bash
    /// auto-detect 성공 시) `Some((window, gpu))` 로 그대로 돌려줘 caller 가 부팅을
    /// 계속 진행하게 한다. bash 조차 없으면 shell-setup 모드로 진입하고 (window, gpu)
    /// 소유권을 `self` 로 넘긴 뒤 `None` — caller 는 즉시 `resumed()` 를 return 해야 한다.
    fn enter_shell_setup_if_needed(
        &mut self,
        settings: &mut crate::settings::Settings,
        window: std::sync::Arc<winit::window::Window>,
        gpu: crate::gpu::GpuState,
    ) -> Option<(std::sync::Arc<winit::window::Window>, crate::gpu::GpuState)> {
        if settings.general.is_shell_valid() {
            return Some((window, gpu));
        }
        if let Some(detected) = crate::settings::GeneralSettings::detect_bash() {
            tracing::info!("configured shell invalid; auto-detected bash at {detected}");
            settings.general.shell = detected;
            if let Err(e) = settings.save() {
                tracing::warn!("failed to save auto-detected shell: {e}");
            }
            return Some((window, gpu));
        }
        self.enter_shell_setup_mode(window, gpu);
        None
    }

    /// `enter_shell_setup_if_needed` 의 실제 진입부 — bash 조차 없을 때 shell-setup
    /// 모드로 진입한다: setup 화면 첫 프레임을 그리고(렌더 실패 시에도 표시 — 창이
    /// 영영 hidden 이 되지 않게 하는 fallback) window/GPU 소유권을 `self` 로 넘긴다.
    fn enter_shell_setup_mode(
        &mut self,
        window: std::sync::Arc<winit::window::Window>,
        mut gpu: crate::gpu::GpuState,
    ) {
        tracing::warn!("bash not found; entering shell setup mode");
        self.shell_setup_mode = true;
        self.shell_setup_path = String::new();
        // Ok(action) 은 버린다 — 사용자 입력 전 첫 프레임이라 항상 None.
        if let Err(e) = gpu.render_shell_setup(&window, &mut self.shell_setup_path) {
            tracing::warn!("shell setup first frame render failed: {e} — showing window anyway");
        }
        window.set_visible(true);
        self.shell_setup_gpu = Some(gpu);
        self.shell_setup_window = Some(window);
    }

    /// `resumed()` 의 tray 아이콘 최초 생성(best-effort, ADR-0001). macOS 는 state 를
    /// park 하고 window 를 재생성하므로, 재생성 때마다 tray 가 다시 만들어지지 않게
    /// 가드한다. `None` = tray 불가 — 앱은 taskbar/dock minimize 로 degrade.
    #[cfg(all(
        any(windows, target_os = "macos", target_os = "linux"),
        feature = "gui"
    ))]
    fn ensure_tray_icon_once(&mut self) {
        if self.tray_icon.is_none()
            && let Some((tray, ids)) = crate::system_tray::create_tray_icon()
        {
            self.tray_icon = Some(tray);
            self.tray_menu_ids = Some(ids);
        }
    }

    /// `about_to_wait()` 지원 — RssSurge 이상탐지(상세 `docs/features/telemetry/index.md`):
    /// PluginManager 가 sysinfo 로 직접 sampling 한 (plugin_id, rss_bytes) 를 anomaly
    /// detector 에 공급. 어느 window 소관인지 몰라(PluginManager 는 App-level
    /// singleton) plugin lifecycle cascade 와 동일하게 첫 main window 를 대상으로
    /// 삼는다.
    fn record_plugin_rss_samples_if_present(&mut self) {
        if let Some(mgr) = self.plugin_manager.as_mut() {
            let rss_samples = mgr.take_rss_samples();
            if !rss_samples.is_empty()
                && let Some(main) = self.view.views.values_mut().find_map(|w| w.as_main_mut())
            {
                crate::adapters::ipc::handler::record_plugin_rss_samples(
                    &self.core,
                    &mut main.state,
                    &mut main.core_state,
                    &rss_samples,
                );
            }
        }
    }

    /// `about_to_wait()` 지원 — parked engine(macOS 최소화로 window 가 파괴되고
    /// CoreState 만 남은 경우, `handle_minimize` macOS 분기)의 mesh mirror 구독도
    /// 실제 frame relay 대상이 되도록 headless 와 동일한 구동 로직을 적용한다 —
    /// 살아있는 window 는 `MainView::handle_redraw` 가 이미 매 프레임 이 surface 들을
    /// 구동하므로 여기 대상이 아니다. owning-engine 순회 패턴
    /// (`apply_mesh_context_on_owning_engine` 등)과 동형이나, 이 스텝은 그 쪽처럼 첫
    /// 매치에서 멈추지 않고 `parked_states` 전부를 순회한다 — 여러 window 가 동시에
    /// 최소화돼 있어도 각 engine 이 독립적으로 계속 forward 된다(첫 번째만 복원돼도
    /// 나머지는 계속 이 스텝의 대상으로 남는다, `window_lifecycle.rs`의 `remove(0)` 참조).
    fn forward_mesh_frames_for_parked(&mut self) {
        if let Some(ref mgr) = self.plugin_manager {
            for (_, engine) in self.parked_states.iter_mut() {
                crate::plugin_bridge::mesh_forward::forward_mesh_frames_for_engine(
                    engine,
                    mgr,
                    &self.stream_hub,
                );
            }
        }
    }

    /// `about_to_wait()` 지원 — SurfaceInvalidated(단계 06): plugin 이 idle 상태(입력
    /// 무)에서 파일 변경을 알리면 그 surface 를 dirty 표시해 다음 redraw 에서 무입력
    /// 재-forward → 기존 poll_reload 가 새 내용을 읽게 한다. paint 에 종속된
    /// egui_mesh.rs 게이트의 유일한 예외 진입점. 어느 window 소관인지 몰라 전 View 를
    /// 순회한다.
    fn mark_invalidated_surfaces_dirty(&mut self) {
        let invalidated_surfaces = self
            .plugin_manager
            .as_mut()
            .map(|mgr| mgr.take_invalidated_surfaces())
            .unwrap_or_default();
        if invalidated_surfaces.is_empty() {
            return;
        }
        for w in self.view.views.values_mut() {
            let touched = match w.as_main_mut() {
                Some(main) => {
                    // `any()`는 첫 true 에서 멈춰 나머지 surface_id 를 못 마크한다 —
                    // 전부 순회해야 하므로 명시 루프.
                    let mut any_touched = false;
                    for &sid in &invalidated_surfaces {
                        if main.mark_surface_invalidated(sid) {
                            any_touched = true;
                        }
                    }
                    any_touched
                }
                None => false,
            };
            if touched {
                w.mark_dirty();
            }
        }
    }

    /// `about_to_wait()` 지원 — `PopupInvalidated`(TODO 15): egui-mesh popup
    /// (git-viewer/clipboard-viewer 등) plugin 이 egui `viewport_output` self-repaint
    /// 를 요청(예: 스크롤 스무딩이 유휴 상태에서 아직 안 끝남)하면, 다음 프레임에
    /// 무입력으로 재-forward 되도록 예약한다. [`mark_invalidated_surfaces_dirty`] 의
    /// popup 대응이지만, popup 의 forward 게이팅(`popup_render.rs`)은 이미 이 목적의
    /// `AppState::plugin_mesh_popup_pending_repaint`(ADR-0056, 비동기 host→plugin
    /// push 후 강제 repaint)를 갖고 있어 그대로 재사용한다 — 어느 window 가 이
    /// popup 을 그리는지 몰라(popup instance 는 window 소유권을 안 나름) 전 main
    /// window 에 broadcast 한다(`attach_client.rs` 의 기존 pending_repaint 예약
    /// 패턴과 동형).
    fn mark_invalidated_popups_dirty(&mut self) {
        let invalidated_popups = self
            .plugin_manager
            .as_mut()
            .map(|mgr| mgr.take_invalidated_popups())
            .unwrap_or_default();
        if invalidated_popups.is_empty() {
            return;
        }
        for main in self.main_windows_iter_mut() {
            for &iid in &invalidated_popups {
                main.state.plugin_mesh_popup_pending_repaint.insert(iid);
            }
            main.mark_dirty();
        }
    }

    /// `about_to_wait()` 지원 — tray 메뉴 이벤트 폴링(Windows/macOS/Linux) + Linux GTK
    /// pump. tasty 는 전용 GTK 메인루프가 없어 winit 루프에서 직접 구동해야 Linux
    /// tray(AppIndicator)가 메뉴 클릭을 dispatch 할 수 있다 — tray 미생성/비-Linux 면
    /// no-op.
    fn poll_tray_menu_events(&mut self) {
        // Pump GTK so the Linux tray (AppIndicator) can dispatch its menu clicks —
        // tasty has no dedicated GTK main loop, so we drive it from the winit loop.
        // 열려 있는 네이티브 컨텍스트 메뉴(GTK 팝업)도 같은 GTK main context 를
        // 쓰므로, tray 가 없더라도 메뉴가 떠 있으면 함께 펌프한다 — redraw 의
        // `poll_pending_native_menu` 와 이중이지만, redraw 가 뜸한 순간에도
        // 메뉴가 계속 반응하게 만드는 쪽이라 무해하다(둘 다 non-blocking).
        // No-op when no tray was created and no menu is open / off Linux.
        #[cfg(target_os = "linux")]
        if self.tray_icon.is_some() || self.any_pending_native_menu() {
            crate::system_tray::pump_gtk_events();
        }

        // Poll system tray menu events (Windows / macOS / Linux).
        #[cfg(all(
            any(windows, target_os = "macos", target_os = "linux"),
            feature = "gui"
        ))]
        if let Some(ref ids) = self.tray_menu_ids
            && let Some(menu_id) = crate::system_tray::poll_menu_event()
        {
            if menu_id == ids.show_window {
                // "Show Window" 는 *창을 보이게 한다* 이지 *새로 만든다* 가 아니다.
                // 살아있는 main view 가 하나라도 있으면 그 창을 맨 앞으로 가져와
                // focus 하고, 하나도 없을 때(전부 parked = macOS 최소화로 창이 파괴된
                // 상태)만 새 창을 생성/복원한다. Windows / Linux 는 창을 죽이지 않고
                // hide 하므로 TrayShowWindow 로 재표시한다.
                #[cfg(target_os = "macos")]
                {
                    // focused_view_id 가 가리키는 main view 우선, 없으면 첫 main view.
                    let target = self
                        .view
                        .focused_view_id
                        .filter(|id| {
                            self.view
                                .views
                                .get(id)
                                .is_some_and(|w| w.as_main().is_some())
                        })
                        .or_else(|| {
                            self.view
                                .views
                                .iter()
                                .find(|(_, w)| w.as_main().is_some())
                                .map(|(id, _)| *id)
                        });
                    if let Some(id) = target {
                        if let Some(w) = self.view.views.get(&id) {
                            w.base().winit.set_minimized(false);
                            w.base().winit.focus_window();
                        }
                        self.view.focused_view_id = Some(id);
                        tracing::info!("tray show: focusing existing main window");
                    } else {
                        tracing::info!("tray show: no live window, creating");
                        crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::CreateWindow);
                    }
                }
                #[cfg(any(windows, target_os = "linux"))]
                crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::TrayShowWindow);
            } else if menu_id == ids.new_window {
                crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::CreateWindow);
            } else if menu_id == ids.quit {
                crate::shortcuts::send_app_event(&self.view.proxy, AppEvent::Shutdown);
            }
        }
    }

    /// `about_to_wait()` 지원 — Flush deferred PTY resizes (throttled to 100ms
    /// intervals). 아직 pending 인 terminal 이 있으면 다음 프레임에 재시도하도록
    /// redraw 를 요청한다.
    fn flush_pending_pty_resizes(&mut self) {
        let mut any_pending = false;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && crate::core::Core::flush_pty_resizes(&mut main.core_state)
            {
                any_pending = true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if crate::core::Core::flush_pty_resizes(engine) {
                any_pending = true;
            }
        }
        if any_pending {
            for w in self.view.views.values() {
                w.base().winit.request_redraw();
            }
        }
    }

    /// dedup 게이트 early reset — Some → 해당 surface 게이트, None → 글로벌 게이트.
    /// surface 가 없는 factory 의 `note_drained` 는 무해한 no-op 이므로 전 view/parked
    /// 를 순회한다. `handle_terminal_output` 의 채널 drain 직전 사전 패스.
    fn note_drained_all(&self, surface_id: Option<u32>) {
        for w in self.view.views.values() {
            if let Some(main) = w.as_main()
                && let Some(factory) = main.core_state.waker_factory.as_ref()
            {
                factory.note_drained(surface_id);
            }
        }
        for (_, engine) in self.parked_states.iter() {
            if let Some(factory) = engine.waker_factory.as_ref() {
                factory.note_drained(surface_id);
            }
        }
    }

    /// `AppEvent::TerminalOutput` 핸들러 — reader thread 의 wake 를 받아 대상 surface
    /// 의 PTY 출력을 drain·cascade 한다. Some(sid) 는 targeted polling, None 은 전
    /// engine fallback. dedup 게이트 early reset 후 processing 순서는 통신 semantics
    /// 에 영향하므로 원본 순서를 보존한다.
    fn handle_terminal_output(&mut self, surface_id: Option<u32>) {
        use crate::app::dispatch_domain::DispatchSource;
        use crate::core::intent::CoreEvent;
        // Early reset: 채널 drain 직전에 dedup 게이트를 풀어, drain 과 경합하는
        // reader wake 가 스킵되어 유실되는 것을 막는다 (research §8).
        self.note_drained_all(surface_id);
        let core = &mut self.core;
        let views = &mut self.view.views;
        let parked_states = &mut self.parked_states;
        let mut pending: Vec<(DispatchSource, Vec<CoreEvent>)> = Vec::new();
        if let Some(sid) = surface_id {
            // Targeted polling: 모든 view 의 engine 을 순회하며 해당 surface 보유 시 process
            let mut found = false;
            for (wid, w) in views.iter_mut() {
                let Some(main) = w.as_main_mut() else {
                    continue;
                };
                if main.core_state.find_terminal_by_id(sid).is_some() {
                    let outcome = core.process_pty_output(&mut main.core_state, sid);
                    if !outcome.events.is_empty() {
                        pending.push((DispatchSource::Main(*wid), outcome.events));
                    }
                    main.recalc_ime_preedit_anchor();
                    // P3 visibility gate: 안 보이는 surface 의 출력은 보이는
                    // 창의 콘텐츠를 바꾸지 않으므로 redraw 요청을 생략한다.
                    // 데이터 drain(process_pty_output)·이벤트 cascade 는 위에서
                    // 이미 수행됐다. 해당 surface 가 보이게 전환되는 경로(탭/
                    // 워크스페이스 전환·split·복원)는 각자 dirty 를 설정하므로
                    // 전환 시 최신 grid 가 렌더된다.
                    if main.is_surface_visible(sid) {
                        main.mark_dirty();
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                for (idx, (_, engine)) in parked_states.iter_mut().enumerate() {
                    if engine.find_terminal_by_id(sid).is_some() {
                        let outcome = core.process_pty_output(engine, sid);
                        if !outcome.events.is_empty() {
                            pending.push((DispatchSource::Parked(idx), outcome.events));
                        }
                        break;
                    }
                }
            }
        } else {
            // Fallback: wake all views and process all terminals across engines
            for (wid, w) in views.iter_mut() {
                if let Some(main) = w.as_main_mut() {
                    let outcome = core.process_all_pty_output(&mut main.core_state);
                    if !outcome.events.is_empty() {
                        pending.push((DispatchSource::Main(*wid), outcome.events));
                    }
                }
                w.mark_dirty();
            }
            for (idx, (_, engine)) in parked_states.iter_mut().enumerate() {
                let outcome = core.process_all_pty_output(engine);
                if !outcome.events.is_empty() {
                    pending.push((DispatchSource::Parked(idx), outcome.events));
                }
            }
        }
        // borrow scope 종료 후 cascade dispatch.
        for (source, events) in pending {
            for ev in events {
                self.handle_core_event_system(source, ev);
            }
        }
    }

    /// `AppEvent::Minimize` 핸들러 — 플랫폼별 최소화 전략. macOS 는 창 파괴 후
    /// MainView 상태 파킹, Windows/Linux 는 트레이 유무에 따라 hide 또는 taskbar
    /// 최소화(창 유지).
    fn handle_minimize(&mut self) {
        #[cfg(target_os = "macos")]
        {
            // macOS: destroy windows, park all MainView states (dock reopen restores).
            // 모달은 파킹 대상이 아니므로 그냥 drop.
            let drained: Vec<_> = self.view.views.drain().map(|(_, w)| w).collect();
            for w in drained {
                if let Some(main_box) = crate::view::unbox_main(w) {
                    self.parked_states
                        .push((main_box.state, main_box.core_state));
                }
            }
            self.view.focused_view_id = None;
            self.view.active_modal_id = None;
            tracing::info!(
                "minimized to background ({} states parked)",
                self.parked_states.len()
            );
        }
        #[cfg(windows)]
        {
            if self.tray_icon.is_some() {
                // Windows with tray: hide windows to tray (keep alive)
                for w in self.view.views.values() {
                    w.base().winit.set_visible(false);
                }
                tracing::info!("hid {} window(s) to system tray", self.view.views.len());
            } else {
                // Windows without tray: minimize to taskbar
                for w in self.view.views.values() {
                    w.base().winit.set_minimized(true);
                }
                tracing::info!("minimized {} window(s) to taskbar", self.view.views.len());
            }
        }
        #[cfg(target_os = "linux")]
        {
            if self.tray_icon.is_some() {
                // Linux with tray: hide windows to tray (keep alive)
                for w in self.view.views.values() {
                    w.base().winit.set_visible(false);
                }
                tracing::info!("hid {} window(s) to system tray", self.view.views.len());
            } else {
                // Linux without tray: minimize windows to taskbar (keep alive)
                for w in self.view.views.values() {
                    w.base().winit.set_minimized(true);
                }
                tracing::info!("minimized {} window(s) to taskbar", self.view.views.len());
            }
        }
    }

    /// `AppEvent::IdentifyDone` 핸들러 — 비동기 파일 식별 결과를 focused MainView 에
    /// 적용한다(`apply_identify_result`). split borrow 회피를 위해 focused view 를
    /// 인덱스로 직접 접근.
    fn handle_identify_done(
        &mut self,
        request_id: crate::identify_worker::IdentifyRequestId,
        target: crate::file::format::FileTarget,
        detector: Option<crate::file::format::DetectorId>,
        origin_surface_id: Option<u32>,
        ignore_size_limit: bool,
    ) {
        tracing::debug!(
            request_id = %request_id,
            target = %target.display(),
            detector = ?detector.as_ref().map(|d| d.as_str()),
            origin_surface_id = ?origin_surface_id,
            ignore_size_limit,
            "IdentifyDone",
        );
        // Split borrow — focused_window_mut 는 &mut self 전체를 잡아
        // self.core 와 충돌하므로 인덱스로 직접 접근.
        if let Some(id) = self.view.focused_view_id
            && let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut())
        {
            self.core.apply_identify_result(
                &mut main.state,
                &mut main.core_state,
                target,
                detector,
                origin_surface_id,
                ignore_size_limit,
            );
        }
    }

    /// shell setup mode 의 `WindowEvent` 핸들러. RedrawRequested 시 setup 화면을
    /// 렌더하고 사용자 확정/종료를 처리하며, 그 외 이벤트는 egui 로 forward 한다.
    /// 항상 이벤트를 소비한다(caller 는 호출 후 즉시 return).
    /// `handle_shell_setup_window_event` 의 `ShellSetupAction::Confirmed` 처리 —
    /// 사용자가 shell setup 화면에서 확정한 경로를 설정에 반영·저장하고, setup 창/GPU
    /// 를 `begin_boot` 로 넘겨 부팅 상태 머신을 시작한다(진입 경로 ②). 창은 이미
    /// setup 화면으로 보이는 상태라 축A(set_visible)는 스킵(window_hidden=false),
    /// phase 구동은 일반 경로와 동일. boot_t0 는 Confirmed 시각.
    fn finish_shell_setup_confirmed(&mut self) {
        let mut settings = crate::settings::Settings::load();
        let normalize_report = settings.normalize();
        settings.general.shell = self.shell_setup_path.clone();
        if let Err(e) = settings.save() {
            tracing::error!("failed to save settings: {e}");
        }
        self.shell_setup_mode = false;
        let window = self.shell_setup_window.take().unwrap();
        let gpu = self.shell_setup_gpu.take().unwrap();
        self.begin_boot(window, gpu, settings, std::time::Instant::now(), false);
        // invalid_theme_name 처리는 부팅 상태 머신 내부로 이동했으므로
        // normalize_report 는 여기서 별도 소비할 필요 없음.
        drop(normalize_report);
    }

    fn handle_shell_setup_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) {
        if let WindowEvent::RedrawRequested = &event {
            if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window)
            {
                let result = gpu.render_shell_setup(window, &mut self.shell_setup_path);
                match result {
                    Ok(crate::gpu::ShellSetupAction::None) => {}
                    Ok(crate::gpu::ShellSetupAction::Confirmed) => {
                        self.finish_shell_setup_confirmed();
                    }
                    Ok(crate::gpu::ShellSetupAction::Exit) => {
                        event_loop.exit();
                    }
                    Err(e) => {
                        let msg = format!("shell setup render error: {e}");
                        tracing::warn!("{}", msg);
                        crate::crash_report::record_error(&msg);
                    }
                }
            }
            if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window)
            {
                gpu.handle_egui_event(window, &event);
            }
            return;
        }
        if let (Some(gpu), Some(window)) = (&mut self.shell_setup_gpu, &self.shell_setup_window) {
            gpu.handle_egui_event(window, &event);
            if let WindowEvent::CloseRequested = &event {
                event_loop.exit();
            }
        }
    }

    /// 활성 모달을 대상으로 한 `WindowEvent` 를 모달 view 에 위임하고 그 `ViewAction`
    /// (Close / CloseWithEvent)을 처리한다. caller 는 호출 후 즉시 return.
    fn handle_active_modal_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let action = if let Some(modal) = self.view.views.get_mut(&id) {
            let mut ctx = ViewCtx {
                event_loop,
                modal_active: false,
                plugin_manager: self.plugin_manager.as_ref(),
                stream_hub: &self.stream_hub,
            };
            modal.handle_event(event, &mut ctx)
        } else {
            ViewAction::None
        };

        match action {
            ViewAction::None => {}
            ViewAction::Close => {
                self.close_active_modal();
            }
            ViewAction::CloseWithEvent(app_event) => {
                self.close_active_modal();
                crate::shortcuts::send_app_event(&self.view.proxy, app_event);
            }
        }
    }

    /// `WindowEvent::Focused(true)` 처리 — MainView 면 focused_view_id 갱신,
    /// `window.focused` host event 발화, 활성 모달이 있으면 앞으로 가져온다.
    fn handle_window_focused(&mut self, id: WindowId) {
        // 모달이 focus 이벤트를 받아도 focused_view_id는 MainView 전용
        let is_main = self
            .view
            .views
            .get(&id)
            .map(|w| w.as_main().is_some())
            .unwrap_or(false);
        if is_main {
            self.view.focused_view_id = Some(id);
        }
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::WindowFocused;
            let payload = WindowFocused {
                window_id: u64::from(id),
            };
            mgr.emit_host_event("window.focused", &payload, EventScope::System);
        }
        // If a modal is active, bring it to the front so it's not buried
        if let Some(modal_id) = self.view.active_modal_id
            && let Some(modal) = self.view.views.get(&modal_id)
        {
            modal.base().winit.focus_window();
        }
    }

    /// 일반 모드 `WindowEvent` 를 대상 view 에 위임하고 반환 `ViewAction` 및 후속
    /// close 요청(마지막 main window 파킹·포커스 이양)을 처리한다.
    fn dispatch_window_event_to_view(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let modal_active = self.view.is_modal_active();
        let action = {
            if let Some(w) = self.view.views.get_mut(&id) {
                let mut ctx = ViewCtx {
                    event_loop,
                    modal_active,
                    plugin_manager: self.plugin_manager.as_ref(),
                    stream_hub: &self.stream_hub,
                };
                // MainView.handle_event는 항상 ViewAction::None을 반환한다.
                // PresetView (modeless editor) 는 CloseRequested 에서 Close 를 반환하므로
                // 이 경로에서 처리한다. 그 외 modal Close 는 위쪽 모달 경로에서 소비된다.
                w.handle_event(event, &mut ctx)
            } else {
                ViewAction::None
            }
        };
        if self.view.views.contains_key(&id) {
            match action {
                ViewAction::None => {}
                ViewAction::Close => {
                    if self.preset_view_id == Some(id) {
                        self.on_preset_window_closed(id);
                        return;
                    }
                    debug_assert!(false, "non-modal window returned Close unexpectedly");
                }
                ViewAction::CloseWithEvent(app_event) => {
                    if self.preset_view_id == Some(id) {
                        self.on_preset_window_closed(id);
                        crate::shortcuts::send_app_event(&self.view.proxy, app_event);
                        return;
                    }
                    debug_assert!(
                        false,
                        "non-modal window returned CloseWithEvent unexpectedly"
                    );
                }
            }

            // Check if the window requested to close (e.g. last workspace removed)
            let close_requested = self
                .view
                .views
                .get(&id)
                .map(|w| w.base().close_requested)
                .unwrap_or(false);
            if close_requested {
                if let Some(w) = self.view.views.remove(&id)
                    && self.view.views.values().all(|w| w.as_main().is_none())
                    && let Some(main_box) = crate::view::unbox_main(w)
                {
                    tracing::info!("last main window closed via request, parking state");
                    self.parked_states
                        .push((main_box.state, main_box.core_state));
                }
                if self.view.focused_view_id == Some(id) {
                    self.view.focused_view_id = self
                        .view
                        .views
                        .iter()
                        .find(|(_, w)| w.as_main().is_some())
                        .map(|(id, _)| *id);
                }
            }
        }
    }

    /// OS 절전 복귀(`AppEvent::SystemResumed`, Windows) 헬스 패스 (ADR-0017).
    /// 전 view/parked engine 을 순회하며 (1) 살아있는 PTY 자식을 wake nudge,
    /// (2) `process_all_pty_output` 로 절전 중 죽은 자식의 `ProcessExited` cascade
    /// 를 즉시 트리거(→ surface 정리), (3) 자식 TUI 가 도는데 깨어나지 못할 수도
    /// 있는 surface 를 알림으로 가시화한다.
    #[cfg(all(windows, feature = "gui"))]
    pub(crate) fn resume_health_pass(&mut self) {
        use crate::app::dispatch_domain::DispatchSource;
        use crate::core::intent::CoreEvent;
        tracing::info!("system resumed — running PTY health pass (ADR-0017)");
        let core = &mut self.core;
        let mut pending: Vec<(DispatchSource, Vec<CoreEvent>)> = Vec::new();
        for (wid, w) in self.view.views.iter_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            let suspects = main.core_state.wake_terminals_after_resume();
            let outcome = core.process_all_pty_output(&mut main.core_state);
            if !outcome.events.is_empty() {
                pending.push((DispatchSource::Main(*wid), outcome.events));
            }
            Self::notify_resume_suspects(&mut main.core_state, &suspects);
            w.mark_dirty();
        }
        for (idx, (_, engine)) in self.parked_states.iter_mut().enumerate() {
            let suspects = engine.wake_terminals_after_resume();
            let outcome = core.process_all_pty_output(engine);
            if !outcome.events.is_empty() {
                pending.push((DispatchSource::Parked(idx), outcome.events));
            }
            Self::notify_resume_suspects(engine, &suspects);
        }
        for (source, events) in pending {
            for ev in events {
                self.handle_core_event_system(source, ev);
            }
        }
    }

    /// resume 헬스 패스가 찾은 "절전 복귀 후 응답하지 않을 수 있는" surface 들에 대해
    /// 알림을 발행한다 (각 surface 의 workspace 로 귀속, surface highlight 포함).
    #[cfg(all(windows, feature = "gui"))]
    fn notify_resume_suspects(engine: &mut crate::core::CoreState, suspects: &[u32]) {
        for &sid in suspects {
            let ws_id = engine
                .workspaces
                .iter()
                .find(|w| w.all_surface_ids().contains(&sid))
                .map(|w| w.id)
                .unwrap_or(0);
            let title = crate::i18n::t("resume.suspect.title").to_string();
            let body = crate::i18n::t("resume.suspect.body").to_string();
            if engine.notifications.add(ws_id, sid, title, body).is_some() {
                // toast producer — 신규 알림이면 surface attention 발동(producer
                // 중립 공유 상태로 이전됨, 옛 add() 내부 insert 대체).
                engine.raise_attention(sid, crate::core::AttentionKind::Completion);
            }
        }
    }

    /// 윈도우 닫기 요청을 라우팅한다 — 네이티브 `WindowEvent::CloseRequested` 와
    /// CSD titlebar close 버튼(`AppEvent::CloseWindow`)의 공통 경로.
    /// PresetView 는 즉시 닫고, MainView 가 여럿이면 해당 창만, 마지막 하나면 quit
    /// 흐름으로 라우팅한다.
    pub(crate) fn request_close_window(&mut self, id: WindowId, event_loop: &ActiveEventLoop) {
        // PresetView (modeless editor) — 바로 닫힌다. quit 흐름 거치지 않음.
        // 메인 윈도우 개수와 무관하게 자기 자신만 닫는 게 의도된 동작.
        if self.preset_view_id == Some(id) {
            self.on_preset_window_closed(id);
            return;
        }
        // MainView 개수 기준으로 판단 (모달은 수에 포함되지 않음)
        if self.main_window_count() > 1 {
            // Multiple windows: just close this one
            self.close_main_window(id, tasty_plugin_protocol::LifecycleReason::User);
        } else {
            // Last main window: route through quit logic
            self.handle_quit_requested(event_loop);
        }
    }

    /// 메인 윈도우 하나를 제거하고 종료 통지를 발화한다 — `window.closed` plugin
    /// event + `window.delete.post` Lua fire + (닫힌 창이 focused 였으면) 포커스
    /// 이양. GUI(`request_close_window`)와 IPC(`window.close`)가 공유하는 공통
    /// 경로이며, 닫은 주체는 `reason` 으로 구분한다 (User / Ipc). 마지막 main
    /// window 분기(quit 흐름)는 포함하지 않는다 — 호출 전에 caller 가 판단한다.
    pub(crate) fn close_main_window(
        &mut self,
        id: WindowId,
        reason: tasty_plugin_protocol::LifecycleReason,
    ) {
        self.view.views.remove(&id);
        let scripts = self.autofire_scripts();
        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::WindowClosed;
            let payload = WindowClosed {
                window_id: u64::from(id),
                reason,
            };
            mgr.emit_host_event("window.closed", &payload, EventScope::System);
            crate::hooks::lua::fire(
                self.lua_engine.as_ref(),
                crate::hooks::lua::AutofireCtx {
                    scripts: &scripts,
                    guard: &mut self.lua_autofire,
                },
                "window.delete.post",
                &payload,
            );
        }
        if self.view.focused_view_id == Some(id) {
            self.view.focused_view_id = self
                .view
                .views
                .iter()
                .find(|(_, w)| w.as_main().is_some())
                .map(|(id, _)| *id);
        }
    }

    /// stream client 들이 끊겼을 때 그들이 잡고 있던 attach lock 을 모든 engine
    /// (활성 main view + parked)에서 자동 해제한다(attach/detach 단계 3 EOF 해제).
    /// 한 client_id 는 한 engine 에만 lock 을 가지므로 전 engine 순회는 멱등·안전.
    pub(crate) fn release_attach_for_disconnected(&mut self, clients: &[u32]) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                for &cid in clients {
                    main.core_state.attach.release_all_for_client(cid);
                    // (06) bulk 연결 종료 시 커밋 안 된 대용량 partial 청소.
                    main.core_state.bulk_transfers.clear_client(cid);
                    // 캡처 업로드 연결 종료 시 커밋 안 된 partial 청소.
                    main.core_state.capture_uploads.clear_client(cid);
                    // mesh 구독 정리 — 불필요한 plugin CPU 낭비 방지.
                    main.core_state.mesh_mirror.remove_for_client(cid);
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            for &cid in clients {
                engine.attach.release_all_for_client(cid);
                engine.bulk_transfers.clear_client(cid);
                engine.capture_uploads.clear_client(cid);
                engine.mesh_mirror.remove_for_client(cid);
            }
        }
    }

    /// `pump_inbound` 가 분류한 stream inbound 를 적용한다(attach/detach 단계 4).
    /// gui 는 engine 이 여럿(활성 main view + parked)이라, 각 요청을 *대상 surface 를
    /// 소유한 engine* 에 라우팅한다. 끊김은 전 engine 해제(멱등).
    pub(crate) fn apply_stream_outcome(
        &mut self,
        outcome: crate::adapters::production::stream_hub::PumpOutcome,
    ) {
        // StreamHub 는 Arc clone(저렴) — 필드 동시 차용 회피용.
        let hub = self.stream_hub.clone();

        self.apply_attach_requests_batch(outcome.attach_requests, &hub);
        self.apply_workspace_attach_requests_batch(outcome.workspace_attach_requests, &hub);
        self.apply_input_frames_batch(outcome.input_frames);
        self.apply_structural_ops_batch(outcome.structural_ops, &hub);
        // client-driven mirror geometry(ADR-0045): mirror client 가 요청한 크기로
        // 원격 PTY 를 resize. holder 검증은 `apply_attached_workspace_resize` 가
        // 담당하며, 변화가 있으면 기존 resize tap 이 server→client `Resize` echo 를
        // 자동 fan-out 한다(여기서 추가 push 없음).
        self.apply_resize_requests_batch(outcome.resize_requests);
        self.apply_mesh_context_requests_batch(outcome.mesh_context_requests, &hub);
        self.apply_mesh_full_resend_requests_batch(outcome.mesh_full_resend_requests, &hub);
        self.apply_mesh_input_events_batch(outcome.mesh_input_events, &hub);

        // (03) screenshot→remote-clipboard: mirror client 가 attach 채널로 보낸
        // 캡처 업로드 청크/커밋. holder(그 client 가 점유한 워크스페이스를 가진
        // engine)를 찾아 누적/커밋한다.
        self.apply_capture_uploads_batch(outcome.capture_uploads, &hub);
        // (04) file picker: mirror client 가 attach 채널로 보낸 디렉토리 목록 조회
        // 요청. holder(그 client 가 점유한 워크스페이스를 가진 engine)를 찾아 처리.
        self.apply_list_dir_requests_batch(outcome.list_dir_requests, &hub);
        self.apply_git_query_requests_batch(outcome.git_query_requests, &hub);
        // (06) native bulk 파일 전송: begin/chunk/commit 을 **도착 순서 그대로**
        // (단일 벡터) 결속 workspace 를 소유한 engine 으로 라우팅한다. 순서 보존이라
        // chunk 가 begin 을 앞지르지 않는다(전량 폐기 + 빈 파일 성공 오보 방지). 결속
        // ws 는 연결-단위 bulk 태깅에서 조회.
        self.apply_bulk_events_batch(outcome.bulk_events, &hub);

        if !outcome.disconnected.is_empty() {
            self.release_attach_for_disconnected(&outcome.disconnected);
        }

        // 작업 J: attach/detach 직후 즉시 서버 readonly display mirror 를 채워(또는
        // 해제분 정리) 첫 3초 tick 전 blank 를 없앤다. 점유 mirror 있는 window 만 dirty.
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.refresh_readonly_views()
            {
                w.mark_dirty();
            }
        }
    }

    /// `apply_stream_outcome` 지원 — `attach_requests` 배치 적용.
    fn apply_attach_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32)>,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id) in requests {
            if !self.attach_on_owning_engine(surface_id, client_id, hub) {
                // 어떤 engine 도 이 surface 를 소유하지 않음 → 거부.
                crate::core::attach_runtime::reject_attach(hub, client_id, "not_found", None);
            }
        }
    }

    /// `apply_stream_outcome` 지원 — `workspace_attach_requests` 배치 적용.
    fn apply_workspace_attach_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32)>,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, workspace_id) in requests {
            if !self.attach_workspace_on_owning_engine(workspace_id, client_id, hub) {
                crate::core::attach_runtime::reject_attach(
                    hub,
                    client_id,
                    "workspace_not_found",
                    None,
                );
            }
        }
    }

    /// `apply_stream_outcome` 지원 — `input_frames` 배치 적용.
    fn apply_input_frames_batch(&mut self, frames: impl IntoIterator<Item = (u32, Vec<u8>)>) {
        for (client_id, bytes) in frames {
            let routed = self.feed_stream_input(client_id, &bytes);
            #[cfg(debug_assertions)]
            if !routed {
                // 단계 1 echo client(점유 surface 없음): debug 빌드 회신.
                let echo_frame = crate::ipc::stream::StreamFrame::new(
                    crate::ipc::stream::StreamTag::Data,
                    bytes,
                );
                let _ = self.stream_hub.push(client_id, echo_frame); // best-effort echo — PushResult(Result 아님) 무시: client 끊김 시 무해.
            }
            #[cfg(not(debug_assertions))]
            let _ = routed; // release: echo 분기 없어 routed 미사용 — 값 drop(Result 아님).
        }
    }

    /// `apply_stream_outcome` 지원 — `structural_ops` 배치 적용.
    fn apply_structural_ops_batch(
        &mut self,
        ops: impl IntoIterator<Item = (u32, u64, crate::ipc::stream::StructuralOp)>,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, op_id, op) in ops {
            self.apply_forwarded_structural_op(client_id, op_id, &op, hub);
        }
    }

    /// `apply_stream_outcome` 지원 — `resize_requests` 배치 적용.
    fn apply_resize_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32, usize, usize)>,
    ) {
        for (client_id, remote_surface_id, cols, rows) in requests {
            self.apply_forwarded_resize(client_id, remote_surface_id, cols, rows);
        }
    }

    /// `apply_stream_outcome` 지원 — `mesh_context_requests` 배치 적용. mesh
    /// 구독/geometry 갱신 — 구독 요청 자체가 capability negotiation이다. holder
    /// 불일치/미점유 surface 는 명시 MeshError 로 회신한다. 실제 plugin 구동 + mesh
    /// 바이트 forward 는 GUI-live/GUI-parked/headless 세 경로 모두에 배선되어 있다(상세:
    /// docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-소비-경로).
    fn apply_mesh_context_requests_batch(
        &mut self,
        requests: impl IntoIterator<
            Item = (
                u32,
                u32,
                u32,
                u32,
                f32,
                Option<tasty_plugin_protocol::protocol::ThemeWire>,
                bool,
            ),
        >,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id, width_px, height_px, pixels_per_point, theme, focused) in
            requests
        {
            let ok = self.apply_mesh_context_on_owning_engine(
                surface_id,
                client_id,
                width_px,
                height_px,
                pixels_per_point,
                theme,
                focused,
            );
            if !ok {
                reply_mesh_error(hub, client_id, surface_id, "not_attached");
            }
        }
    }

    /// `apply_stream_outcome` 지원 — `mesh_full_resend_requests` 배치 적용.
    fn apply_mesh_full_resend_requests_batch(
        &mut self,
        requests: impl IntoIterator<Item = (u32, u32)>,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id) in requests {
            let ok = self.apply_mesh_full_resend_on_owning_engine(surface_id, client_id);
            if !ok {
                reply_mesh_error(hub, client_id, surface_id, "not_attached");
            }
        }
    }

    /// `apply_stream_outcome` 지원 — `mesh_input_events` 배치 적용. attach mesh
    /// mirror 입력 역방향 forward — 위 `mesh_context_requests` 와 동일 배선
    /// 범위(headless 만 실제 plugin 구동). gui 가 attach 서버인 경우는 아직 이 축이
    /// 배선되지 않았다(상세 `docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-
    /// 소비-경로`).
    fn apply_mesh_input_events_batch(
        &mut self,
        events: impl IntoIterator<Item = (u32, u32, tasty_plugin_protocol::protocol::RawInputWire)>,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, surface_id, input) in events {
            let ok = self.apply_mesh_input_on_owning_engine(surface_id, client_id, input);
            if !ok {
                reply_mesh_error(hub, client_id, surface_id, "not_attached");
            }
        }
    }

    /// `apply_stream_outcome` 지원 — `capture_uploads` 배치 적용.
    fn apply_capture_uploads_batch(
        &mut self,
        uploads: impl IntoIterator<
            Item = (
                u32,
                crate::adapters::production::stream_hub::CaptureUploadMsg,
            ),
        >,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, msg) in uploads {
            self.apply_capture_upload_msg(client_id, msg, hub);
        }
    }

    /// `apply_stream_outcome` 지원 — `list_dir_requests` 배치 적용.
    fn apply_list_dir_requests_batch(
        &mut self,
        requests: impl IntoIterator<
            Item = (
                u32,
                crate::adapters::production::stream_hub::ListDirRequestMsg,
            ),
        >,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, msg) in requests {
            self.apply_list_dir_request_msg(client_id, msg, hub);
        }
    }

    /// `apply_stream_outcome` 지원 — `git_query_requests` 배치 적용.
    /// git-viewer(`docs/adr/0056-git-viewer-remote-attach-git-query-channel.md`):
    /// mirror client 가 attach 채널로 보낸 git 조회 요청. holder(그 client 가 점유한
    /// 워크스페이스를 가진 engine)를 찾아 처리.
    fn apply_git_query_requests_batch(
        &mut self,
        requests: impl IntoIterator<
            Item = (
                u32,
                crate::adapters::production::stream_hub::GitQueryRequestMsg,
            ),
        >,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, msg) in requests {
            self.apply_git_query_request_msg(client_id, msg, hub);
        }
    }

    /// `apply_stream_outcome` 지원 — `bulk_events` 배치 적용.
    fn apply_bulk_events_batch(
        &mut self,
        events: impl IntoIterator<Item = (u32, crate::adapters::production::stream_hub::BulkEvent)>,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        for (client_id, event) in events {
            match hub.bulk_workspace(client_id) {
                Some(ws) => self.apply_bulk_event(client_id, event, ws, hub),
                None => tracing::warn!(
                    "bulk transfer: event from non-bulk client {client_id} — ignoring"
                ),
            }
        }
    }

    /// 대상 surface 를 소유한 engine 을 찾아 attach 결선. 소유 engine 없으면 false.
    fn attach_on_owning_engine(
        &mut self,
        surface_id: u32,
        client_id: u32,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let e = &mut main.core_state;
                if e.terminals.contains(surface_id) || e.is_surface_deferred(surface_id) {
                    e.attach_surface_for_stream(surface_id, client_id, hub);
                    return true;
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.terminals.contains(surface_id) || engine.is_surface_deferred(surface_id) {
                engine.attach_surface_for_stream(surface_id, client_id, hub);
                return true;
            }
        }
        false
    }

    /// 대상 workspace 를 소유한 engine 을 찾아 workspace attach 결선(단계 6). 없으면 false.
    fn attach_workspace_on_owning_engine(
        &mut self,
        workspace_id: u32,
        client_id: u32,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let e = &mut main.core_state;
                if e.find_workspace_index_for_id(workspace_id).is_some() {
                    e.attach_workspace_for_stream(workspace_id, client_id, hub);
                    return true;
                }
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.find_workspace_index_for_id(workspace_id).is_some() {
                engine.attach_workspace_for_stream(workspace_id, client_id, hub);
                return true;
            }
        }
        false
    }

    /// stream client 입력을 적절한 engine 으로. workspace mode(단계 6)면 입력은
    /// surface-prefixed → demux 후 지정 surface; 아니면 단계 4 의 bare 단일 surface.
    fn feed_stream_input(&mut self, client_id: u32, bytes: &[u8]) -> bool {
        // workspace 점유 engine 우선(client_holds_workspace).
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.attach.client_holds_workspace(client_id)
            {
                return Self::demux_workspace_input(&mut main.core_state, client_id, bytes);
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.attach.client_holds_workspace(client_id) {
                return Self::demux_workspace_input(engine, client_id, bytes);
            }
        }
        // surface 단위(단계 4) 폴백.
        self.feed_input_on_owning_engine(client_id, bytes)
    }

    fn demux_workspace_input(
        engine: &mut crate::core::CoreState,
        client_id: u32,
        bytes: &[u8],
    ) -> bool {
        match crate::ipc::stream::decode_mux(bytes) {
            Some((sid, payload)) => engine.feed_attached_workspace_input(client_id, sid, payload),
            None => false,
        }
    }

    /// client 가 점유한 surface 를 가진 engine 에 입력 전달. 없으면 false.
    fn feed_input_on_owning_engine(&mut self, client_id: u32, bytes: &[u8]) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.feed_attached_input(client_id, bytes)
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.feed_attached_input(client_id, bytes) {
                return true;
            }
        }
        false
    }

    /// 대상 mesh surface 를 점유(holder)한 engine 을 찾아 구독/geometry 갱신을
    /// 반영한다(상세 `docs/dev-guide/egui-mesh-channel.md`). holder 검증은
    /// `CoreState::apply_attached_mesh_context`
    /// 내부에서 하므로, 여기선 "이 client 가 그 surface 의 attach lock 을 쥔 engine"만
    /// 찾으면 된다 — 못 찾으면 false(호출자가 `MeshError` 회신).
    #[allow(clippy::too_many_arguments)]
    fn apply_mesh_context_on_owning_engine(
        &mut self,
        surface_id: u32,
        client_id: u32,
        width_px: u32,
        height_px: u32,
        pixels_per_point: f32,
        theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
        focused: bool,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.apply_attached_mesh_context(
                    surface_id,
                    client_id,
                    width_px,
                    height_px,
                    pixels_per_point,
                    theme.clone(),
                    focused,
                )
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_mesh_context(
                surface_id,
                client_id,
                width_px,
                height_px,
                pixels_per_point,
                theme.clone(),
                focused,
            ) {
                return true;
            }
        }
        false
    }

    /// [`Self::apply_mesh_context_on_owning_engine`]과 동형의 입력 forward 버전(상세
    /// `docs/dev-guide/egui-mesh-channel.md#attach-mesh-mirror-소비-경로`).
    fn apply_mesh_input_on_owning_engine(
        &mut self,
        surface_id: u32,
        client_id: u32,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    ) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main
                    .core_state
                    .apply_attached_mesh_input(surface_id, client_id, input.clone())
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_mesh_input(surface_id, client_id, input.clone()) {
                return true;
            }
        }
        false
    }

    /// [`Self::apply_mesh_context_on_owning_engine`]과 동형의 full-resend 버전.
    fn apply_mesh_full_resend_on_owning_engine(&mut self, surface_id: u32, client_id: u32) -> bool {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main
                    .core_state
                    .apply_attached_mesh_full_resend(surface_id, client_id)
            {
                return true;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_mesh_full_resend(surface_id, client_id) {
                return true;
            }
        }
        false
    }

    /// mirror client 가 forward 한 구조 op 를 실행하고 `StructuralResult` 로 회신한다.
    /// anchor surface 가 속한 워크스페이스를 **그 client 가 점유(holder)** 하고 있는
    /// main window 에서만 실행한다(ADR-0040 hard 점유 = 구조 변경 권한). holder 가
    /// 아니거나 대상 워크스페이스를 찾지 못하면 `ok:false` 로 거부한다.
    ///
    /// 한계: parked engine(백그라운드 창)은 `AppState` 를 갖지 않아 재사용 핸들러를
    /// 호출할 수 없다 — mirror-attach 된 워크스페이스는 활성 호스팅 상태라 실제로는
    /// main window 에 있다.
    fn apply_forwarded_structural_op(
        &mut self,
        client_id: u32,
        op_id: u64,
        op: &crate::ipc::stream::StructuralOp,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        let anchor = op.anchor_surface_id();
        let core = &mut self.core;
        let mut handled = false;
        for w in self.view.views.values_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            let engine = &mut main.core_state;
            let Some(ws) = engine.attach.workspace_of_surface(anchor) else {
                continue;
            };
            handled = true;
            // 점유자(holder)만 이 워크스페이스를 조작할 수 있다(ADR-0040 hard 점유).
            let (ok, reason, delta) = if engine.attach.workspace_holder(ws) != Some(client_id) {
                (false, Some("not workspace holder".to_string()), None)
            } else {
                match crate::core::attach_runtime::execute_forwarded_structural_op(
                    core,
                    &mut main.state,
                    engine,
                    op,
                ) {
                    Ok(delta) => (true, None, delta),
                    Err(reason) => (false, Some(reason), None),
                }
            };
            // 순서: StructuralResult(회신) → StructuralDelta(역반영) → 새 surface tap.
            // delta 를 tap 보다 먼저 보내 client 가 매핑을 만든 뒤 스냅샷을 받게 한다.
            reply_structural_result(hub, client_id, op_id, ok, reason);
            if let Some(fd) = delta {
                push_structural_delta(hub, client_id, &fd.delta);
                for sid in fd.added_terminals {
                    engine.tap_surface_for_stream(sid, client_id, hub);
                }
                // forward 된 ConvertSurface 가 실제 kind 를 바꿨으면, 로컬(비-forward)
                // 변환 경로의 cascade(`app/dispatch_domain.rs` SurfaceConverted)와 동일하게
                // egui-mesh stale frame 을 버려 재-bootstrap 을 강제한다.
                if let Some(sid) = fd.converted_surface
                    && let Some(mgr) = self.plugin_manager.as_mut()
                {
                    mgr.drop_egui_mesh_frame(sid);
                }
            }
            w.mark_dirty();
            break;
        }
        if !handled {
            reply_structural_result(
                hub,
                client_id,
                op_id,
                false,
                Some("workspace not found".to_string()),
            );
        }
    }

    /// mirror client 가 forward 한 client-driven resize(ADR-0045)를 원격 PTY 에
    /// 적용한다. anchor surface 를 가진 engine 을 순회하며
    /// `apply_attached_workspace_resize`(holder 검증 포함)를 호출한다 — 성공한
    /// engine 에서 멈춘다. 실제 grid 변화는 `Terminal::resize` 가 판정하고, 변화
    /// 시 resize tap 이 server→client `Resize` echo 를 자동 fan-out 한다(추가 push
    /// 불요). holder 아님/미발견이면 조용히 무시(구조 op 와 달리 회신 프레임 없음 —
    /// 요청은 idempotent 하고 echo 부재가 곧 "변화 없음"이다).
    fn apply_forwarded_resize(
        &mut self,
        client_id: u32,
        remote_surface_id: u32,
        cols: usize,
        rows: usize,
    ) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.apply_attached_workspace_resize(
                    client_id,
                    remote_surface_id,
                    cols,
                    rows,
                )
            {
                w.mark_dirty();
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.apply_attached_workspace_resize(client_id, remote_surface_id, cols, rows) {
                return;
            }
        }
    }

    /// (03) screenshot→remote-clipboard — mirror client 가 보낸 캡처 업로드 청크/커밋
    /// 하나를 적용한다. `client_id` 가 워크스페이스를 점유(holder)한 engine 을 찾아
    /// 그 engine 의 `capture_uploads` 레지스트리에 누적하거나(청크) 완결 처리한다
    /// (커밋 — `finalize_capture_upload` 가 파일 저장 + 클립보드 기록 + 회신까지 담당).
    fn apply_capture_upload_msg(
        &mut self,
        client_id: u32,
        msg: crate::adapters::production::stream_hub::CaptureUploadMsg,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        use crate::adapters::production::stream_hub::CaptureUploadMsg;
        match msg {
            CaptureUploadMsg::CaptureChunk {
                upload_id,
                data_b64,
                ..
            } => {
                use base64::Engine as _;
                let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&data_b64) else {
                    tracing::warn!(
                        "capture upload: invalid base64 chunk (client {client_id}, upload {upload_id})"
                    );
                    return;
                };
                match find_workspace_holder_engine_mut(
                    &mut self.view.views,
                    &mut self.parked_states,
                    client_id,
                ) {
                    Some(engine) => engine.capture_uploads.append(
                        client_id,
                        upload_id,
                        &bytes,
                        std::time::Instant::now(),
                    ),
                    None => tracing::warn!(
                        "capture upload: client {client_id} does not hold a workspace — dropping chunk"
                    ),
                }
            }
            CaptureUploadMsg::CaptureCommit {
                upload_id,
                file_name,
            } => {
                let core = &self.core;
                match find_workspace_holder_engine_mut(
                    &mut self.view.views,
                    &mut self.parked_states,
                    client_id,
                ) {
                    Some(engine) => {
                        crate::core::attach_runtime::finalize_capture_upload(
                            engine, core, hub, client_id, upload_id, &file_name,
                        );
                    }
                    None => {
                        // holder 를 못 찾음(예: chunk 하나 없이 commit 만 도착, 혹은 이미
                        // detach) — capture_result 실패 회신.
                        let payload = serde_json::json!({
                            "event": "capture_result",
                            "upload_id": upload_id,
                            "ok": false,
                            "reason": "client does not hold a workspace attach",
                        });
                        let frame = crate::ipc::stream::StreamFrame::new(
                            crate::ipc::stream::StreamTag::Control,
                            serde_json::to_vec(&payload).unwrap_or_default(),
                        );
                        let _ = hub.push(client_id, frame); // best-effort — client 끊김 시 무해.
                    }
                }
            }
        }
    }

    /// (04) file picker — mirror client 가 attach 채널로 보낸 `list_dir_request`
    /// 하나를 적용한다. `client_id` 가 워크스페이스를 점유(holder)한 engine 을 찾아
    /// `attach_runtime::handle_list_dir_request` 로 위임(holder 검증 + 디렉토리
    /// 읽기 + `list_dir_result` 회신까지 그 함수가 담당).
    fn apply_list_dir_request_msg(
        &mut self,
        client_id: u32,
        msg: crate::adapters::production::stream_hub::ListDirRequestMsg,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        use crate::adapters::production::stream_hub::ListDirRequestMsg;
        let ListDirRequestMsg::ListDirRequest { request_id, dir } = msg;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.attach.client_holds_workspace(client_id)
            {
                crate::core::attach_runtime::handle_list_dir_request(
                    &mut main.core_state,
                    hub,
                    client_id,
                    request_id,
                    &dir,
                );
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.attach.client_holds_workspace(client_id) {
                crate::core::attach_runtime::handle_list_dir_request(
                    engine, hub, client_id, request_id, &dir,
                );
                return;
            }
        }
        // holder 를 못 찾음 — list_dir_result 실패 회신(commit-without-holder 와 동일 처리).
        let payload = serde_json::json!({
            "event": "list_dir_result",
            "request_id": request_id,
            "ok": false,
            "reason": "client does not hold a workspace attach",
        });
        let frame = crate::ipc::stream::StreamFrame::new(
            crate::ipc::stream::StreamTag::Control,
            serde_json::to_vec(&payload).unwrap_or_default(),
        );
        let _ = hub.push(client_id, frame); // best-effort — client 끊김 시 무해.
    }

    /// git-viewer(`docs/adr/0056-git-viewer-remote-attach-git-query-channel.md`) —
    /// mirror client 가 attach 채널로 보낸 `git_query_request` 하나를 적용한다.
    /// `apply_list_dir_request_msg` 와 완전히 동형 — holder engine 을
    /// 찾아 `attach_runtime::handle_git_query_request` 로 위임한다.
    fn apply_git_query_request_msg(
        &mut self,
        client_id: u32,
        msg: crate::adapters::production::stream_hub::GitQueryRequestMsg,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        use crate::adapters::production::stream_hub::GitQueryRequestMsg;
        let GitQueryRequestMsg::GitQueryRequest {
            request_id,
            surface_id,
            kind,
            worktree_path,
            diff_path,
        } = msg;
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main.core_state.attach.client_holds_workspace(client_id)
            {
                crate::core::attach_runtime::handle_git_query_request(
                    &mut main.core_state,
                    hub,
                    client_id,
                    request_id,
                    surface_id,
                    kind,
                    worktree_path,
                    diff_path,
                );
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.attach.client_holds_workspace(client_id) {
                crate::core::attach_runtime::handle_git_query_request(
                    engine,
                    hub,
                    client_id,
                    request_id,
                    surface_id,
                    kind,
                    worktree_path,
                    diff_path,
                );
                return;
            }
        }
        // holder 를 못 찾음 — git_query_result 실패 회신(list_dir 과 동일 처리).
        let payload = serde_json::json!({
            "event": "git_query_result",
            "request_id": request_id,
            "ok": false,
            "kind": kind.as_wire_str(),
            "reason": "client does not hold a workspace attach",
        });
        let frame = crate::ipc::stream::StreamFrame::new(
            crate::ipc::stream::StreamTag::Control,
            serde_json::to_vec(&payload).unwrap_or_default(),
        );
        let _ = hub.push(client_id, frame); // best-effort — client 끊김 시 무해.
    }

    /// (06) native bulk 파일 전송 이벤트(begin/chunk/commit) 하나를 결속
    /// workspace(`bulk_ws`)를 **소유한** engine 으로 라우팅한다. 호출자가 이 메서드를
    /// `bulk_events` 순서대로 부르므로 begin→chunk→commit 이 올바른 순서로 같은
    /// engine 에 도착한다. bulk 연결은 holder 가 아니므로(조사 §6)
    /// `client_holds_workspace` 로 engine 을 못 찾는다 — workspace **소유** 기준
    /// (`find_workspace_index_for_id`)으로 라우팅해 begin/chunk/commit 이 항상 같은
    /// engine 에 모이게 한다. begin=등록, chunk=append, commit=finalize(인가 검증 +
    /// 저장 + `BulkResult` 회신). 소유 engine 이 없으면 commit 은 실패 회신, begin/
    /// chunk 는 warn 후 드롭.
    fn apply_bulk_event(
        &mut self,
        client_id: u32,
        event: crate::adapters::production::stream_hub::BulkEvent,
        bulk_ws: u32,
        hub: &crate::adapters::production::stream_hub::StreamHub,
    ) {
        use crate::adapters::production::stream_hub::BulkEvent;
        match event {
            BulkEvent::Begin {
                transfer_id,
                filename,
                total_size,
            } => {
                if self
                    .with_bulk_ws_engine(bulk_ws, |engine| {
                        // (07) 용량 사전판정 — 초과면 등록하지 않고 capacity-exceeded
                        // 회신(청크 0바이트 수신). 통과 시 begin 등록.
                        crate::core::attach_runtime::begin_bulk_transfer(
                            engine,
                            hub,
                            client_id,
                            transfer_id,
                            filename,
                            total_size,
                        );
                    })
                    .is_none()
                {
                    tracing::warn!(
                        "bulk transfer: no engine owns workspace {bulk_ws} — dropping begin"
                    );
                }
            }
            BulkEvent::Chunk {
                transfer_id,
                seq,
                bytes,
            } => {
                let found = self.with_bulk_ws_engine(bulk_ws, |engine| {
                    engine
                        .bulk_transfers
                        .append(client_id, transfer_id, seq, &bytes)
                });
                log_bulk_chunk_result(found, client_id, transfer_id, bulk_ws);
            }
            BulkEvent::Commit { transfer_id } => {
                let found = self.with_bulk_ws_engine(bulk_ws, |engine| {
                    // (07) 저장 dir 은 설정값(빈 값이면 기본 폴더) — begin 용량 판정과
                    // 같은 폴더 기준. 소유 engine 의 settings 에서 도출한다.
                    let dir =
                        crate::core::attach_runtime::resolve_bulk_transfer_dir(&engine.settings);
                    crate::core::attach_runtime::finalize_bulk_transfer(
                        engine,
                        hub,
                        client_id,
                        transfer_id,
                        bulk_ws,
                        dir,
                    );
                });
                if found.is_none() {
                    // 소유 engine 없음 → bulk_result 실패 회신.
                    send_bulk_commit_failure(hub, client_id, transfer_id);
                }
            }
        }
    }

    /// `bulk_ws` 를 소유한 engine(활성 main view 또는 parked)을 찾아 `f` 를 그 engine 에
    /// 적용하고 반환값을 돌려준다. 소유 engine 이 없으면 `None`. begin/chunk/commit 이
    /// 항상 같은 소유 engine 으로 가도록 라우팅을 한 곳에 모은다.
    fn with_bulk_ws_engine<R>(
        &mut self,
        bulk_ws: u32,
        f: impl FnOnce(&mut crate::core::CoreState) -> R,
    ) -> Option<R> {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut()
                && main
                    .core_state
                    .find_workspace_index_for_id(bulk_ws)
                    .is_some()
            {
                return Some(f(&mut main.core_state));
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.find_workspace_index_for_id(bulk_ws).is_some() {
                return Some(f(engine));
            }
        }
        None
    }
}

/// `StructuralResult` 회신 프레임을 client 에 push(best-effort).
/// `client_id` 가 워크스페이스를 점유(holder)한 engine 을 찾는다(main view 우선,
/// 다음 parked). `apply_capture_upload_msg` 의 `CaptureChunk`/`CaptureCommit` 두 arm이
/// 중복하던 holder 탐색을 공용화. `App::` 메서드(`&mut self`) 대신 개별 필드를 받아 —
/// 호출자가 `self.core` 등 다른 필드를 동시에 빌릴 수 있게 한다.
fn find_workspace_holder_engine_mut<'a>(
    views: &'a mut std::collections::HashMap<WindowId, Box<dyn View>>,
    parked_states: &'a mut [(crate::state::AppState, crate::core::CoreState)],
    client_id: u32,
) -> Option<&'a mut crate::core::CoreState> {
    for w in views.values_mut() {
        if let Some(main) = w.as_main_mut()
            && main.core_state.attach.client_holds_workspace(client_id)
        {
            return Some(&mut main.core_state);
        }
    }
    parked_states
        .iter_mut()
        .map(|(_, engine)| engine)
        .find(|engine| engine.attach.client_holds_workspace(client_id))
}

/// `apply_bulk_event` 의 `Chunk` 처리 결과 로깅 — begin 없이 청크가 온 경우(no
/// transfer)와 소유 engine 자체가 없는 경우를 구분해 warn.
fn log_bulk_chunk_result(found: Option<bool>, client_id: u32, transfer_id: u64, bulk_ws: u32) {
    match found {
        Some(true) => {}
        Some(false) => tracing::warn!(
            "bulk transfer: chunk for unknown transfer (client {client_id}, transfer {transfer_id}) — no begin? dropping"
        ),
        None => {
            tracing::warn!("bulk transfer: no engine owns workspace {bulk_ws} — dropping chunk")
        }
    }
}

/// `apply_bulk_event` 의 `Commit` 처리 — 소유 engine 을 못 찾았을 때 `BulkResult`
/// 실패 회신을 client 에 push(best-effort).
fn send_bulk_commit_failure(
    hub: &crate::adapters::production::stream_hub::StreamHub,
    client_id: u32,
    transfer_id: u64,
) {
    let reply = crate::ipc::stream::StreamControl::BulkResult {
        transfer_id,
        ok: false,
        path: None,
        reason: Some("no engine owns the bound workspace".to_string()),
    };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // best-effort — client 끊김 시 무해.
}

fn reply_structural_result(
    hub: &crate::adapters::production::stream_hub::StreamHub,
    client_id: u32,
    op_id: u64,
    ok: bool,
    reason: Option<String>,
) {
    let reply = crate::ipc::stream::StreamControl::StructuralResult { op_id, ok, reason };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // best-effort 회신 — client 끊김 시 무해.
}

/// `StructuralDelta`(역반영) 프레임을 client 에 push(best-effort).
fn push_structural_delta(
    hub: &crate::adapters::production::stream_hub::StreamHub,
    client_id: u32,
    delta: &crate::ipc::stream::StreamControl,
) {
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(delta).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // best-effort delta — client 끊김 시 무해.
}

/// `MeshError`(미지원/미점유 mesh 요청은 무시 대신 명시 오류로 회신, 상세
/// `docs/dev-guide/attach-behavior.md` / `docs/dev-guide/egui-mesh-channel.md`)
/// 회신 프레임을 client 에 push(best-effort).
fn reply_mesh_error(
    hub: &crate::adapters::production::stream_hub::StreamHub,
    client_id: u32,
    surface_id: u32,
    reason: &str,
) {
    let reply = crate::ipc::stream::StreamControl::MeshError {
        surface_id,
        reason: reason.to_string(),
    };
    let frame = crate::ipc::stream::StreamFrame::new(
        crate::ipc::stream::StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // best-effort 오류 회신 — client 끊김 시 무해.
}

/// 네이티브 컨텍스트 메뉴가 떠 있는 동안의 폴링 주기 상한. 메뉴 트래킹(하이라이트
/// 이동 등)이 사람 눈에 끊겨 보이지 않을 만큼 짧고, idle 상태에서 winit 을 계속
/// 깨우지는 않을 만큼만 짧다.
const PENDING_MENU_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);

/// pending native menu 유무 → control flow 재예약 결정. `None` 이면 기존 흐름
/// (`Wait`)을 그대로 둔다. 순수 함수로 뽑아 헤드리스 회귀 테스트가 가능하게 했다 —
/// 이 재예약을 빠뜨리면 메뉴가 열린 채 프레임이 멈춘다.
fn pending_menu_control_flow(
    has_pending: bool,
    now: std::time::Instant,
) -> Option<winit::event_loop::ControlFlow> {
    has_pending.then(|| winit::event_loop::ControlFlow::WaitUntil(now + PENDING_MENU_POLL_INTERVAL))
}

impl App {
    /// 결과 미회수 네이티브 컨텍스트 메뉴를 가진 MainView 가 하나라도 있는지.
    /// 포커스 무관하게 전 창을 순회한다(불가침 원칙 §3).
    fn any_pending_native_menu(&self) -> bool {
        self.view
            .views
            .values()
            .any(|w| w.as_main().is_some_and(|m| m.has_pending_native_menu()))
    }

    /// 열려 있는 네이티브 컨텍스트 메뉴를 전 창에 걸쳐 1회씩 펌프한다.
    /// 메뉴가 없는 창에서는 no-op.
    fn poll_pending_native_menus(&mut self) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                main.poll_pending_native_menu();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PENDING_MENU_POLL_INTERVAL, pending_menu_control_flow};
    use winit::event_loop::ControlFlow;

    /// 메뉴가 떠 있으면 짧은 WaitUntil 로 다음 폴링 프레임을 반드시 예약한다.
    #[test]
    fn pending_menu_reschedules_a_poll_frame() {
        let now = std::time::Instant::now();
        match pending_menu_control_flow(true, now) {
            Some(ControlFlow::WaitUntil(at)) => {
                assert_eq!(at, now + PENDING_MENU_POLL_INTERVAL);
                assert!(
                    PENDING_MENU_POLL_INTERVAL <= std::time::Duration::from_millis(16),
                    "폴링 주기가 한 프레임(60fps)보다 길면 메뉴 트래킹이 끊겨 보인다"
                );
            }
            other => panic!("expected a WaitUntil reschedule, got {other:?}"),
        }
    }

    /// 메뉴가 없으면 기존 control flow 를 건드리지 않는다(상시 wakeup 금지).
    #[test]
    fn no_pending_menu_leaves_control_flow_alone() {
        assert!(pending_menu_control_flow(false, std::time::Instant::now()).is_none());
    }
}
