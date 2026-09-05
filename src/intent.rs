// 이유: headless 빌드에선 호출 트리 (app::dispatch::intents) 가 cfg(gui) 로 가려져
// intent variant / 도메인 핸들러 / preset/capture 헬퍼가 미사용으로 잡힌다.
// 본질적으로 gui 어댑터의 API 면 + IPC handler 경유 후보이므로 *headless 한정*
// 으로 dead_code/unused_imports 를 침묵 — gui 빌드에서는 검사 그대로.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]
// 본 모듈의 `from_*` 메서드는 `From` trait 변환이 아니라 *intent 의 dispatch
// source 부착* 의미 (예: `intent.from_user_shortcut(id)` = "이 intent 는 사용자
// 단축키로 발화되었다고 표시"). 따라서 `self` 를 받는 것이 의도된 형태.
#![allow(clippy::wrong_self_convention)]

//! Host-internal action dispatch (Intent 큐).
//!
//! 설계: `docs/design/flows/action-dispatch.md`.
//!
//! 발화자는 `AppState::dispatch_intent`로 `DispatchedIntent`를 push만 한다.
//! 메인 루프의 `App::dispatch_pending_intents`가 drain 하여 도메인별 핸들러
//! (`intent::popup`, `intent::preset`, ...)로 분기한다. fire-and-forget.
//!
//! 본 모듈의 `from_*` 메서드는 `From` trait 변환이 아니라 *intent 의 dispatch
//! source 부착* 의미. `self` 를 받는 것이 의도된 형태이며
//! `clippy::wrong_self_convention` 은 모듈 단위로 허용한다.

pub mod closed_item;
pub(crate) mod headless;
pub mod pane;
pub mod popup;
pub mod preset;
pub mod preset_capture;
pub mod surface;
pub mod tab;
#[cfg(debug_assertions)]
pub mod watch;
pub mod workspace;

use crate::model::SplitDirection;
use crate::model::popup_kind::{PopupId, PopupScope};

pub use preset::ClonedPreset;

/// `Core::apply` 가 반환한 에러를 도메인 핸들러가 공통 처리한다. mirror(원격 attach
/// client) 워크스페이스에서 구조 변경을 시도해 거부된 경우
/// ([`crate::core::MirrorStructuralBlocked`]) 사용자에게 차단 toast 를 띄우고,
/// 그 외 에러는 `warn` 로그를 남긴다. `label` 은 로그용 컨텍스트(예: "SplitSurface").
///
/// mirror 구조 변경 forward 는 2단계에서 붙는다 — 현재(1단계)는 로컬 실행을 막고
/// 사용자에게 "원격 워크스페이스라 로컬 실행 불가" 를 알리는 데서 그친다.
pub fn report_apply_error(state: &mut crate::state::AppState, label: &str, err: &anyhow::Error) {
    if let Some(blocked) = err.downcast_ref::<crate::core::MirrorStructuralBlocked>() {
        // 2단계: forward 로 큐잉된 op 는 원격 실행 결과가 UX 를 결정한다 — 여기서 차단
        // toast 를 띄우지 않는다(성공 무음, 실패 시 App drain 이 forward 실패 toast).
        // forward 대상이 아닌 op(mirror↔local 경계를 넘는 move-surface 등)만 기존
        // 차단 toast.
        if !blocked.forwarded {
            #[cfg(feature = "gui")]
            state.toasts.push(
                crate::i18n::t("attach.toast.mirror_structural_blocked"),
                crate::model::toast_kind::ToastKind::Warning,
                crate::model::toast_kind::ToastScope::Window,
            );
        }
    } else {
        tracing::warn!("{label} failed: {err}");
    }
}

/// 발화된 Intent. 메인 루프 drain 까지 `AppState::pending_intents` 에 머문다.
#[derive(Debug, Clone)]
pub struct DispatchedIntent {
    pub body: Intent,
    pub origin: IntentOrigin,
    /// `Some` 이면 그대로 결과 envelope 에 전파, `None` 이면 bridge 가 새로 발급.
    pub trace_id: Option<String>,
}

/// 호스트 내부 명령. flat enum — variant 가 늘어나도 nested 하지 않는다.
///
/// **분류축** (D.3.I — `intent-ui-vs-domain.md`):
/// - `Ui(UiIntent)`: 사용자 시각 상태 변경 (popup open/close/toggle). headless
///   빌드에서는 컴파일 타임에 사라진다 (Phase E).
/// - 그 외 variant: Domain Intent — 영속 도메인 mutate. headless 빌드에서도 동작.
///
/// release 빌드에서 *시스템/Core/Domain handler 가 자동으로 `Ui` variant 를
/// 발화* 하는 것은 금지된다 (`docs/design/systems/popup.md` "Popup 발화 정책").
/// debug 빌드의 `debug.popup.*` IPC 만 예외.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // reason: hot intent queue 에 Box 화 시 alloc 비용 큼
pub enum Intent {
    /// UI Intent (popup 발화). 별 enum `UiIntent` 로 분리되어 분류축이 명시된다.
    Ui(UiIntent),
    /// Domain Intent (영속 도메인 mutate). `crate::core::intent::DomainIntent` 를
    /// 래핑하므로 같은 큐 (`pending_intents`) 로 발화 가능. dispatch_one_intent
    /// 에서 `core.apply` 경로로 분기된다.
    ///
    /// 마이그레이션 진행: 현재 도메인 variant (ApplyPreset / SavePreset / ...)
    /// 가 점진적으로 `DomainIntent` 안으로 흡수될 예정.
    Domain(crate::core::intent::DomainIntent),

    // ---- Preset 도메인 ----
    /// Preset 적용. focus 정책은 origin 으로 자동 분기 (User=true, Agent=false).
    ApplyPreset {
        kind: tasty_presets::PresetKind,
        name: String,
        /// Workspace preset 적용 시 소속시킬 카테고리. `None` 이면 normal(기본).
        /// 카테고리 헤더 우클릭 메뉴의 "프리셋으로부터 워크스페이스 생성" 이 그
        /// 카테고리 id 를 실어 보낸다. Tab/Pane preset 에는 의미 없음(무시).
        category: Option<crate::model::WorkspaceCategoryId>,
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

    // ---- Surface 도메인 ----
    /// focused surface 를 split. focused 의존이므로 사용자 단축키 전용 (CLI/IPC 미노출).
    SplitSurface { direction: SplitDirection },
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
        /// 생성 시점 카테고리 소속. `None` 이면 normal(기본). 레일 카테고리 팝업의
        /// "Add workspace" 가 해당 카테고리 id 를 실어 보낸다.
        category: Option<crate::model::WorkspaceCategoryId>,
    },

    // ---- Closed items 도메인 ----
    /// closed_items 스택 top 복원. focused pane 의존 (사용자 단축키 전용).
    /// handler 가 focused pane / workspace 비어있음 사전처리 후 DomainIntent 발화.
    RestoreClosedItem,
}

/// UI Intent — 사용자 시각 상태 변경. release 표면에서는 사용자 행동 (단축키 /
/// 마우스 / 메뉴) 에서만 발화되며, 자동 발화는 금지된다 (`popup-system.md`,
/// `toast-system.md`, `debug-ipc.md` 의 자매 정책 — `intent-ui-vs-domain.md` 2절).
///
/// Phase E 의 headless 빌드 (`--no-default-features` 또는 `feature = "gui"` off)
/// 에서는 본 enum 자체가 컴파일 타임에 사라질 예정 — 그때 `Intent::Ui` variant
/// 도 `#[cfg(feature = "gui")]` 가드된다. 본 commit 에서는 분류축 표시 + builder
/// 도입만, cfg 가드는 후속.
#[derive(Debug, Clone)]
pub enum UiIntent {
    /// popup 열기.
    OpenPopup { id: PopupId, mode: OpenPopupMode },
    /// popup 닫기.
    ClosePopup { id: PopupId },
    /// popup toggle (열려있으면 닫고, 닫혀있으면 열기).
    TogglePopup { id: PopupId, mode: OpenPopupMode },
    /// Theme 색상 또는 host UI zoom 배율이 바뀌었다. 모든 윈도우 (main + modal)
    /// 의 GpuState 가 전역 `Theme` 인스턴스를 재빌드 후 egui ctx 에 reapply
    /// 해야 한다. dispatcher 가 fan-out 처리.
    AppearanceChanged,
}

impl From<UiIntent> for Intent {
    fn from(ui: UiIntent) -> Self {
        Intent::Ui(ui)
    }
}

/// `UiIntent` 발화 ergonomics — `Intent` 의 builder 들을 그대로 갖춰 호출처가
/// `UiIntent::OpenPopup{...}.from_user_shortcut(...)` 형태로 발화할 수 있게 한다.
///
/// origin 분기 builder set — agent plugin / cli / cascade 발화 경로가 wiring
/// 전이라 일부 메서드 dead. 외부 호출처 추가 시 일관 set 이 필요하므로 보존.
impl UiIntent {
    pub fn from_user_shortcut(self, id: &'static str) -> DispatchedIntent {
        Intent::Ui(self).from_user_shortcut(id)
    }

    pub fn from_user_menu(self, id: &'static str) -> DispatchedIntent {
        Intent::Ui(self).from_user_menu(id)
    }

    pub fn from_user_context_menu(self) -> DispatchedIntent {
        Intent::Ui(self).from_user_context_menu()
    }

    pub fn from_agent_ipc(self) -> DispatchedIntent {
        Intent::Ui(self).from_agent_ipc()
    }

    /// agent plugin 발화 — `file_picker.trigger`(ADR-0058)가 실사용처.
    pub fn from_agent_plugin(self, plugin_id: impl Into<String>) -> DispatchedIntent {
        Intent::Ui(self).from_agent_plugin(plugin_id)
    }

    /// agent CLI 발화 경로 wiring 전 — 실사용처 없음.
    #[allow(dead_code)]
    pub fn from_agent_cli(self) -> DispatchedIntent {
        Intent::Ui(self).from_agent_cli()
    }

    /// cascade 발화 경로 wiring 전 — 실사용처 없음.
    #[allow(dead_code)]
    pub fn cascaded_from(self, parent: &DispatchedIntent) -> DispatchedIntent {
        Intent::Ui(self).cascaded_from(parent)
    }
}

/// `DomainIntent` 발화 ergonomics — `UiIntent` 와 동일 패턴. 단 `from_system()`
/// 은 *Domain 한정* 으로 본 impl 에만 존재한다 — UI Intent 의 자동 발화 차단.
///
/// origin 분기 builder set — context_menu / agent_plugin / agent_cli / cascade
/// 발화 경로가 wiring 전이라 일부 메서드 dead. 외부 호출처 추가 시 일관 set 이
/// 필요하므로 보존.
impl crate::core::intent::DomainIntent {
    pub(crate) fn from_user_shortcut(self, id: &'static str) -> DispatchedIntent {
        Intent::Domain(self).from_user_shortcut(id)
    }

    pub(crate) fn from_user_menu(self, id: &'static str) -> DispatchedIntent {
        Intent::Domain(self).from_user_menu(id)
    }

    pub(crate) fn from_user_context_menu(self) -> DispatchedIntent {
        Intent::Domain(self).from_user_context_menu()
    }

    pub(crate) fn from_agent_ipc(self) -> DispatchedIntent {
        Intent::Domain(self).from_agent_ipc()
    }

    /// agent plugin 발화 경로 wiring 전 — 실사용처 없음.
    #[allow(dead_code)]
    pub(crate) fn from_agent_plugin(self, plugin_id: impl Into<String>) -> DispatchedIntent {
        Intent::Domain(self).from_agent_plugin(plugin_id)
    }

    /// agent CLI 발화 경로 wiring 전 — 실사용처 없음.
    #[allow(dead_code)]
    pub(crate) fn from_agent_cli(self) -> DispatchedIntent {
        Intent::Domain(self).from_agent_cli()
    }

    /// 시스템 내부 cascade 발화 — PTY escape sequence 가 trigger 한 자동 cascade
    /// 등. UI Intent 의 system 발화는 type-level 로 차단되므로 본 method 는
    /// `DomainIntent` 에만 존재한다.
    pub(crate) fn from_system(self) -> DispatchedIntent {
        DispatchedIntent {
            body: Intent::Domain(self),
            origin: IntentOrigin::System,
            trace_id: None,
        }
    }

    /// cascade 발화 경로 wiring 전 — 실사용처 없음.
    #[allow(dead_code)]
    pub(crate) fn cascaded_from(self, parent: &DispatchedIntent) -> DispatchedIntent {
        Intent::Domain(self).cascaded_from(parent)
    }
}

/// Surface 변환 타깃. Terminal 은 host 내장 special case, 나머지는 surface_registry
/// 의 kind 로 통합. plugin 이 등록한 kind 도 모두 이 경로로 처리한다.
///
/// `Kind` 의 `cwd` 는 호출자가 명시 또는 None (handler 가 source surface 에서 resolve).
#[derive(Debug, Clone)]
pub enum ConvertTarget {
    Terminal,
    Kind {
        /// 변환 대상의 시작 cwd. None 이면 intent handler 가 source surface 로부터 resolve.
        cwd: Option<std::path::PathBuf>,
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
    /// 지정 위치 (context menu). egui::Pos2 — gui-only.
    #[cfg(feature = "gui")]
    AtFocused(egui::Pos2),
}

/// Intent 를 발화한 주체. 핸들러가 정책 분기에 사용.
///
/// `System` variant 는 *사용자도 에이전트도 아닌 시스템 내부 cascade* 를 표현 —
/// PTY 가 출력한 escape sequence 가 trigger 한 cascade (OSC 9 알림, OSC 7 cwd
/// 변경, OSC 52 클립보드 등) 와 같이 *사용자/에이전트의 직접 발화가 아닌
/// 자동 cascade*. focus 정책상 `User` 도 `Agent` 도 아닌 *제3 카테고리* —
/// focus 가져가지 않고, closed-tab restore 스택에도 push 하지 않는다 (기존
/// `is_user()` 가 false 인 경로와 동일 동작).
///
/// `System` 발화는 *Domain Intent 한정* — UI Intent (`Intent::Ui`) 의 자동
/// 발화는 release 표면에서 금지되므로 `UiIntent` 위에는 `from_system()` 을
/// 두지 않는다 (`popup-system.md` "Popup 발화 정책").
///
/// `source` 페이로드는 audit/debug trace 용 — match arm 에서 destructure 하지
/// 않으나 `Debug` derive 로 노출. D.3.C.F.3 Audit log 가 본 정보를 읽기
/// 시작 예정.
#[derive(Debug, Clone)]
pub enum IntentOrigin {
    User {
        #[allow(dead_code)]
        source: UserSource,
    },
    Agent {
        #[allow(dead_code)]
        source: AgentSource,
    },
    System,
}

/// 사용자 발화의 정확한 origin (shortcut id, menu id 등). 페이로드는 audit
/// trace 전용으로 destructure 되지 않으나 Debug 출력에 노출.
#[derive(Debug, Clone)]
pub enum UserSource {
    Shortcut(#[allow(dead_code)] &'static str),
    Menu(#[allow(dead_code)] &'static str),
    ContextMenu,
}

/// 에이전트 발화의 channel + plugin id. audit trace 용 — Plugin/Cli variant
/// 자체는 추후 plugin/CLI dispatch chain 통합 시 활성화 예정 (E.B 영역).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum AgentSource {
    Ipc,
    Plugin(String),
    Cli,
}

impl IntentOrigin {
    pub fn is_user(&self) -> bool {
        matches!(self, IntentOrigin::User { .. })
    }

    /// audit/branch helper — origin 분기 시 IntentOrigin 패턴 매칭 대신 사용.
    #[allow(dead_code)]
    pub fn is_agent(&self) -> bool {
        matches!(self, IntentOrigin::Agent { .. })
    }

    /// `is_agent` 와 짝인 audit/branch helper — origin 분기 호출처 추가 시 사용.
    #[allow(dead_code)]
    pub fn is_system(&self) -> bool {
        matches!(self, IntentOrigin::System)
    }
}

/// 발화 ergonomics. `UiIntent::OpenPopup { ... }.from_user_shortcut("id")` 또는
/// 도메인 variant 에서 `Intent::ApplyPreset { ... }.from_user_menu("id")` 형태.
///
/// origin 분기 builder set — agent plugin / cli / cascade 발화 경로가 wiring
/// 전이라 일부 메서드 dead. 외부 호출처 추가 시 일관 set 이 필요하므로 보존.
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

    /// agent plugin 발화 — `file_picker.trigger`(ADR-0058)가 실사용처.
    pub fn from_agent_plugin(self, plugin_id: impl Into<String>) -> DispatchedIntent {
        DispatchedIntent {
            body: self,
            origin: IntentOrigin::Agent {
                source: AgentSource::Plugin(plugin_id.into()),
            },
            trace_id: None,
        }
    }

    /// agent CLI 발화 경로 wiring 전 — 실사용처 없음.
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

    /// cascade: 직전 Intent 의 origin 을 명시적으로 전파. `trace_id` 도 그대로.
    /// 비-테스트 호출처 없음 — cfg(test) 에서만 직접 호출됨.
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
    /// `trace_id` 명시 지정 (IPC chain 등). Audit trace 용 helper — IPC
    /// 핸들러가 외부에서 발급한 trace_id 를 chain 시작 시점에 주입할 때 호출.
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
