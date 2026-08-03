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
/// 경로가 다름 (terminal: PTY spawn, kind: SurfaceKindRegistry 경유 — markdown
/// / image 등 모든 host/plugin kind 통합).
#[derive(Debug, Clone)]
pub(crate) enum ConvertSurfaceTarget {
    Terminal {
        cwd: Option<PathBuf>,
    },
    /// `cwd` 는 호출자가 source surface 로부터 resolve 한 carry cwd. Surface cwd
    /// invariant — 호출자는 None 으로 임의 고정하지 않는다 (변환 시 cwd 손실 금지).
    /// 자세한 규칙은 `docs/architecture/invariants/surface-cwd.md`.
    Kind {
        cwd: Option<PathBuf>,
        kind: String,
        params: Value,
    },
}

/// `DomainIntent::SendToSurface` 의 payload. 호출자가 *어느 메서드 호출* 할지
/// 결정해 전달 — Core 는 받아서 그대로 dispatch.
/// - `Bytes`: 변환 완료된 raw bytes (control sequences 포함). `send_bytes` 호출.
/// - `Text`: raw text. `send_key` 호출 (escape 처리 없음 — UTF-8 그대로).
#[derive(Debug, Clone)]
pub(crate) enum SendPayload {
    Bytes(Vec<u8>),
    Text(String),
}

/// 도메인 변경 요청. Core 만이 자기 메서드로 적용한다.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // reason: hot intent queue 에 Box 화 시 alloc 비용 큼
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
        /// 생성 시점 카테고리 소속. `None` 이면 normal(기본).
        category: Option<crate::model::WorkspaceCategoryId>,
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
        /// 명시 탭 이름. `Some` 이면 생성 시점부터 `explicit_name` 으로 고정되어
        /// `display_name()` 에서 최우선으로 쓰인다 (cwd/OSC title 로 덮이지 않음).
        name: Option<String>,
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
    /// headless PTY(`pty_registry`, ADR-0050 · features/headless-pty 참고)를 실제
    /// Surface 로 **승격(adopt)** 한다(`pty.attach_surface`). `CreateTab` 처럼 새
    /// tab_id/surface_id 를 발급받아
    /// Tab/Pane 트리에 marker 를 꽂되, **새 Terminal 을 spawn 하지 않는다** — 이미
    /// headless(`PTY_ID_BASE` 이상) id 로 `TerminalStore` 에 존재하는 Terminal 을
    /// 새 surface_id 로 re-key 하고 `pty_registry` 에서 제거한다. `RespawnTerminal`
    /// 이 같은 id 위에서 Terminal 을 교체하는 것과 반대 방향(id 이전) 연산이다.
    /// 성공 시 `CoreEvent::TabCreated` 를 발행 — cascade 는 `CreateTab` 과 동형.
    AdoptTerminal { pane_id: u32, pty_id: u32 },

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
    /// 살아있는 surface 를 트리에서 떼어 다른 위치로 **이동(replace)** 한다 (T9).
    /// `source` 를 그 자리에서 떼어내(형제 끌어올림 / sole 이면 tab/pane/workspace
    /// cascade) `target` 위치의 leaf 를 대체하고, `target` 의 옛 surface 는 닫는다
    /// (PTY kill, closed-item 히스토리 미기록). source 의 Terminal/scrollback 은
    /// surface_id 불변이라 `TerminalStore` 가 자동으로 따라온다 — **이동 경로는
    /// source 에 대해 store/cleanup 을 절대 호출하지 않는다(PTY 보존).**
    MoveSurface {
        source_surface_id: u32,
        target_surface_id: u32,
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

    // ─── Surface completion (highlight producer) ───
    /// "이 surface 가 작업을 완료했다" 신호. highlight 를 발동하는 producer
    /// 중 하나(release 정식 IPC/CLI). cascade 가 surface 보유 engine 의
    /// `raise_surface_highlight` + redraw. surface_id 필수(포커스 독립 — 불가침
    /// 원칙 1). 향후 completion 고유 효과가 생기면 cascade 를 확장한다.
    SurfaceCompletion { surface_id: u32 },

    // ─── Closed items (D.3.C.D.5) ───
    /// closed_items stack top 을 pop 해 복원. `target_pane_id` 는 *호출자가
    /// 결정한* attach 대상 (focused pane). Workspace 복원 시에는 사용 안 함.
    /// `target_pane_id == None` 이면 (engine.workspaces 비어있는 상태에서
    /// Surface/Tab 을 복원 요청한 경우) 복원은 Workspace 인 경우만 가능.
    /// 이 경우 caller 가 사전에 ensure_workspace_exists 처리하는 것을 권장.
    RestoreClosedItem { target_pane_id: Option<u32> },

    // ─── Tab name (D.3.C.C.8) ───
    /// Terminal 의 OSC 0/2 title 변경 등으로 tab 표시명을 갱신. surface_id 가
    /// 속한 tab 을 모든 workspace 에서 찾아 `osc_title` 필드 set. explicit_name
    /// 은 *건드리지 않음* — 사용자가 직접 이름 지은 tab 의 이름은 OSC title 에
    /// 의해 덮어쓰여지지 않는다 (display_name 우선순위: explicit_name >
    /// osc_title > cached_display_name > name).
    UpdateTabName { surface_id: u32, name: String },

    // ─── Layout persistence (D.3.C.D.4) ───
    /// 현재 layout 을 ~/.tasty/layout.json 에 저장.
    /// - `active_workspace`: 호출자가 결정한 active workspace 인덱스 (AppState 가
    ///   들고 있는 정보이므로 Intent 발화 시 동봉).
    /// - `force=true`: shutdown 경로용. debounce 무시 + `restore_surface_content`
    ///   설정이 켜져 있으면 layout_dirty 가 false 여도 저장.
    /// - `force=false`: main loop tick 경로. debounce 통과 시에만 저장.
    ///
    /// settings.restore_layout=false 면 skip. cascade 없음. host event 없음.
    SaveLayoutNow {
        active_workspace: usize,
        force: bool,
    },

    /// `engine.pending_layout_restore` 를 take 해 live engine 으로 복원.
    /// 호출 전에 *호출자* 가 wait-for-plugin loop 를 끝내 둬야 함 — Intent 본문
    /// 안에서 plugin manager 를 못 만진다 (Core 의존 없음).
    /// pending_layout_restore 가 None 이면 no-op (`restored=false`).
    ApplyPendingLayoutRestore,

    // ─── File dispatch (D.3.C.G.3) ───
    /// 파일 dispatch 진입점. mouse ctrl+click / drag&drop / IPC `file_handler.dispatch`
    /// 가 발화. apply 분기에서 `engine.identify_worker.spawn(target, depth)` 호출 —
    /// Cheap/Deep 모두 worker thread 경유 (통일된 경로). 결과는
    /// `AppEvent::IdentifyDone` 으로 main thread 도착 후 `event_handler` 가
    /// `Core::apply_identify_result` Method 를 직접 호출. worker 미주입 시 drop
    /// + warn.
    DispatchFile {
        target: crate::file::format::FileTarget,
        depth: crate::file::format::DetectDepth,
        /// OpenSurface action 실행 시 이 surface 가 속한 *Pane* 에 새 tab 으로 추가.
        /// None 이면 focused pane 의 새 탭 (기존 동작).
        origin_surface_id: Option<u32>,
        /// true 면 대용량 markdown 확인 게이트를 건너뛰고 즉시 연다(에이전트/IPC
        /// 강제 열기, `02-md-ipc-size-bypass-flag`). 기본 false — 게이트(팝업) 적용.
        ignore_size_limit: bool,
    },
}

/// `Core::apply` 의 결과 — `handle_core_event` 가 소비해 도메인 cascade(설정 적용,
/// 워크스페이스/탭 생성, 알림 등)를 구동한다.
/// (관찰: observer/remote-attach 는 각자 별도 메커니즘으로 이미 완성돼 CoreEvent 를
/// 쓸 계획이 없고, replay 는 기능 자체가 미착수.)
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // reason: event queue 의 Box 화는 alloc/clone 비용 큼
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
    /// `pane_id` 는 닫힌 tab 이 속해 있던 pane (host event 발화용). 못 찾은
    /// 경우 `None`.
    TabClosed {
        tab_id: u32,
        pane_id: Option<u32>,
        closed: bool,
        cleanup_targets: Vec<(u32, Option<String>)>,
    },
    /// tab 이동 완료. `moved=false` 면 no-op (pane 없음 / from==to / out-of-range).
    TabMoved { moved: bool },

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
    /// `closed_tab_ids` / `closed_pane_ids` 는 cascade 가 host event
    /// (`tab.closed` / `pane.closed`) 를 발화할 때 사용 — Surface level 은 비어
    /// 있고, Tab level 은 닫힌 tab 1 개, Pane level 은 tab + pane, Workspace
    /// level 은 workspace 안 모든 tab + pane.
    SurfaceClosed {
        surface_id: u32,
        closed: bool,
        cascade_level: CascadeLevel,
        cleanup_targets: Vec<(u32, Option<String>)>,
        closed_tab_ids: Vec<u32>,
        closed_pane_ids: Vec<u32>,
        workspace_id_purged: Option<u32>,
        workspaces_now_empty: bool,
    },
    /// surface 변환 완료. `replaced=false` 면 surface 못 찾음 또는 변환 실패.
    SurfaceConverted { surface_id: u32, replaced: bool },
    /// surface 이동(replace) 완료 (T9). `moved=false` 면 self-ref / source 무효 /
    /// target 못 찾음 (no-op, 슬롯만 비움).
    ///
    /// `b_cleanup` 은 닫히는 target(B) 의 `(surface_id, scrollback_persist_id)` —
    /// cascade 가 `cleanup_surface`(PTY kill) + `surface.closed` host event 발화.
    /// 나머지 필드는 **source(A) 의 옛 자리** 가 sole 이라 구조적으로 닫힌
    /// tab/pane/workspace 의 cascade 정보 (split 안 이동이면 `Surface` level + 빈
    /// vec). A 의 surface 자체는 **cleanup 대상이 아니다**(이동이므로 살아있음).
    /// 의미상 `SurfaceClosed` 와 동일 cascade 를 재사용한다.
    MoveSurfaceApplied {
        moved: bool,
        b_cleanup: Option<(u32, Option<String>)>,
        cascade_level: CascadeLevel,
        closed_tab_ids: Vec<u32>,
        closed_pane_ids: Vec<u32>,
        workspace_id_purged: Option<u32>,
        workspaces_now_empty: bool,
    },
    /// terminal send 완료. `sent=false` 면 surface 가 terminal 이 아니거나 없음.
    SurfaceSent { sent: bool },
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

    // ─── Surface completion (highlight producer) ───
    /// Surface completion 신호 요청. cascade 가 surface 보유 engine 의
    /// `raise_surface_highlight` + redraw.
    SurfaceCompletionRequested { surface_id: u32 },

    // ─── Closed items (D.3.C.D.5) ───
    /// closed_items pop + 복원 완료. cascade 가 (Workspace kind 인 경우)
    /// active_workspace 를 새 인덱스로 옮긴다.
    /// - `restored=false`: closed_items 가 비었거나 rebuild 실패.
    /// - `kind`: 어떤 종류가 복원되었는지 + cascade 가 알아야 할 인덱스.
    ClosedItemRestored { restored: bool, kind: RestoredKind },

    // ─── Terminal cascade (D.3.C.C.8) — PTY emit 발화 ───
    /// PTY child process exit. cascade 가 hook 발화 + ProcessExited host event
    /// enqueue + closed_items snapshot 분류 + 후속 `DomainIntent::CloseSurface
    /// { save_snapshot: true }` 발행.
    TerminalProcessExited { surface_id: u32 },

    /// OSC 0/2 로 받은 terminal window title. cascade 가 SurfaceTitleChanged
    /// host event 발화 (plugin 호환) + 후속 `DomainIntent::UpdateTabName` 발행.
    TerminalTitleChanged { surface_id: u32, title: String },

    /// OSC 9 / OSC 99 / OSC 777 알림. cascade 가 settings.notification gate
    /// 적용 + `DomainIntent::PushNotification` 발행 + hook 발화.
    TerminalNotification {
        surface_id: u32,
        title: String,
        body: String,
    },

    /// Bell (\\a) 수신. cascade 가 settings.notification gate 적용 +
    /// `DomainIntent::PushNotification { title: "Bell" }` 발행 + hook 발화.
    TerminalBellRing { surface_id: u32 },

    /// PTY 출력이 완성된 한 라인을 이뤘다. cascade 가 등록된
    /// `OutputMatch` 훅과 이 라인 텍스트를 비교해 발화한다. `has_output_match_hook`
    /// 로 게이트돼 있어 이 surface 에 `OutputMatch` 훅이 없으면 애초에 발행되지
    /// 않는다.
    TerminalOutputMatch { surface_id: u32, text: String },

    /// OSC 7 cwd 변경. cascade 가 후속 `DomainIntent::SurfaceCwdChanged` 발행.
    TerminalCwdChanged { surface_id: u32 },

    /// OSC 133 D phase — 셸 통합이 명령 완료 + exit code 를 보고했다. cascade
    /// 가 exit code 무관하게 항상 surface highlight 를 발동하고(자동 경로), 동시에
    /// `HookEvent::CommandCompleted(exit_code)` 로 훅도 발화한다(커스터마이즈 경로 —
    /// 두 경로는 상호 배타적이지 않다. 상세: `docs/features/surface-highlight/index.md`).
    TerminalCommandCompleted {
        surface_id: u32,
        exit_code: Option<i32>,
    },

    /// 이 surface 가 출력을 내고 있는데도 일정 시간 `PromptBoundary` 를 한 번도
    /// 못 받았다 — OSC 133 셸 통합 미설치로 추정. cascade 가 안내 배너를
    /// 1 회 띄운다(자동 조치 없음).
    TerminalShellIntegrationHint { surface_id: u32 },

    /// OSC 52 clipboard set. cascade 가 `toast.copied_osc52` 토스트만 발행한다.
    /// 시스템 clipboard 쓰기는 Core::process_pty_output 이 self.clipboard 로 직접 처리한다.
    /// `surface_id` 는 토스트를 Surface 스코프로 띄우기 위함 (호스트가 stamp 한 실제 sid).
    TerminalClipboardSet { surface_id: u32 },

    /// `DomainIntent::UpdateTabName` 적용 결과. cascade 가 mark_dirty 만.
    /// `osc_title` 은 layout.json 영속 대상 아님 — mark_layout_dirty 호출 안 함.
    TabNameUpdated {
        /// `apply_update_tab_name` 의 explicit_name 보존 분기 여부 — production
        /// cascade 는 mark_dirty 만 하고 참조하지 않는다. 테스트 전용 관측 계약
        /// (`non_focused_surface_title_does_not_change_tab_name` 등이 assert).
        #[allow(dead_code)]
        skipped_explicit: bool,
    },

    // ─── Layout persistence (D.3.C.D.4) ───
    /// `SaveLayoutNow` 결과 알림 — 저장/skip(설정 off 또는 debounce 미만 + force=false)
    /// 여부와 무관하게 cascade 없음.
    LayoutSaved,

    /// `ApplyPendingLayoutRestore` 결과. `restored=true` 면 caller 가
    /// `active_workspace` 로 `state.switch_workspace` 수행. `restored=false` 면
    /// pending 없거나 schema 미스매치. cascade 없음 — caller 가 events 직접 검사.
    LayoutRestored {
        restored: bool,
        active_workspace: Option<usize>,
    },

    // ─── Plugin lifecycle (D.3.C.G.2) ───
    /// Plugin process 가 spawn 되어 hello 까지 완료. cascade 가
    /// PendingHostEvent::PluginLoaded enqueue + plugin event_bus broadcast.
    PluginLoaded { plugin_id: String, version: String },

    /// Plugin 활성화 상태 변경 (enable/disable). PluginManager.config 변경
    /// 직후 발화. `enabled=false` 인 경우 cascade 가 `PluginUnloaded` 도 함께
    /// 발화 (옛 lifecycle.rs:317 의 was_running 분기를 cascade 가 흡수).
    PluginEnableToggled { plugin_id: String, enabled: bool },

    /// Plugin process 가 graceful shutdown 또는 abnormal terminate.
    /// `reason` 은 `LifecycleReason::{User, Ipc, Crash}` 3종. 본 substep 에서는
    /// (결정 §7.2) 항상 `User` — caller context 추적 단순화.
    PluginUnloaded {
        plugin_id: String,
        reason: tasty_plugin_protocol::events::LifecycleReason,
    },

    /// Plugin 실패 — spawn/runtime/pump 등 모든 error 통합. cascade(`cascade_plugin_error`)
    /// 는 완전히 구현돼 host event + event_bus broadcast 까지 연결돼 있으나, 이 variant
    /// 를 실제로 construct 하는 producer 가 아직 없다(범위 밖이라 미착수. plugin
    /// spawn/runtime/pump 실패 지점에서 이 event 를 발화하도록 배선하는 게 다음
    /// 단계).
    #[allow(dead_code)]
    PluginError {
        plugin_id: String,
        error_kind: String,
        message: String,
    },

    /// Plugin 의 surface_kind 가 registry 에 등록됨. hello 처리 시 매 kind 마다
    /// 발화. cascade 가 PendingHostEvent 로 라우팅 (외부 가시성). `rendering` 은
    /// "remote" / "host" / "webview" 중 하나.
    PluginSurfaceKindRegistered {
        plugin_id: String,
        kind: String,
        rendering: String,
    },

    /// Plugin install / remove / grant / revoke 완료. 정적 상태 변경 (config /
    /// packages / permissions). cascade 가 host event 라우팅.
    PluginRegistryChanged {
        plugin_id: String,
        change: PluginRegistryChange,
    },

    /// Plugin manifest 의 `[[contributes.window]]` 항목이 hello 시점에 등록됨.
    /// 1.0 에서는 *stub 통지* — 실 spawn handler 는 별도 영역. cascade 가 host
    /// event (`plugin.window_declared`) 만 발화.
    PluginWindowDeclared {
        plugin_id: String,
        window_id: String,
    },
}

/// `CoreEvent::PluginRegistryChanged` 의 변경 종류.
#[derive(Debug, Clone)]
pub(crate) enum PluginRegistryChange {
    Installed { version: String },
    Removed,
    PermissionGranted { permission: String },
    PermissionRevoked { permission: String },
}

/// `CoreEvent::ClosedItemRestored` 의 복원 결과 분류.
#[derive(Debug, Clone)]
pub(crate) enum RestoredKind {
    /// 비어있는 스택 또는 rebuild 실패.
    Nothing,
    /// Workspace 복원 — cascade 가 `state.active_workspace = new_ws_index` 적용.
    Workspace { new_ws_index: usize },
    /// Surface 또는 Tab 이 기존 pane 의 tab 으로 attach 됨.
    /// cascade 가 별도 mutate 없이 mark_dirty 만 발화 (Core::apply 가 이미
    /// mark_layout_dirty 처리).
    TabIntoPane,
}

/// `CoreEvent::SurfaceClosed` 의 cascade 깊이 정보.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CascadeLevel {
    Surface,
    Tab,
    Pane,
    Workspace,
}

/// `Core::process_pty_output` 의 반환. PTY drain 의 부수효과를 *데이터로* 표현
/// — 호출자 (event_handler) 가 cascade dispatch + state queue 분배.
///
/// `events` 는 cascade dispatcher 가 처리할 CoreEvent.
#[derive(Debug, Default)]
pub(crate) struct ProcessPtyOutcome {
    pub events: Vec<CoreEvent>,
}
