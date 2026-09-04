//! winit user event 버스에 흐르는 host 자체 이벤트.
//!
//! 백그라운드 스레드 / IPC / OS-level 트리거가 `EventLoopProxy<AppEvent>` 로 push
//! 하면 App::user_event 가 분기 처리한다. variant 별 producer/consumer 매핑은
//! 각 doc-comment 참조.

/// 새 창 생성을 **누가** 요청했는지. 실패 안내 채널을 가르는 유일한 기준이다.
///
/// 핵심 원칙 1 은 에이전트 행동의 부수효과가 사용자 상태(포커스)에 닿는 것을 금지한다.
/// 실패 안내라도 예외가 아니다 — 사용자가 하지 않은 일의 실패 통지 때문에 하던 일의
/// 포커스를 잃어서는 안 된다. 그래서 `Agent` 는 포커스를 건드리지 않는 toast 로,
/// `User` 는 방금 그 조작의 결과이므로 모달로 알린다.
/// 근거: `docs/adr/0117-window-and-modal-creation-failure-policy.md`.
#[cfg(feature = "gui")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowRequestOrigin {
    /// 메뉴 · 단축키 · dock · tray — 사용자가 방금 요청했다.
    User,
    /// `window.create` / `view.create` IPC — 에이전트가 요청했다.
    Agent,
}

/// Custom events sent to the winit event loop from background threads.
#[derive(Debug)]
pub(crate) enum AppEvent {
    /// PTY reader thread produced output. If targeted_pty_polling is enabled,
    /// contains the surface_id that has new data. Otherwise None (poll all).
    TerminalOutput(Option<u32>),
    /// IPC command arrived -- wake up and process.
    IpcReady,
    /// A streaming-channel client sent an inbound frame -- wake up and drain the
    /// stream inbound queue (StreamHub::pump_inbound).
    StreamReady,
    /// egui requested a repaint (new window, animation, cursor blink).
    /// `window_id` 는 어느 윈도우의 egui 컨텍스트가 repaint 를 요청했는지 식별 —
    /// 핸들러는 해당 윈도우만 dirty 로 표시한다. (모든 egui Context 는 root viewport
    /// 만 쓰므로 `viewport_id` 는 항상 `ROOT` 라 윈도우 구분에 못 쓴다 — window_id 로 라우팅.)
    /// delay-aware repaint (`Duration > 0`) 는 idle frame loop 방지 위해 callback 단계에서 drop 된다.
    #[cfg(feature = "gui")]
    EguiRepaint { window_id: winit::window::WindowId },
    /// Request to create a new window. 페이로드는 **누가 요청했는지** — 창 생성이
    /// 실패했을 때 안내를 어느 채널로 낼지 가르는 유일한 기준이다
    /// ([`WindowRequestOrigin`], `docs/adr/0117-window-and-modal-creation-failure-policy.md`).
    #[cfg(feature = "gui")]
    CreateWindow(WindowRequestOrigin),
    /// CSD titlebar close 버튼이 발화하는 per-window 닫기 요청 (사용자 클릭).
    /// 네이티브 `WindowEvent::CloseRequested` 와 동일한 라이프사이클로 라우팅한다
    /// (단일 창이면 quit 흐름, 다중 창이면 해당 창만 닫음).
    #[cfg(feature = "gui")]
    CloseWindow(winit::window::WindowId),
    /// 사용자 스크립트 단축키가 눌려 Lua 워커에서 실행 요청 (ADR-0031).
    /// view 의 `handle_shortcut` 이 combo 매칭 후 스크립트 소스를 읽어 발행하고,
    /// App 이 소유한 `lua_engine` 워커로 실행한다(사용자 키 입력 경로에서만 — identity 원칙 1).
    RunLuaScript { source: String, name: String },
    /// Request to open settings modal.
    #[cfg(feature = "gui")]
    OpenSettings,
    /// Request to open plugins modal.
    #[cfg(feature = "gui")]
    OpenPlugins,
    /// Request to shut down the entire application.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    Shutdown,
    /// Request to minimize (park state, close windows).
    #[cfg(feature = "gui")]
    Minimize,
    /// Request quit following the close_behavior setting.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    QuitRequested,
    /// Request to re-show hidden windows from the system tray (Windows / Linux,
    /// where windows persist while hidden). macOS restores via CreateWindow instead.
    #[cfg(all(any(windows, target_os = "linux"), feature = "gui"))]
    TrayShowWindow,
    /// OS 가 절전(suspend)에서 복귀했다 (Windows `WM_POWERBROADCAST`). resume
    /// 헬스 패스를 돌려 죽은 ConPTY 자식을 정리하고 살아있는 자식을 wake nudge
    /// 한다 (ADR-0017). Windows 전용 — Unix PTY 는 절전에 강건해 불필요하다.
    #[cfg(all(windows, feature = "gui"))]
    SystemResumed,
    /// 중앙 타이머 허브(`docs/dev-guide/timer-hub.md`)의 waker 스레드가 다음 데드라인에
    /// 도달했다는 wake 신호. **이 이벤트 자체는 아무 일도 하지 않는다** — 깨어난
    /// `about_to_wait` 가 `timers.drain_due` 로 due 한 키를 실행한다. headless 는
    /// 메인 루프가 `recv_timeout` 으로 직접 데드라인을 지키므로 이 이벤트가 없다.
    #[cfg(feature = "gui")]
    TimerTick,
    /// attach/detach 작업 J — client mirror reader thread 가 원격 출력을 받으면 보내는
    /// 실시간 wake 신호. 서버측 readonly 뷰의 3초 cadence(`Tick::AttachView`)와 달리, 내가 직접
    /// 다루는 client mirror 는 로컬 워크스페이스처럼 데이터가 오는 즉시 적용/repaint 한다.
    /// App 이 누적 출력 버퍼를 drain 해 mirror Terminal 에 feed 한다.
    #[cfg(feature = "gui")]
    AttachClientData,
    /// attach/detach 단계 7 — 자동 attach 워커 스레드가 SSH 터널 수립(또는 loopback
    /// 해석)을 마치면 보내는 wake 신호. App 이 결과 채널(`auto_attach_rx`)을 drain 해
    /// `start_gui_attach` 로 mirror 를 띄운다. 터널 핸들은 채널로 전달(AppEvent 는
    /// Debug 라 핸들을 싣지 않는다).
    #[cfg(feature = "gui")]
    AutoAttachReady,
    /// (03) 스크린샷→클립보드 캡처 워커 스레드가 OS 인터랙티브 캡처를 마치면
    /// 보내는 wake 신호. App 이 결과 채널(`screenshot_capture_rx`)을 drain 해
    /// 로컬 클립보드에 기록하거나 mirror 세션으로 업로드한다.
    #[cfg(feature = "gui")]
    ScreenshotCaptureReady,
    /// (08) mirror 이미지 paste 업로드 워커 스레드가 bulk 업로드를 마치면 보내는 wake
    /// 신호. App 이 결과 채널(`image_upload_rx`)을 drain 해 성공 시 원격 경로를 대상
    /// mirror surface 입력에 삽입하거나(실패 시 Warning toast) 처리한다.
    #[cfg(feature = "gui")]
    ImageUploadReady,
    /// (09) mirror 파일 전송 업로드 워커의 진행 이벤트(청크 전송)가 도착했다는 wake 신호.
    /// App 이 진행 채널(`transfer_progress_rx`)을 drain 해 진행 팝업의 행(바이트/속도/
    /// determinate bar)을 갱신한다.
    #[cfg(feature = "gui")]
    TransferProgressTick,
    /// 비동기 파일 식별 결과. `IdentifyWorker::spawn` 의 worker thread 가 완료 시 송신.
    /// 콜사이트(Phase C 의 mouse.rs 등) 는 보관한 마지막 `request_id` 와 매칭해
    /// 오래된 결과를 drop 한다.
    #[cfg(feature = "gui")]
    IdentifyDone {
        request_id: crate::identify_worker::IdentifyRequestId,
        target: crate::file::format::FileTarget,
        detector: Option<crate::file::format::DetectorId>,
        /// `DispatchFile.origin_surface_id` 그대로 carry. Some 이면
        /// `apply_identify_result` → `execute_handler_action` 가
        /// origin 의 *Pane* 에 새 tab 으로 추가.
        origin_surface_id: Option<u32>,
        /// `DispatchFile.ignore_size_limit` 그대로 carry — 대용량 markdown 게이트를
        /// 건너뛴다(에이전트/IPC 강제 열기). 비동기 식별 왕복을 통과시키기 위함.
        ignore_size_limit: bool,
    },
}
