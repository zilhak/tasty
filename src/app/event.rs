//! winit user event 버스에 흐르는 host 자체 이벤트.
//!
//! 백그라운드 스레드 / IPC / OS-level 트리거가 `EventLoopProxy<AppEvent>` 로 push
//! 하면 App::user_event 가 분기 처리한다. variant 별 producer/consumer 매핑은
//! 각 doc-comment 참조.

#[cfg(feature = "gui")]
use crate::ClipboardData;

/// Custom events sent to the winit event loop from background threads.
#[derive(Debug)]
pub(crate) enum AppEvent {
    /// PTY reader thread produced output. If targeted_pty_polling is enabled,
    /// contains the surface_id that has new data. Otherwise None (poll all).
    TerminalOutput(Option<u32>),
    /// IPC command arrived -- wake up and process.
    IpcReady,
    /// egui requested a repaint (new window, animation, cursor blink).
    /// `viewport_id` 는 어느 egui 컨텍스트가 repaint 를 요청했는지 식별 — 핸들러는
    /// 해당 viewport 의 view 만 dirty 로 표시한다.
    /// delay-aware repaint (`Duration > 0`) 는 idle frame loop 방지 위해 callback 단계에서 drop 된다.
    #[cfg(feature = "gui")]
    EguiRepaint { viewport_id: egui::ViewportId },
    /// Request to create a new window (triggered by IPC or shortcut).
    #[cfg(feature = "gui")]
    CreateWindow,
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
    /// Request to show window from system tray (Windows only).
    #[cfg(all(windows, feature = "gui"))]
    TrayShowWindow,
    /// 백그라운드 스레드에서 클립보드 변경을 감지하여 데이터를 전달.
    #[cfg(feature = "gui")]
    ClipboardChanged(ClipboardData),
    /// ~1초 간격 ticker. 모든 surface의 busy 상태를 다시 평가한다.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    BusyPoll,
    /// 비동기 파일 식별 결과. `IdentifyWorker::spawn` 의 worker thread 가 완료 시 송신.
    /// 콜사이트(Phase C 의 mouse.rs 등) 는 보관한 마지막 `request_id` 와 매칭해
    /// 오래된 결과를 drop 한다.
    #[cfg(feature = "gui")]
    IdentifyDone {
        request_id: crate::identify_worker::IdentifyRequestId,
        target: crate::file::format::FileTarget,
        detector: Option<crate::file::format::DetectorId>,
    },
}
