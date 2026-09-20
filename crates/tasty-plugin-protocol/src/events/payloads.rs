//! 1.0 이벤트 카탈로그의 페이로드 Rust 타입.
//!
//! 호스트는 이 타입을 만들어 `serde_json::to_value`로 직렬화한 뒤
//! [`super::EventEnvelope`]의 `payload`에 싣는다. Plugin은 envelope에서
//! payload를 꺼내 자기가 관심 있는 타입으로 `from_value`해 사용한다.
//!
//! 각 타입의 이벤트 키·발화 시점·scope·안정성 등급은
//! `docs/reference/event-catalog.md`가 SoT다.

use serde::{Deserialize, Serialize};

use super::LifecycleReason;

// ── Surface ──────────────────────────────────────────────────────────────────

/// `surface.created` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceCreated {
    pub surface_id: u32,
    pub kind: String,
    pub tab_id: u32,
    pub pane_id: u32,
    pub workspace_id: u32,
    pub created_by: SurfaceCreatedBy,
}

/// `surface.created`의 `created_by` 필드 — agent spawn과 user split을 구분.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceCreatedBy {
    /// 사용자가 UI로 split/탭 추가한 결과.
    User,
    /// plugin이 IPC로 spawn한 결과. `source_plugin`이 spawn 주체.
    Agent { source_plugin: String },
}

/// `surface.closed` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceClosed {
    pub surface_id: u32,
    pub kind: String,
    pub reason: LifecycleReason,
}

/// `surface.focused` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceFocused {
    pub surface_id: u32,
    pub prev_surface_id: Option<u32>,
}

/// `surface.resized` 페이로드. scope=Surface.
/// 호스트가 150ms leading+trailing 쓰로틀 적용 후 발화.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceResized {
    pub surface_id: u32,
    pub width_px: u32,
    pub height_px: u32,
}

/// `surface.title_changed` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SurfaceTitleChanged {
    pub surface_id: u32,
    pub title: String,
}

// ── Tab ──────────────────────────────────────────────────────────────────────

/// `tab.created` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TabCreated {
    pub tab_id: u32,
    pub pane_id: u32,
    pub workspace_id: u32,
    pub kind: String,
}

/// `tab.closed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TabClosed {
    pub tab_id: u32,
    pub pane_id: u32,
    pub reason: LifecycleReason,
}

/// `tab.focused` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TabFocused {
    pub tab_id: u32,
    pub pane_id: u32,
    pub prev_tab_id: Option<u32>,
}

/// `tab.moved` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TabMoved {
    pub tab_id: u32,
    pub from_pane: u32,
    pub to_pane: u32,
}

/// `tab.renamed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TabRenamed {
    pub tab_id: u32,
    pub title: String,
}

// ── Pane ─────────────────────────────────────────────────────────────────────

/// `pane.created` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaneCreated {
    pub pane_id: u32,
    pub parent_pane_group: Option<u32>,
    pub workspace_id: u32,
}

/// `pane.closed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaneClosed {
    pub pane_id: u32,
    pub reason: LifecycleReason,
}

/// `pane.split` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaneSplit {
    pub original_pane: u32,
    pub new_pane: u32,
    pub direction: SplitDirection,
}

/// pane 분할 방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

// ── Split (PaneGroup / SurfaceGroup) ─────────────────────────────────────────

/// `split.ratio_changed` 페이로드. scope=System.
/// 호스트가 150ms 쓰로틀 적용 후 발화. 드래그 시작·종료에는 무조건 1회씩.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SplitRatioChanged {
    pub group_id: u32,
    pub level: SplitLevel,
    pub ratio: f32,
}

/// split이 적용된 레이어 — pane-level 또는 surface-level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitLevel {
    Pane,
    Surface,
}

// ── Workspace ────────────────────────────────────────────────────────────────

/// `workspace.created` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceCreated {
    pub workspace_id: u32,
    pub window_id: u64,
    pub name: String,
}

/// `workspace.closed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceClosed {
    pub workspace_id: u32,
    pub reason: LifecycleReason,
}

/// `workspace.activated` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceActivated {
    pub workspace_id: u32,
    pub prev_workspace_id: Option<u32>,
}

/// `workspace.renamed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkspaceRenamed {
    pub workspace_id: u32,
    pub name: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
}

// ── Window (OS) ──────────────────────────────────────────────────────────────

/// `window.created` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowCreated {
    pub window_id: u64,
    pub kind: String,
    pub modality: WindowModality,
}

/// 윈도우 modality — 유비쿼터스 언어(`docs/concepts/ubiquitous-language.md`)와 일치.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowModality {
    Modeless,
    Modal,
}

/// `window.closed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowClosed {
    pub window_id: u64,
    pub reason: LifecycleReason,
}

/// `window.focused` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WindowFocused {
    pub window_id: u64,
}

// ── Plugin lifecycle ─────────────────────────────────────────────────────────

/// `plugin.loaded` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginLoaded {
    pub plugin_id: String,
    pub version: String,
}

/// `plugin.unloaded` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginUnloaded {
    pub plugin_id: String,
    pub reason: LifecycleReason,
}

/// `plugin.error` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginError {
    pub plugin_id: String,
    /// 자유 문자열 카테고리. 예: "spawn_failed", "handshake_rejected", "panicked".
    pub error_kind: String,
    pub message: String,
}

/// `plugin.enabled` / `plugin.disabled` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginEnableToggled {
    pub plugin_id: String,
}

// ── Extension lifecycle ──────────────────────────────────────────────────────

/// `extension.activated` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtensionActivated {
    pub extension_id: String,
    pub target_id: String,
}

/// `extension.pending` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtensionPending {
    pub extension_id: String,
    pub target_id: String,
    /// 자유 문자열. 예: "target_not_loaded", "version_incompatible".
    pub reason: String,
}

/// `extension.conflict` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExtensionConflict {
    pub extension_id: String,
    pub target_id: String,
    pub conflicting_id: String,
}

// ── Tool ─────────────────────────────────────────────────────────────────────

/// `tool.invoked` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolInvoked {
    pub tool_id: String,
    pub source: ToolSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolSource {
    Builtin,
    Plugin { plugin_id: String },
}

// ── Command (Option D) ───────────────────────────────────────────────────────

/// `command.invoked` 페이로드 — owner plugin에 unicast로 전달.
/// trigger=shortcut & scope=Surface인 경우 envelope scope=Surface, 그 외 System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandInvoked {
    pub plugin_id: String,
    pub command_id: String,
    pub scope: CommandScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_surface_id: Option<u32>,
    pub trigger: CommandTrigger,
}

/// command scope — 매니페스트의 `[[contributes.command]] scope`에 대응.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandScope {
    /// 어디서나 동작. 단축키는 조합키만 허용.
    Global,
    /// owner plugin이 만든 surface에 포커스가 있을 때만 동작. 단일 키 허용.
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandTrigger {
    Shortcut,
    Menu,
    Ipc,
}

/// `command.shortcut_changed` 페이로드 — 사용자가 설정창에서 매핑 변경 시 broadcast.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CommandShortcutChanged {
    pub plugin_id: String,
    pub command_id: String,
    /// 새 단축키. `None`이면 매핑 해제.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    /// 변경 전 단축키. `None`이면 새 매핑 추가.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_shortcut: Option<String>,
}

// ── IME ──────────────────────────────────────────────────────────────────────

/// `ime.composition_start` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImeCompositionStart {
    pub surface_id: u32,
}

/// `ime.composition_end` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ImeCompositionEnd {
    pub surface_id: u32,
    pub committed_text: String,
}

// ── Theme / Language ─────────────────────────────────────────────────────────

/// `theme.changed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThemeChanged {
    pub theme_id: String,
}

/// `language.changed` 페이로드. scope=System. UI 언어 변경.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LanguageChanged {
    /// BCP 47 언어 코드 또는 토큰 (예: "en", "ko", "ja").
    pub language_code: String,
}

// ── Notification / Hook ──────────────────────────────────────────────────────

/// `notification.created` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NotificationCreated {
    pub id: String,
    pub title: String,
    pub body: String,
    /// "host" 또는 plugin_id.
    pub source: String,
}

/// `notification.dismissed` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NotificationDismissed {
    pub id: String,
}

/// `hook.fired` 페이로드 — `tasty-hooks` 시스템의 fire 결과.
/// surface hook이면 scope=Surface(surface_id 있음), global hook이면 scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookFired {
    pub hook_id: String,
    /// `process-exit`, `bell`, `output-match:<pattern>`, `idle-timeout:<secs>`,
    /// `interval:<secs>`, `once:<secs>`, `file:<path>` 등.
    pub event_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<u32>,
    pub payload: serde_json::Value,
}

// ── Process (PTY) ────────────────────────────────────────────────────────────

/// `process.started` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessStarted {
    pub surface_id: u32,
    pub pid: u32,
    pub command: String,
}

/// `process.exited` 페이로드. scope=Surface.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProcessExited {
    pub surface_id: u32,
    /// OS exit code. 신호 종료 시 일부 플랫폼에서 음수 또는 `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

// ── Memory ───────────────────────────────────────────────────────────────────

/// `memory.changed` 페이로드. scope=System.
///
/// `tasty-memory` 의 regular 영역에서 put/delete/expire/scope cleanup 이
/// 일어날 때 호스트가 발화. **secret 영역 변경은 발화하지 않는다** — 다른
/// plugin 에 owner/key 정보를 누설하지 않기 위함.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryChanged {
    /// `surface:42`, `workspace:1`, `global` 등 scope token.
    pub scope: String,
    pub key: String,
    pub kind: MemoryChangeKind,
    /// 새 version (Created/Updated). Deleted/Expired 는 생략.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
}

/// `memory.changed` 의 변경 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryChangeKind {
    Created,
    Updated,
    Deleted,
    Expired,
}

// ── Agent ────────────────────────────────────────────────────────────────────

/// `agent.task_finished` 페이로드. scope=System.
///
/// **종결 전이만 싣는다.** `waiting`/`ready`/`running` 으로 들어가는 전이는 발화
/// 대상이 아니다 — 종결에는 모든 진입 경로가 지나는 단일 깔때기가 이미 있고
/// (`agent.task_await` 가 그것으로 깨어난다) 비종결에는 그런 자리가 없다. 둘을 같은
/// 키에 담으면 비종결 쪽이 조용히 빠진 피드가 되고, 소비자는 그 사실을 알 수 없다.
///
/// **실패 사유와 결과를 안 싣는다.** 그 문자열은 task 가 돌린 명령의 출력을 그대로
/// 담을 수 있고, 피드는 구독 권한만 있으면 누구나 받는다. 필요한 소비자는 `task_id`
/// 로 `agent.task_get` 을 부른다 — 그쪽에는 호출자 권한이 걸린다. 나중에 실어야
/// 하면 **옵션 필드 추가**라 기존 소비자를 안 깨뜨린다(반대 방향은 major 다).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentTaskFinished {
    /// 그 task 가 사는 workspace. **`meta.scope` 는 `system` 이다** — envelope 의
    /// scope 축은 `system`/`surface` 둘뿐이고, workspace 를 가리키는 기존 사건
    /// (`workspace.created` 등)이 모두 이 방식으로 낸다.
    pub workspace_id: u32,
    pub task_id: String,
    /// `succeeded` · `failed` · `cancelled` · `skipped` 넷 중 하나.
    pub state: String,
}

/// `agent.barrier_closed` 페이로드. scope=System.
///
/// barrier 가 요구 수를 채워 닫힌 순간. **시간 초과(`timed_out`)는 여기 안 실린다** —
/// 그쪽은 전이가 일어나는 순간이 없고 읽는 쪽이 시계를 견줄 때 도장이 찍힌다.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentBarrierClosed {
    pub workspace_id: u32,
    /// barrier 이름. 만든 쪽이 정하는 식별자다.
    pub name: String,
    /// 닫힐 때 요구된 신호 수.
    pub count_required: u32,
}

// ── System ───────────────────────────────────────────────────────────────────

/// `system.startup_complete` 페이로드. scope=System.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SystemStartupComplete {}

/// `system.shutdown_initiated` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SystemShutdownInitiated {
    /// 자유 문자열. 예: "user_quit", "os_shutdown", "panic".
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_created_round_trip() {
        let p = SurfaceCreated {
            surface_id: 1,
            kind: "terminal".into(),
            tab_id: 2,
            pane_id: 3,
            workspace_id: 4,
            created_by: SurfaceCreatedBy::Agent {
                source_plugin: "com.tasty.claude".into(),
            },
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["surface_id"], 1);
        assert_eq!(v["created_by"]["kind"], "agent");
        assert_eq!(v["created_by"]["source_plugin"], "com.tasty.claude");
        let back: SurfaceCreated = serde_json::from_value(v).unwrap();
        assert_eq!(back.surface_id, 1);
    }

    #[test]
    fn surface_closed_uses_lifecycle_reason() {
        let p = SurfaceClosed {
            surface_id: 9,
            kind: "terminal".into(),
            reason: LifecycleReason::Ipc,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["reason"], "ipc");
    }

    #[test]
    fn command_invoked_optional_surface_id_skipped_when_none() {
        let p = CommandInvoked {
            plugin_id: "com.example.x".into(),
            command_id: "open_popup".into(),
            scope: CommandScope::Global,
            source_surface_id: None,
            trigger: CommandTrigger::Shortcut,
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(!s.contains("source_surface_id"));
        assert!(s.contains("\"trigger\":\"shortcut\""));
        assert!(s.contains("\"scope\":\"global\""));
    }

    #[test]
    fn process_exited_optional_exit_code() {
        let p = ProcessExited {
            surface_id: 1,
            exit_code: None,
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(!s.contains("exit_code"));
    }

    /// 이 페이로드가 **안 싣기로 한 것**을 고정한다. 실패 사유·결과를 나중에 누가
    /// 편하다고 끼워 넣으면 피드가 명령 출력을 나르게 되고, 그것은 되돌릴 때
    /// major 가 된다(뺀 필드는 기존 소비자를 깨뜨린다).
    #[test]
    fn a_finished_task_carries_its_verdict_but_not_what_it_printed() {
        let p = AgentTaskFinished {
            workspace_id: 3,
            task_id: "t-1716800000123-7".into(),
            state: "failed".into(),
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"state\":\"failed\""), "{s}");
        assert!(!s.contains("error"), "실패 사유가 실렸다: {s}");
        assert!(!s.contains("result"), "결과가 실렸다: {s}");
        assert!(!s.contains("output"), "출력이 실렸다: {s}");
    }

    #[test]
    fn a_closed_barrier_names_itself_and_the_count_it_needed() {
        let p = AgentBarrierClosed {
            workspace_id: 1,
            name: "wave-1".into(),
            count_required: 4,
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back["name"], "wave-1");
        assert_eq!(back["count_required"], 4);
        assert_eq!(back["workspace_id"], 1);
    }
}
