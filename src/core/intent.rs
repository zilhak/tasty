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

/// `DomainIntent::ConvertSurface` 의 target. variant 별로 새 surface 생성
/// 경로가 다름 (terminal: PTY spawn, markdown/image: builtin 패널, kind:
/// registry).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum ConvertSurfaceTarget {
    Terminal { cwd: Option<PathBuf> },
    Markdown { file_path: String },
    Image,
    Kind { kind: String, params: Value },
}

/// `DomainIntent::SendToSurface` 의 payload. 호출자가 *어느 메서드 호출* 할지
/// 결정해 전달 — Core 는 받아서 그대로 dispatch.
/// - `Bytes`: 변환 완료된 raw bytes (control sequences 포함). `send_bytes` 호출.
/// - `Text`: raw text. `send_key` 호출 (escape 처리 없음 — UTF-8 그대로).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum SendPayload {
    Bytes(Vec<u8>),
    Text(String),
}

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
    /// 특정 surface 를 split (같은 tab 안에서). focused 의존 없음 — 호출자가
    /// target_surface_id 결정. cascade 가 (User origin 이면) tab 의
    /// focused_surface 를 new_surface_id 로 변경.
    SplitSurface {
        target_surface_id: u32,
        direction: crate::model::SplitDirection,
        cwd: Option<PathBuf>,
        kind: String,
        surface_params: Value,
    },
    /// pane_id 로 pane 제거. workspace 안 invariant (focused_pane 보존) 는
    /// Core::apply 가 직접 처리 (workspace 안 *닫힌 곳* 의 자연 이동 — 원칙 1
    /// 위반 아님). cleanup_surface (markdown/image/memory) 는 cascade.
    ClosePane { pane_id: u32 },
    /// surface 제거. cascading close — surface→tab→pane→workspace 단계까지
    /// 자동 cascade. `save_snapshot=true` 면 각 단계에서 ClosedItem snapshot
    /// 푸시 (Ctrl+Shift+T 복원). cleanup_surface / workspace scope memory purge /
    /// active_workspace 보정 / auto-recreate empty workspace 는 cascade + handler.
    CloseSurface {
        surface_id: u32,
        save_snapshot: bool,
    },
    /// 기존 surface 를 다른 kind 로 변환. tab 의 split 안 leaf 만 교체 / sole
    /// surface tab 전체 교체. terminal 변환은 호출자가 cwd 미리 결정.
    ConvertSurface {
        surface_id: u32,
        target: ConvertSurfaceTarget,
    },

    // ─── Terminal send (D.3.C.C.1) ───
    /// terminal surface 에 입력 전송. payload 의 종류에 따라 send_bytes 또는
    /// send_key 호출. ensure_surface_initialized 도 Core 가 처리.
    SendToSurface {
        surface_id: u32,
        payload: SendPayload,
    },
    /// 특정 surface 의 PTY 를 새 terminal 로 교체 (respawn). cwd 가 주어지면
    /// 새 PTY 의 working_dir 로 사용. plugin `claude.respawn` 의 진입점.
    RespawnTerminal {
        surface_id: u32,
        cwd: Option<PathBuf>,
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

    // ─── Closed items (D.3.C.D.5) ───
    /// closed_items stack top 을 pop 해 복원. `target_pane_id` 는 *호출자가
    /// 결정한* attach 대상 (focused pane). Workspace 복원 시에는 사용 안 함.
    /// `target_pane_id == None` 이면 (engine.workspaces 비어있는 상태에서
    /// Surface/Tab 을 복원 요청한 경우) 복원은 Workspace 인 경우만 가능.
    /// 이 경우 caller 가 사전에 ensure_workspace_exists 처리하는 것을 권장.
    RestoreClosedItem { target_pane_id: Option<u32> },
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
    /// surface split 완료. cascade 가 (User origin 이면) tab 의 focused_surface
    /// 를 new_surface_id 로 변경.
    SurfaceSplit {
        workspace_index: usize,
        pane_id: u32,
        target_surface_id: u32,
        new_surface_id: u32,
    },
    /// pane close 완료. cleanup_targets 는 닫힌 pane 안의 (surface_id,
    /// persist_id) — cascade 가 cleanup_surface 호출.
    PaneClosed {
        pane_id: u32,
        closed: bool,
        cleanup_targets: Vec<(u32, Option<String>)>,
    },
    /// surface close 완료 (cascading). `cascade_level` 은 어디까지 닫혔는지
    /// (Surface/Tab/Pane/Workspace). `workspace_id_purged` 는 Case 4 (workspace
    /// 자체 닫힘) 시 cascade 가 memory scope purge 할 workspace_id.
    /// `workspaces_now_empty` 가 true 면 caller 가 auto-recreate.
    SurfaceClosed {
        surface_id: u32,
        closed: bool,
        cascade_level: CascadeLevel,
        cleanup_targets: Vec<(u32, Option<String>)>,
        workspace_id_purged: Option<u32>,
        workspaces_now_empty: bool,
    },
    /// surface 변환 완료. `replaced=false` 면 surface 못 찾음 또는 변환 실패.
    SurfaceConverted {
        surface_id: u32,
        replaced: bool,
        is_terminal: bool, // cascade 에서 send_fast_init 결정용
    },
    /// terminal send 완료. `sent=false` 면 surface 가 terminal 이 아니거나 없음.
    SurfaceSent { surface_id: u32, sent: bool },
    /// terminal respawn 완료. `error` 가 Some 이면 spawn 실패 또는 surface
    /// 가 terminal 이 아님 — handler 가 invalid_params 반환.
    TerminalRespawned {
        surface_id: u32,
        error: Option<String>,
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

    // ─── Closed items (D.3.C.D.5) ───
    /// closed_items pop + 복원 완료. cascade 가 (Workspace kind 인 경우)
    /// active_workspace 를 새 인덱스로 옮긴다.
    /// - `restored=false`: closed_items 가 비었거나 rebuild 실패.
    /// - `kind`: 어떤 종류가 복원되었는지 + cascade 가 알아야 할 인덱스.
    ClosedItemRestored { restored: bool, kind: RestoredKind },
}

/// `CoreEvent::ClosedItemRestored` 의 복원 결과 분류.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum RestoredKind {
    /// 비어있는 스택 또는 rebuild 실패.
    Nothing,
    /// Workspace 복원 — cascade 가 `state.active_workspace = new_ws_index` 적용.
    Workspace { new_ws_index: usize },
    /// Surface 또는 Tab 이 기존 pane 의 tab 으로 attach 됨.
    /// cascade 가 별도 mutate 없이 mark_dirty 만 발화 (Core::apply 가 이미
    /// mark_layout_dirty 처리).
    TabIntoPane { pane_id: u32 },
}

/// `CoreEvent::SurfaceClosed` 의 cascade 깊이 정보.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum CascadeLevel {
    Surface,
    Tab,
    Pane,
    Workspace,
}
