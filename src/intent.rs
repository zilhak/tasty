// reason: from_*는 타입 변환 대신 요청 출처를 붙이는 빌더이므로 self를 받는다.
#![allow(clippy::wrong_self_convention)]

//! 호스트 내부 명령 큐.
//!
//! AppState::dispatch_intent로 넣은 명령을 App::dispatch_pending_intents가 꺼내
//! 각 도메인 핸들러에 전달한다. 호출자는 실행 결과를 기다리지 않는다.
//! 설계는 docs/design/flows/action-dispatch.md를 따른다.

#[cfg(all(test, feature = "gui"))]
mod apply_error_tests;
pub mod closed_item;
// 헤드리스용 큐 처리를 GUI 조합의 시험에서도 검증한다.
#[cfg(any(not(feature = "gui"), test))]
pub(crate) mod headless;
pub mod pane;
pub mod popup;
pub mod preset;
pub mod preset_capture;
pub mod surface;
pub mod tab;
#[cfg(all(debug_assertions, feature = "gui"))]
pub mod watch;
pub mod workspace;

use crate::model::SplitDirection;
use crate::model::popup_kind::{PopupId, PopupScope};

pub use preset::ClonedPreset;
// 도메인도 origin을 사용하므로 정의를 core에 두고 여기서는 재수출한다.
#[cfg(any(feature = "gui", test))]
pub use crate::core::origin::UserSource;
pub use crate::core::origin::{AgentSource, IntentOrigin};

/// Core::apply의 오류를 처리한다. mirror 구조 변경 차단과 철회된 kind 오류는
/// 사용자 요청일 때만 토스트로 알리고, 에이전트 요청은 로그로 남긴다.
/// 그 밖의 오류는 warn으로 기록한다. label은 로그에서 작업을 구분하는 이름이다.
/// 원격으로 전달한 에이전트 요청에는 실패 회신도 로그만 남기도록 표시한다.
/// 사용자 표시 규칙은 docs/design/systems/toast.md를 따른다.
pub fn report_apply_error(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    origin: &IntentOrigin,
    label: &str,
    err: &anyhow::Error,
) {
    crate::core::mark_last_forward_agent_origin(engine, err, origin);
    if let Some(blocked) = err.downcast_ref::<crate::core::MirrorStructuralBlocked>() {
        // 원격에 전달한 요청은 회신에서 실패를 처리하므로 여기서는 토스트를 띄우지 않는다.
        if blocked.forwarded {
            return;
        }
        if !origin.is_user() {
            tracing::warn!("{label} blocked on a mirror workspace (agent origin, no toast): {err}");
            return;
        }
        #[cfg(feature = "gui")]
        state.toasts.push(
            crate::i18n::t("attach.toast.mirror_structural_blocked"),
            crate::model::toast_kind::ToastKind::Warning,
            crate::model::toast_kind::ToastScope::Window,
        );
    } else if let Some(withdrawn) =
        err.downcast_ref::<crate::core::surface_registry::SurfaceKindWithdrawn>()
    {
        report_withdrawn_kind(state, origin, label, err, withdrawn);
    } else {
        tracing::warn!("{label} failed: {err}");
    }
}

// reason: 헤드리스에는 토스트 매니저가 없어 state를 사용하지 않는다.
#[cfg_attr(not(feature = "gui"), allow(unused_variables))]
fn report_withdrawn_kind(
    state: &mut crate::state::AppState,
    origin: &IntentOrigin,
    label: &str,
    err: &anyhow::Error,
    withdrawn: &crate::core::surface_registry::SurfaceKindWithdrawn,
) {
    if !origin.is_user() {
        tracing::warn!("{label} failed (agent origin, no toast): {err}");
        return;
    }
    tracing::info!(
        kind = %withdrawn.kind,
        plugin_id = %withdrawn.plugin_id,
        "{label} refused: the surface kind's plugin is off or has not reconnected"
    );
    #[cfg(feature = "gui")]
    state.toasts.push(
        crate::i18n::t_fmt2(
            "surface.kind_toast.withdrawn",
            &withdrawn.kind,
            &withdrawn.plugin_id,
        ),
        crate::model::toast_kind::ToastKind::Warning,
        crate::model::toast_kind::ToastScope::Window,
    );
}

/// 메인 루프가 처리할 때까지 AppState::pending_intents에 보관하는 명령.
#[derive(Debug, Clone)]
pub struct DispatchedIntent {
    pub body: Intent,
    pub origin: IntentOrigin,
    /// 현재 제품 코드에서는 None이며 debug intent 로그만 이 값을 읽는다.
    /// Event Bus의 trace_id와는 별개다. IPC 요청 번호는
    /// [`tasty_ipc::server::RequestSeq`]를 사용한다.
    pub trace_id: Option<String>,
}

/// 호스트 내부 명령. UI·도메인 명령과 사용자 단축키용 명령을 같은 큐에 담는다.
// 이유: 단축키·메뉴 전용 variant는 헤드리스에서 생성하지 않지만 큐 처리에서 열거한다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "only the gui raises the user-shortcut intents, so headless never builds them"
    )
)]
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // reason: hot intent queue 에 Box 화 시 alloc 비용 큼
pub enum Intent {
    Ui(UiIntent),
    /// 도메인 명령도 같은 큐에 넣고 Core::apply로 전달한다.
    Domain(crate::core::intent::DomainIntent),

    /// 사용자 요청일 때만 적용 후 포커스를 옮긴다.
    ApplyPreset {
        kind: tasty_presets::PresetKind,
        name: String,
        /// Workspace 프리셋의 소속 카테고리. None이면 normal이며 Tab/Pane에서는 무시한다.
        category: Option<crate::model::WorkspaceCategoryId>,
    },
    /// explicit_name을 우선 사용하고, 없으면 base_name으로 중복되지 않는 이름을 만든다.
    SavePreset {
        base_name: String,
        explicit_name: Option<String>,
        overwrite: bool,
        preset: ClonedPreset,
    },

    /// 포커스된 surface를 분할한다. 사용자 단축키용이며 IPC는 ID를 지정한다.
    SplitSurface {
        direction: SplitDirection,
    },
    /// Terminal은 호스트 내장 종류이며 나머지는 등록된 kind를 사용한다.
    ConvertSurface {
        surface_id: u32,
        target: ConvertTarget,
    },

    /// 포커스된 pane에 탭을 추가한다. kind가 None이면 terminal을 사용한다.
    NewTab {
        kind: Option<String>,
        params: serde_json::Value,
    },

    /// 포커스된 pane을 분할하는 사용자 단축키 명령.
    SplitPane {
        direction: SplitDirection,
    },

    /// kind가 None이면 terminal을 사용한다. 사용자 요청일 때만 새 워크스페이스를 활성화한다.
    /// IPC workspace.create는 동기 응답을 위해 직접 처리한다.
    NewWorkspace {
        kind: Option<String>,
        params: serde_json::Value,
        /// 새 워크스페이스의 소속 카테고리. None이면 normal이다.
        category: Option<crate::model::WorkspaceCategoryId>,
    },

    /// 최근 닫은 항목을 포커스된 pane에 복원한다. 필요한 워크스페이스는 먼저 만든다.
    RestoreClosedItem,
}

/// 팝업과 테마 등 화면 상태를 바꾸는 명령.
// 이유: 팝업 입력은 GUI에서 발생하며 헤드리스에서는 해당 variant를 만들지 않는다.
#[cfg_attr(
    not(feature = "gui"),
    expect(dead_code, reason = "only user input in the gui raises a popup intent")
)]
#[derive(Debug, Clone)]
pub enum UiIntent {
    OpenPopup {
        id: PopupId,
        mode: OpenPopupMode,
    },
    ClosePopup {
        id: PopupId,
    },
    TogglePopup {
        id: PopupId,
        mode: OpenPopupMode,
    },
    /// 테마·UI 배율 변경을 모든 main/modal 창의 GPU 상태와 egui에 반영한다.
    AppearanceChanged,
}

impl From<UiIntent> for Intent {
    fn from(ui: UiIntent) -> Self {
        Intent::Ui(ui)
    }
}

impl UiIntent {
    #[cfg(feature = "gui")]
    pub fn from_user_shortcut(self, id: &'static str) -> DispatchedIntent {
        Intent::Ui(self).from_user_shortcut(id)
    }

    #[cfg(any(feature = "gui", test))]
    pub fn from_user_menu(self, id: &'static str) -> DispatchedIntent {
        Intent::Ui(self).from_user_menu(id)
    }

    #[cfg(feature = "gui")]
    pub fn from_user_context_menu(self) -> DispatchedIntent {
        Intent::Ui(self).from_user_context_menu()
    }

    #[cfg(feature = "gui")]
    pub fn from_agent_ipc(self) -> DispatchedIntent {
        Intent::Ui(self).from_agent_ipc()
    }

    #[cfg(feature = "gui")]
    pub fn from_agent_plugin(self, plugin_id: impl Into<String>) -> DispatchedIntent {
        Intent::Ui(self).from_agent_plugin(plugin_id)
    }

    // reason: 요청 출처별 빌더를 제공하지만 현재 CLI 호출부는 없다.
    #[allow(dead_code)]
    pub fn from_agent_cli(self) -> DispatchedIntent {
        Intent::Ui(self).from_agent_cli()
    }

    // reason: origin을 이어받는 빌더이며 현재 호출부는 없다.
    #[allow(dead_code)]
    pub fn cascaded_from(self, parent: &DispatchedIntent) -> DispatchedIntent {
        Intent::Ui(self).cascaded_from(parent)
    }
}

// 시스템 요청용 빌더는 DomainIntent에만 둔다.
impl crate::core::intent::DomainIntent {
    #[cfg(feature = "gui")]
    pub(crate) fn from_user_shortcut(self, id: &'static str) -> DispatchedIntent {
        Intent::Domain(self).from_user_shortcut(id)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn from_user_menu(self, id: &'static str) -> DispatchedIntent {
        Intent::Domain(self).from_user_menu(id)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn from_user_context_menu(self) -> DispatchedIntent {
        Intent::Domain(self).from_user_context_menu()
    }

    pub(crate) fn from_agent_ipc(self) -> DispatchedIntent {
        Intent::Domain(self).from_agent_ipc()
    }

    // reason: 요청 출처별 빌더를 제공하지만 현재 플러그인 호출부는 없다.
    #[allow(dead_code)]
    pub(crate) fn from_agent_plugin(self, plugin_id: impl Into<String>) -> DispatchedIntent {
        Intent::Domain(self).from_agent_plugin(plugin_id)
    }

    // reason: 요청 출처별 빌더를 제공하지만 현재 CLI 호출부는 없다.
    #[allow(dead_code)]
    pub(crate) fn from_agent_cli(self) -> DispatchedIntent {
        Intent::Domain(self).from_agent_cli()
    }

    pub(crate) fn from_system(self) -> DispatchedIntent {
        DispatchedIntent {
            body: Intent::Domain(self),
            origin: IntentOrigin::System,
            trace_id: None,
        }
    }

    // reason: origin을 이어받는 빌더이며 현재 호출부는 없다.
    #[allow(dead_code)]
    pub(crate) fn cascaded_from(self, parent: &DispatchedIntent) -> DispatchedIntent {
        Intent::Domain(self).cascaded_from(parent)
    }
}

/// Surface 변환 대상. Terminal 외에는 surface_registry에 등록된 kind를 사용한다.
// 이유: 변환을 요청하는 팝업과 메뉴는 GUI에만 있다.
#[cfg_attr(
    not(feature = "gui"),
    expect(
        dead_code,
        reason = "only the gui converts a surface through an intent"
    )
)]
#[derive(Debug, Clone)]
pub enum ConvertTarget {
    Terminal,
    Kind {
        /// 시작 cwd. None이면 기존 surface에서 구한다.
        cwd: Option<std::path::PathBuf>,
        kind: String,
        params: serde_json::Value,
    },
}

/// 팝업 위치와 포커스 정책.
// 이유: 팝업 열기는 GUI 입력에서만 요청한다.
#[cfg_attr(
    not(feature = "gui"),
    expect(dead_code, reason = "only user input in the gui opens a popup")
)]
#[derive(Debug, Clone)]
pub enum OpenPopupMode {
    /// 위치를 지정하지 않고 포커스도 옮기지 않는다.
    Default,
    /// 화면 중앙에 열고 포커스를 옮긴다.
    CenteredFocused,
    /// 지정한 범위의 중앙에 연다.
    WithScope(PopupScope),
    /// 지정한 범위의 상단에 연다.
    AtTopOfScope(PopupScope),
    /// 컨텍스트 메뉴에서 지정한 위치에 연다.
    #[cfg(feature = "gui")]
    AtFocused(egui::Pos2),
}

impl Intent {
    #[cfg(any(feature = "gui", test))]
    pub fn from_user_shortcut(self, id: &'static str) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::User {
                source: UserSource::Shortcut(id),
            },
            trace_id: None,
        }
    }

    #[cfg(any(feature = "gui", test))]
    pub fn from_user_menu(self, id: &'static str) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::User {
                source: UserSource::Menu(id),
            },
            trace_id: None,
        }
    }

    #[cfg(feature = "gui")]
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

    // reason: 요청 출처별 빌더를 제공하지만 현재 CLI 호출부는 없다.
    #[allow(dead_code)]
    pub fn from_agent_cli(self) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::Agent {
                source: AgentSource::Cli,
            },
            trace_id: None,
        }
    }

    /// 이전 명령의 origin과 trace_id를 그대로 이어받는다.
    // reason: 제품 코드에는 호출부가 없고 시험에서만 사용한다.
    #[allow(dead_code)]
    pub fn cascaded_from(self, parent: &DispatchedIntent) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: parent.origin.clone(),
            trace_id: parent.trace_id.clone(),
        }
    }
}

impl DispatchedIntent {
    /// 로그를 연결할 trace_id를 지정한다. IPC 요청 번호와는 별개다.
    // reason: 제품 코드에는 호출부가 없고 시험에서만 사용한다.
    #[allow(dead_code)]
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
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).unwrap();
        let preset_store = std::sync::Arc::new(std::sync::Mutex::new(
            tasty_presets::PresetStore::load_default(),
        ));
        let memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> =
            std::sync::Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            ));
        AppState::new(&mut engine, preset_store, memory)
    }

    #[test]
    fn dispatch_intent_pushes_to_queue() {
        let mut state = make_state();
        state.dispatch_intent(Intent::RestoreClosedItem.from_user_shortcut("test"));
        assert_eq!(state.pending_intents.len(), 1);
    }

    #[test]
    fn take_pending_intents_clears_queue() {
        let mut state = make_state();
        state.dispatch_intent(Intent::RestoreClosedItem.from_user_shortcut("a"));
        state.dispatch_intent(Intent::RestoreClosedItem.from_user_shortcut("b"));
        let drained = state.take_pending_intents();
        assert_eq!(drained.len(), 2);
        assert!(state.pending_intents.is_empty());
    }

    #[test]
    fn origin_is_user_is_agent() {
        let user = Intent::RestoreClosedItem.from_user_shortcut("x");
        let agent = Intent::RestoreClosedItem.from_agent_ipc();
        assert!(user.origin.is_user());
        assert!(!user.origin.is_agent());
        assert!(agent.origin.is_agent());
        assert!(!agent.origin.is_user());
    }

    #[test]
    fn appearance_changed_intent_pushes_to_queue() {
        let mut state = make_state();
        state.dispatch_intent(
            UiIntent::AppearanceChanged.from_user_menu("settings.appearance.changed"),
        );
        assert_eq!(state.pending_intents.len(), 1);
        let drained = state.take_pending_intents();
        assert!(matches!(
            drained[0].body,
            Intent::Ui(UiIntent::AppearanceChanged)
        ));
    }

    #[test]
    fn cascade_preserves_origin_and_trace_id() {
        let parent = Intent::RestoreClosedItem
            .from_user_shortcut("approve")
            .with_trace_id("t1");
        let child = Intent::RestoreClosedItem.cascaded_from(&parent);
        assert!(matches!(
            child.origin,
            IntentOrigin::User {
                source: UserSource::Shortcut("approve")
            }
        ));
        assert_eq!(child.trace_id.as_deref(), Some("t1"));
    }
}
