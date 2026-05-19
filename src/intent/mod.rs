//! Host-internal action dispatch (Intent 큐).
//!
//! 설계: `docs/design/action-dispatch.md`.
//!
//! 발화자는 `AppState::dispatch_intent`로 `DispatchedIntent`를 push만 한다.
//! 메인 루프의 `App::dispatch_pending_intents`가 drain 하여 도메인별 핸들러
//! (`intent::popup`, `intent::preset`, ...)로 분기한다. fire-and-forget.

use crate::ui::popup::{PopupId, PopupScope};

pub mod popup;

#[cfg(debug_assertions)]
pub mod watch;

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

/// envelope `meta.origin` 표현. Event Bus 1.0 envelope 와 1:1 매핑.
///
/// User → host, Agent::Plugin(id) → plugin, 그 외 Agent → host.
#[allow(dead_code)] // TODO 03 이후 bridge 에서 사용.
pub fn envelope_origin(intent: &DispatchedIntent) -> serde_json::Value {
    match &intent.origin {
        IntentOrigin::User { .. } => serde_json::json!({ "kind": "host" }),
        IntentOrigin::Agent { source } => match source {
            AgentSource::Plugin(id) => serde_json::json!({ "kind": "plugin", "plugin_id": id }),
            AgentSource::Ipc | AgentSource::Cli => serde_json::json!({ "kind": "host" }),
        },
    }
}

/// envelope `meta.trace_id`. `Some` 이면 그대로, `None` 이면 새 ID 발급
/// (`PluginManager` 의 host event 패턴과 동일한 `i{n:x}` 형식).
#[allow(dead_code)] // TODO 03 이후 bridge 에서 사용.
pub fn envelope_trace_id(intent: &DispatchedIntent) -> String {
    intent.trace_id.clone().unwrap_or_else(new_trace_id)
}

#[allow(dead_code)] // TODO 03 이후 bridge 에서 사용.
fn new_trace_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("i{n:x}")
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
    fn envelope_origin_user_is_host() {
        let i = Intent::Noop.from_user_shortcut("x");
        let v = envelope_origin(&i);
        assert_eq!(v, serde_json::json!({ "kind": "host" }));
    }

    #[test]
    fn envelope_origin_plugin_is_plugin() {
        let i = Intent::Noop.from_agent_plugin("p1");
        let v = envelope_origin(&i);
        assert_eq!(v, serde_json::json!({ "kind": "plugin", "plugin_id": "p1" }));
    }

    #[test]
    fn envelope_origin_ipc_cli_is_host() {
        let ipc = Intent::Noop.from_agent_ipc();
        let cli = Intent::Noop.from_agent_cli();
        assert_eq!(envelope_origin(&ipc), serde_json::json!({ "kind": "host" }));
        assert_eq!(envelope_origin(&cli), serde_json::json!({ "kind": "host" }));
    }

    #[test]
    fn trace_id_some_preserved() {
        let i = Intent::Noop.from_user_shortcut("x").with_trace_id("abc");
        assert_eq!(envelope_trace_id(&i), "abc");
    }

    #[test]
    fn trace_id_none_generates_id() {
        let i = Intent::Noop.from_user_shortcut("x");
        let id = envelope_trace_id(&i);
        // `i<hex>` 형식.
        assert!(id.starts_with('i'));
        assert!(id.len() > 1);
    }

    #[test]
    fn cascade_preserves_origin_and_trace_id() {
        let parent = Intent::Noop.from_user_shortcut("approve").with_trace_id("t1");
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
