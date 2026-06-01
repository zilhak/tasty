//! `DomainIntent` — `Core::apply` 의 입력. 영속 도메인 mutate 요청.
//!
//! 분류축 (intent-ui-vs-domain.md): 모든 Intent 는 *UI Intent* (시각 상태
//! 변경, `crate::intent::UiIntent`) 또는 *Domain Intent* (도메인 mutate, 본
//! 타입) 중 하나다. `DomainIntent` 는 headless 빌드에서도 그대로 실행된다.
//!
//! 현재 큐 구조 (Phase D 진행 중):
//! - `AppState.pending_intents`: 통합 Intent 큐. UI Intent (`Intent::Ui`) 와
//!   Domain Intent (`Intent::Domain(DomainIntent)`) 가 같은 큐 위에서 처리됨.
//!   `App::dispatch_pending_intents` 가 매 frame drain — UI 항목은 popup handler
//!   분기, Domain 항목은 별 batch 로 모아 `dispatch_domain_intent` (core.apply +
//!   handle_core_event cascade) 일괄 처리.

use std::path::PathBuf;

use serde_json::Value;
use tasty_settings::Settings;

/// 도메인 변경 요청. Core 만이 자기 메서드로 적용한다.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum DomainIntent {
    // ─── Settings (D.3.C.A.2) ───
    /// Settings 전체 교체. cascade — Theme apply / Scrollback limit / clipboard
    /// max / notification coalesce 가 Core 내부에서 자동 발동.
    UpdateSettings(Settings),

    // ─── Workspace lifecycle (D.3.C.B.1) ───
    /// 새 workspace 를 생성. focused 의존 없음 — `cwd` 는 호출자가 미리
    /// 결정해 payload 로 넘긴다 (terminal kind 에서 사용). `kind="empty"` 는
    /// 거부. `name` 이 None 이면 자동 ("Workspace N"). cascade 가 host event
    /// (WorkspaceRenamed) 발화 + (User origin 이면) active 전환.
    CreateWorkspace {
        cwd: Option<PathBuf>,
        kind: String,
        surface_params: Value,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    },
    /// 기존 workspace 의 메타 (name/subtitle/description) 부분 갱신. None
    /// 필드는 변경 없음. cascade 가 host event (WorkspaceRenamed) 발화.
    UpdateWorkspaceMeta {
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    },
    /// workspace 순서 이동 (from_index → to_index). out-of-range 또는
    /// from==to 면 no-op. cascade 가 발화 source 의 active_workspace 를
    /// *사용자가 보던 동일 ws 가 계속 active 유지* 되도록 보정.
    MoveWorkspace { from_index: usize, to_index: usize },

    // ─── Tab lifecycle (D.3.C.B.5) ───
    /// 특정 pane 에 새 tab 생성. focused pane 의존 없음 — 호출자가 pane_id
    /// 미리 결정. `cwd` 는 terminal kind 에서만 사용 (호출자가 inherit 결정).
    CreateTab {
        pane_id: u32,
        cwd: Option<PathBuf>,
        kind: String,
        surface_params: Value,
    },
    /// tab_id 로 tab close. *모든* workspace 의 pane 순회 (포커스 독립).
    /// cleanup (markdown_views / image_views / surface_meta / memory purge) 은
    /// cascade 에서 처리 — Core 가 AppState 데이터 모름.
    CloseTab { tab_id: u32 },
    /// pane 안 tab 순서 이동 (from_index → to_index). out-of-range 면 no-op.
    /// pane_id 로 *모든* workspace 순회 — focused 의존 없음.
    MoveTab {
        pane_id: u32,
        from_index: usize,
        to_index: usize,
    },

    // ─── Pane lifecycle (D.3.C.B.3) ───
    /// 특정 pane 을 split. focused 의존 없음 — 호출자가 target_pane_id 결정.
    /// cwd 는 terminal kind 에서만 사용 (호출자가 inherit 결정).
    /// cascade 가 host event (PaneSplit) 발화 + (User origin 이면) focused_pane
    /// 을 new_pane_id 로 변경.
    SplitPane {
        target_pane_id: u32,
        direction: crate::model::SplitDirection,
        cwd: Option<PathBuf>,
        kind: String,
        surface_params: Value,
    },

    // ─── Notifications (D.3.C.E.2) ───
    /// 알림 push. ws_id 가 라우팅 키 — 해당 workspace 가 속한 main window 의
    /// notifications store 에 add (coalesce 자동) + host event enqueue.
    /// `source` 는 host event 의 source 태그 ("host" / "telemetry.cap" 등).
    PushNotification {
        ws_id: u32,
        surface_id: u32,
        title: String,
        body: String,
        source: String,
    },
    /// 특정 알림 읽음 처리. cascade 가 알림을 보유한 main/parked engine 의
    /// `notifications.mark_read(id)` 호출.
    MarkNotificationRead { id: u64 },
    /// 모든 알림 읽음 처리. cascade 가 main/parked 모두의
    /// `notifications.mark_all_read()` 호출.
    MarkAllNotificationsRead,

    // ─── Surface lifecycle (D.3.C.E.6) ───
    /// Terminal 이 OSC 7 등으로 cwd 변경을 알림. cascade 가
    /// `refresh_tab_display_name` + `mark_layout_dirty` 수행.
    SurfaceCwdChanged { surface_id: u32 },

    // ─── Terminal control (D.3.C.C.3) ───
    /// 특정 surface 의 read mark 설정. cascade 가 main/parked 의 engine
    /// 순회 후 terminal.set_mark() 호출. surface_id 가 None 이면 focused.
    SetTerminalMark { surface_id: u32 },

    // ─── Clipboard history (D.3.C.E.3) ───
    /// Terminal 내부 selection copy 같은 *internal* 클립보드 copy 를 history 에
    /// 기록. `Source::Internal` 태그로 일관. settings.clipboard.history_enabled=false
    /// 이면 cascade 가 no-op.
    RecordInternalClipboardCopy { text: String },
}

/// `Core::apply` 의 결과 — 도메인이 *변경 후 알리는* 이벤트.
/// observer / replay / remote attach 의 기반.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum CoreEvent {
    // ─── Settings (D.3.C.A.2) ───
    /// Settings 가 갱신됨. 새 값 동봉.
    SettingsUpdated(Settings),

    // ─── Workspace lifecycle (D.3.C.B.1) ───
    /// 새 workspace 생성 완료. cascade 가 host event (WorkspaceRenamed —
    /// name/subtitle/description 이 설정된 경우) 발화 + (User origin 이면)
    /// active 전환. `surface_id` 는 focused tab 의 surface.
    WorkspaceCreated {
        id: u32,
        index: usize,
        surface_id: Option<u32>,
        renamed_name: Option<String>,
        renamed_subtitle: Option<String>,
        renamed_description: Option<String>,
    },
    /// workspace 메타 갱신 완료. cascade 가 host event (WorkspaceRenamed) 발화.
    WorkspaceMetaUpdated {
        workspace_id: u32,
        index: usize,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    },
    /// workspace 가 이동됨. cascade 가 발화 source 의 active_workspace 보정.
    /// `moved=false` 면 no-op (out-of-range / from==to).
    WorkspaceMoved {
        from_index: usize,
        to_index: usize,
        moved: bool,
    },

    // ─── Tab lifecycle (D.3.C.B.5) ───
    /// 새 tab 생성 완료. cascade 추가 처리 없음 (main.mark_dirty 만).
    TabCreated {
        pane_id: u32,
        tab_id: u32,
        surface_id: u32,
        tab_count: usize,
        active_tab: usize,
    },
    /// tab close 완료. `cleanup_targets` 는 닫힌 tab 안의 (surface_id,
    /// persist_id) — cascade 가 각각에 `AppState::cleanup_surface` 호출.
    TabClosed {
        tab_id: u32,
        closed: bool,
        cleanup_targets: Vec<(u32, Option<String>)>,
    },
    /// tab 이동 완료. `moved=false` 면 no-op (pane 없음 / from==to / out-of-range).
    TabMoved { pane_id: u32, moved: bool },

    // ─── Pane lifecycle (D.3.C.B.3) ───
    /// pane split 완료. cascade 가 host event (PaneSplit) 발화 + (User origin
    /// 이면) focused_pane 변경.
    PaneSplit {
        workspace_index: usize,
        original_pane_id: u32,
        new_pane_id: u32,
        new_surface_id: u32,
        direction: crate::model::SplitDirection,
    },

    // ─── Notifications (D.3.C.E.2) ───
    /// 알림 push 요청. cascade 가 라우팅 + store.add + host event enqueue.
    NotificationPushRequested {
        ws_id: u32,
        surface_id: u32,
        title: String,
        body: String,
        source: String,
    },
    /// 특정 알림 읽음 처리 요청.
    NotificationReadRequested { id: u64 },
    /// 모든 알림 읽음 처리 요청.
    AllNotificationsReadRequested,

    // ─── Surface lifecycle (D.3.C.E.6) ───
    /// Surface 의 cwd 변경 알림. cascade 가 tab display name / layout dirty 갱신.
    SurfaceCwdChanged { surface_id: u32 },

    // ─── Terminal control (D.3.C.C.3) ───
    /// Terminal read mark 설정 요청. cascade 가 surface 보유 engine 에 적용.
    TerminalMarkSet { surface_id: u32 },

    // ─── Clipboard history (D.3.C.E.3) ───
    /// Internal clipboard copy 가 발생. cascade 가 모든 engine 의 history 에 기록.
    InternalClipboardCopyRecorded { text: String },
}
