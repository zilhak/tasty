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

use tasty_settings::Settings;

/// 도메인 변경 요청. Core 만이 자기 메서드로 적용한다.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum DomainIntent {
    // ─── Settings (D.3.C.A.2) ───
    /// Settings 전체 교체. cascade — Theme apply / Scrollback limit / clipboard
    /// max / notification coalesce 가 Core 내부에서 자동 발동.
    UpdateSettings(Settings),

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

    // ─── Notifications (D.3.C.E.2) ───
    /// 알림 push 요청. cascade 가 라우팅 + store.add + host event enqueue.
    NotificationPushRequested {
        ws_id: u32,
        surface_id: u32,
        title: String,
        body: String,
        source: String,
    },

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
