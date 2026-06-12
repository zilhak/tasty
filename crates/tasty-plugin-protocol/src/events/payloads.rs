//! 1.0 이벤트 카탈로그의 페이로드 Rust 타입.
//!
//! 호스트는 이 타입을 만들어 `serde_json::to_value`로 직렬화한 뒤
//! [`super::EventEnvelope`]의 `payload`에 싣는다. Plugin은 envelope에서
//! payload를 꺼내 자기가 관심 있는 타입으로 `from_value`해 사용한다.
//!
//! 각 타입의 이벤트 키·발화 시점·scope·안정성 등급은
//! `docs/agent-guide/event-catalog.md`가 SoT다.

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

// ── Clipboard (OS) ───────────────────────────────────────────────────────────

/// `clipboard.copied` 페이로드. scope=System.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClipboardCopied {
    pub kind: ClipboardKind,
    /// kind=`Text`일 때만 채워짐.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// kind=`Image`일 때만 채워짐. base64-encoded PNG.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_b64: Option<String>,
    /// UTC unix milliseconds.
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    Text,
    Image,
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
    fn clipboard_copied_image_variant() {
        let p = ClipboardCopied {
            kind: ClipboardKind::Image,
            text: None,
            image_b64: Some("base64==".into()),
            timestamp_ms: 1234567890,
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(!s.contains("\"text\""));
        assert!(s.contains("\"image_b64\":\"base64==\""));
        assert!(s.contains("\"kind\":\"image\""));
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
}
