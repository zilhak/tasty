//! Host-internal action dispatch (Intent 큐).
//!
//! 설계: `docs/design/action-dispatch.md`.
//!
//! 발화자는 `AppState::dispatch_intent`로 `DispatchedIntent`를 push만 한다.
//! 메인 루프의 `App::dispatch_pending_intents`가 drain 하여 도메인별 핸들러
//! (`intent::popup`, `intent::preset`, ...)로 분기한다. fire-and-forget.

pub mod pane;
pub mod popup;
pub mod preset;
pub mod surface;
pub mod tab;
#[cfg(debug_assertions)]
pub mod watch;
pub mod workspace;

use crate::model::SplitDirection;
use crate::ui::popup::{PopupId, PopupScope};

pub use preset::ClonedPreset;

/// 발화된 Intent. 메인 루프 drain 까지 `AppState::pending_intents` 에 머문다.
#[derive(Debug, Clone)]
pub struct DispatchedIntent {
    pub body: Intent,
    pub origin: IntentOrigin,
    /// `Some` 이면 그대로 결과 envelope 에 전파, `None` 이면 bridge 가 새로 발급.
    pub trace_id: Option<String>,
}

/// 호스트 내부 명령. flat enum — variant 가 늘어나도 nested 하지 않는다.
#[derive(Debug, Clone)]
pub enum Intent {
    /// 첫 단계의 placeholder. 핸들러는 아무 일도 하지 않는다.
    Noop,
    /// popup 열기.
    OpenPopup { id: PopupId, mode: OpenPopupMode },
    /// popup 닫기.
    ClosePopup { id: PopupId },
    /// popup toggle (열려있으면 닫고, 닫혀있으면 열기).
    TogglePopup { id: PopupId, mode: OpenPopupMode },

    // ---- Preset 도메인 ----
    /// Preset 적용. focus 정책은 origin 으로 자동 분기 (User=true, Agent=false).
    ApplyPreset {
        kind: tasty_presets::PresetKind,
        name: String,
    },
    /// Preset 저장. `explicit_name` 우선, 없으면 `base_name` 으로 `store.unique_name`.
    /// User origin (우클릭) 은 보통 explicit_name=None + overwrite=false,
    /// Agent origin (IPC) 은 explicit_name=Some + overwrite 명시.
    SavePreset {
        base_name: String,
        explicit_name: Option<String>,
        overwrite: bool,
        preset: ClonedPreset,
    },
    /// Preset 삭제.
    DeletePreset {
        kind: tasty_presets::PresetKind,
        name: String,
    },
    /// Preset 이름 변경.
    RenamePreset {
        kind: tasty_presets::PresetKind,
        from: String,
        to: String,
    },

    // ---- Surface 도메인 ----
    /// focused surface 를 split. focused 의존이므로 사용자 단축키 전용 (CLI/IPC 미노출).
    SplitSurface { direction: SplitDirection },
    /// Surface 닫기. origin.is_user() 면 snapshot 푸시 (Undo 가능), Agent 면 no_snapshot.
    CloseSurface { surface_id: u32 },
    /// Surface 의 kind 변환. Terminal 은 host 내장, 그 외는 plugin 등록 kind.
    ConvertSurface {
        surface_id: u32,
        target: ConvertTarget,
    },

    // ---- Tab 도메인 ----
    /// 새 탭 추가. `kind` None 이면 "terminal" fallback.
    /// focused pane 에 추가 (사용자 동작). ID 명시 경로는 IPC handler 가 직접 처리.
    NewTab {
        kind: Option<String>,
        params: serde_json::Value,
    },
    /// 특정 tab 닫기 (ID 지정).
    CloseTab { tab_id: u32 },

    // ---- Pane 도메인 ----
    /// focused pane 을 split. 사용자 단축키 전용 (focused 의존).
    /// S3=B: ratio / focus 변경 API 는 Intent 미마이그레이션.
    SplitPane { direction: SplitDirection },

    // ---- Workspace 도메인 ----
    /// 새 워크스페이스 생성. `kind` None 이면 "terminal" fallback + active 전환
    /// (사용자 동작 경로). 명시 kind 지정 시 background 경로 (active 전환 없음).
    /// IPC `workspace.create` 는 sync return contract 가 필요하므로 직접 호출 유지.
    /// W1=B: ActivateWorkspace 는 focus 독립성 원칙으로 Intent 미마이그레이션.
    NewWorkspace {
        kind: Option<String>,
        params: serde_json::Value,
    },
}

/// Surface 변환 타깃. Terminal 은 host 내장 special case, 나머지는 surface_registry
/// 의 kind 로 통합. plugin 이 등록한 kind 도 모두 이 경로로 처리한다.
#[derive(Debug, Clone)]
pub enum ConvertTarget {
    Terminal,
    Kind {
        kind: String,
        params: serde_json::Value,
    },
}

/// popup open 위치/포커스 정책.
#[derive(Debug, Clone)]
pub enum OpenPopupMode {
    /// 위치 자유, focus 없음.
    Default,
    /// 화면 중앙 + (user origin 이면) focus.
    CenteredFocused,
    /// 특정 scope rect 기준 센터링.
    WithScope(PopupScope),
    /// scope 상단 정렬.
    AtTopOfScope(PopupScope),
    /// 지정 위치 (context menu).
    AtFocused(egui::Pos2),
}

/// Intent 를 발화한 주체. 핸들러가 정책 분기에 사용.
#[derive(Debug, Clone)]
pub enum IntentOrigin {
    User { source: UserSource },
    Agent { source: AgentSource },
}

#[derive(Debug, Clone)]
pub enum UserSource {
    Shortcut(&'static str),
    Menu(&'static str),
    ContextMenu,
}

#[derive(Debug, Clone)]
pub enum AgentSource {
    Ipc,
    Plugin(String),
    Cli,
}

impl IntentOrigin {
    pub fn is_user(&self) -> bool {
        matches!(self, IntentOrigin::User { .. })
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, IntentOrigin::Agent { .. })
    }
}

/// 발화 ergonomics. `Intent::OpenPopup { ... }.from_user_shortcut("id")` 형태.
impl Intent {
    pub fn from_user_shortcut(self, id: &'static str) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::User {
                source: UserSource::Shortcut(id),
            },
            trace_id: None,
        }
    }

    pub fn from_user_menu(self, id: &'static str) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::User {
                source: UserSource::Menu(id),
            },
            trace_id: None,
        }
    }

    pub fn from_user_context_menu(self) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::User {
                source: UserSource::ContextMenu,
            },
            trace_id: None,
        }
    }

    pub fn from_agent_ipc(self) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::Agent {
                source: AgentSource::Ipc,
            },
            trace_id: None,
        }
    }

    pub fn from_agent_plugin(self, plugin_id: impl Into<String>) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::Agent {
                source: AgentSource::Plugin(plugin_id.into()),
            },
            trace_id: None,
        }
    }

    pub fn from_agent_cli(self) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::Agent {
                source: AgentSource::Cli,
            },
            trace_id: None,
        }
    }

    /// cascade: 직전 Intent 의 origin 을 명시적으로 전파. `trace_id` 도 그대로.
    pub fn cascaded_from(self, parent: &DispatchedIntent) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: parent.origin.clone(),
            trace_id: parent.trace_id.clone(),
        }
    }
}

impl DispatchedIntent {
    /// `trace_id` 명시 지정 (IPC chain 등).
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    fn make_state() -> AppState {
        let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
        AppState::new(80, 24, waker).unwrap()
    }

    #[test]
    fn dispatch_intent_pushes_to_queue() {
        let mut state = make_state();
        state.dispatch_intent(Intent::Noop.from_user_shortcut("test"));
        assert_eq!(state.pending_intents.len(), 1);
    }

    #[test]
    fn take_pending_intents_clears_queue() {
        let mut state = make_state();
        state.dispatch_intent(Intent::Noop.from_user_shortcut("a"));
        state.dispatch_intent(Intent::Noop.from_user_shortcut("b"));
        let drained = state.take_pending_intents();
        assert_eq!(drained.len(), 2);
        assert!(state.pending_intents.is_empty());
    }

    #[test]
    fn origin_is_user_is_agent() {
        let user = Intent::Noop.from_user_shortcut("x");
        let agent = Intent::Noop.from_agent_ipc();
        assert!(user.origin.is_user());
        assert!(!user.origin.is_agent());
        assert!(agent.origin.is_agent());
        assert!(!agent.origin.is_user());
    }

    #[test]
    fn cascade_preserves_origin_and_trace_id() {
        let parent = Intent::Noop
            .from_user_shortcut("approve")
            .with_trace_id("t1");
        let child = Intent::Noop.cascaded_from(&parent);
        assert!(matches!(
            child.origin,
            IntentOrigin::User {
                source: UserSource::Shortcut("approve")
            }
        ));
        assert_eq!(child.trace_id.as_deref(), Some("t1"));
    }
}
