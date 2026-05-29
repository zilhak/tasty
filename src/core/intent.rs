//! `CoreIntent` — `Core::apply` 의 입력. 도메인 변경 요청.
//!
//! 기존 `crate::intent::Intent` 와 *역할이 다르다*:
//! - `Intent` (AppState dispatch): UI + 도메인 혼합. AppState.pending_intents 큐로 push.
//! - `CoreIntent` (Core::apply): *순수 도메인 mutate*. handler 가 read 후 발행, dispatcher 가 drain.
//!
//! Phase D 진행 중. variant 는 점진 추가된다.

use tasty_settings::Settings;

/// 도메인 변경 요청. Core 만이 자기 메서드로 적용한다.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum CoreIntent {
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
}
