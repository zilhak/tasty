// headless 빌드에선 호출 트리 (app::dispatch/intents) 가 cfg(gui) 로 가려져
// state 의 gui 전용 필드/메서드가 미사용으로 잡힌다. 본질적으로 gui 어댑터의
// API 면이므로 *headless 한정* 으로 dead_code/unused_imports 를 침묵시킨다.
// gui 빌드에서는 검사 그대로 작동.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

mod accessors;
mod detect;
mod focus;
// gui 전용 상태(popup/모달/스테이지)를 단정하는 테스트라 headless 빌드에는
// 대상 자체가 없다. `#[cfg(test)]` 만 걸면 `--no-default-features` 테스트 빌드가
// 통째로 깨진다 — `docs/dev-guide/unit-test-isolation.md` "feature 별 테스트 게이팅".
#[cfg(all(test, feature = "gui"))]
mod fullscreen_stage_tests;
mod layout;
mod mark;
pub mod mouse;
pub(crate) mod pane;
// gui 전용 상태(popup/모달/스테이지)를 단정하는 테스트라 headless 빌드에는
// 대상 자체가 없다. `#[cfg(test)]` 만 걸면 `--no-default-features` 테스트 빌드가
// 통째로 깨진다 — `docs/dev-guide/unit-test-isolation.md` "feature 별 테스트 게이팅".
#[cfg(all(test, feature = "gui"))]
mod popup_close_tests;
// gui 전용 상태(popup/모달/스테이지)를 단정하는 테스트라 headless 빌드에는
// 대상 자체가 없다. `#[cfg(test)]` 만 걸면 `--no-default-features` 테스트 빌드가
// 통째로 깨진다 — `docs/dev-guide/unit-test-isolation.md` "feature 별 테스트 게이팅".
#[cfg(all(test, feature = "gui"))]
mod popup_ownership_tests;
mod tab;
#[cfg(test)]
pub(crate) mod tests;
mod workspace;

pub mod command_palette;
pub mod preset_apply;
pub mod search;
pub mod selection;

pub use workspace::WorkspaceCloseOrigin;

use std::collections::VecDeque;

#[cfg(feature = "gui")]
use crate::adapters::ui::info_modal::InfoModal;
#[cfg(feature = "gui")]
use crate::adapters::ui::popup::transfer::{TransferError, TransferProgress};
use crate::core::CoreState;
use crate::model::{LogicalPx, PhysicalPx};
#[cfg(feature = "gui")]
use crate::settings_ui::SettingsUiState;

/// Type of the currently focused surface, used for keyboard routing.
///
/// Terminal은 PTY 입출력 경로가 별도라 빠른 분기 위해 전용 variant로 둔다.
/// 나머지는 surface kind 식별자 기반의 `Kind(String)`으로 일반화 — 외부 plugin도
/// 추가 enum 변경 없이 동작한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusedSurfaceType {
    None,
    Terminal,
    Kind(String),
}

impl FocusedSurfaceType {
    /// 이 surface가 주어진 kind 식별자에 해당하는지 검사.
    pub fn is_kind(&self, kind: &str) -> bool {
        matches!(self, Self::Kind(k) if k == kind)
    }

    /// registry 에서 이 surface kind 의 capability flag 를 조회한다. `Terminal`/`None`
    /// 은 kind 문자열이 아니므로 항상 false. host 가 `kind == "..."` 하드코딩 대신
    /// plugin/builtin 이 선언한 capability 로 게이트를 판정하게 한다.
    pub fn kind_capability(
        &self,
        engine: &CoreState,
        f: impl Fn(&crate::core::surface_registry::SurfaceKindDef) -> bool,
    ) -> bool {
        match self {
            Self::Kind(k) => engine
                .surface_registry
                .get(k)
                .map(|d| f(&d))
                .unwrap_or(false),
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SurfaceMessage {
    pub(crate) id: u32,
    pub(crate) from_surface_id: u32,
    pub(crate) content: String,
}

/// Surface가 닫혔다는 사실을 plugin 측에 broadcast하기 위해 메인 루프가 소비할
/// 큐 항목. `state/`는 `plugin/` 의존이 없으므로 enum 대신 `is_user_close: bool`로
/// reason을 담고, App 메인 루프에서 `SurfaceCloseReason`으로 매핑한다.
#[derive(Debug, Clone)]
pub struct PendingSurfaceClosed {
    pub(crate) surface_id: u32,
    /// kind 가 None 인 경우는 cascade close 경로에서 surface 가 이미 layout 에서
    /// 제거된 뒤 enqueue 되어 식별이 불가능했음을 의미. payload 변환 시 빈 문자열로
    /// 폴백한다 — 구독자(예: plugin-claude)는 surface_id 만으로 cleanup 가능.
    pub(crate) kind: Option<&'static str>,
    pub(crate) is_user_close: bool,
}

/// Event Bus 1.0 호스트 자동 발화용 큐 항목. `state/`가 `plugin/`/`tasty-plugin-protocol`
/// 의존을 갖지 않게, payload 필드는 wire 타입이 아닌 plain 데이터로 보관하고 App
/// 메인 루프가 [`tasty_plugin_protocol`] 타입으로 변환해 발화한다.
#[derive(Debug, Clone)]
pub enum PendingHostEvent {
    SurfaceFocused {
        surface_id: u32,
        prev_surface_id: Option<u32>,
    },
    SurfaceTitleChanged {
        surface_id: u32,
        title: String,
    },
    SurfaceCreated {
        surface_id: u32,
        kind: &'static str,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        /// `None`이면 user-initiated, `Some(plugin_id)`면 agent(plugin)이 spawn한 결과.
        created_by_plugin: Option<String>,
    },
    WorkspaceActivated {
        workspace_id: u32,
        prev_workspace_id: Option<u32>,
    },
    /// 이름/부제/설명 중 변경된 필드만 `Some`. 호스트 발화 측 어디서나 partial
    /// update가 가능하도록 모두 Optional로 둔다.
    ///
    /// `user_direct=true`면 사용자가 GUI 다이얼로그로 직접 변경한 케이스. Lua
    /// hook 의 `workspace.change.post` 는 user_direct 만 발화한다 (observe-only
    /// 단계 명세). IPC/CLI 경유 변경은 false 로 들어와 plugin 이벤트 버스만 받는다.
    WorkspaceRenamed {
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
        user_direct: bool,
    },
    TabFocused {
        tab_id: u32,
        pane_id: u32,
        prev_tab_id: Option<u32>,
    },
    /// Tab 이름 변경. `user_direct=true`면 사용자가 GUI 다이얼로그로 직접 rename
    /// 한 케이스 — Lua `tab.change.post` hook 은 user_direct 만 발화한다.
    TabRenamed {
        tab_id: u32,
        title: String,
        user_direct: bool,
    },
    /// 자식 프로세스 종료. exit_code는 현재 terminal 이벤트가 노출하지 않아 `None` 고정.
    ProcessExited {
        surface_id: u32,
    },
    /// `NotificationStore::add` 결과. source는 발화 측에서 채워 push (host=`"host"`,
    /// plugin=plugin_id).
    NotificationCreated {
        id: u64,
        title: String,
        body: String,
        source: String,
    },
    /// Tab 생성. `detect_tab_lifecycle` polling으로 발견.
    TabCreated {
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    },
    /// Tab 종료. polling이 사라진 tab_id를 발견하면 마지막 위치로 enqueue.
    /// 현재 reason은 항상 User (PR 5의 caller context 도입 이후 Ipc 구분 예정).
    TabClosed {
        tab_id: u32,
        pane_id: u32,
    },
    /// Tab이 다른 pane으로 이동. polling diff로 감지.
    TabMoved {
        tab_id: u32,
        from_pane: u32,
        to_pane: u32,
    },
    /// Pane 생성. polling으로 감지. `parent_pane_group`은 트리 구조 노출 비용이
    /// 커 현재 `None` 고정 (필요해지면 PR 5 이후 확장).
    PaneCreated {
        pane_id: u32,
        workspace_id: u32,
    },
    /// Pane 종료. polling이 사라진 pane_id를 발견하면 발화.
    /// reason은 현재 항상 `User` (caller context 구분은 PR 5에서).
    PaneClosed {
        pane_id: u32,
    },
    /// Workspace 생성. polling으로 감지. `window_id`는 caller가 전달.
    WorkspaceCreated {
        workspace_id: u32,
        window_id: u64,
        name: String,
    },
    /// Workspace 종료. reason은 현재 항상 `User`.
    WorkspaceClosed {
        workspace_id: u32,
    },
    /// Pane 분할. polling으로는 direction을 알 수 없어 호출 사이트에서 직접 enqueue.
    PaneSplit {
        original_pane: u32,
        new_pane: u32,
        direction: crate::model::SplitDirection,
    },
    /// `tasty-hooks`의 surface hook 발화. `check_and_fire` 호출자가 fired hook_id
    /// 리스트와 매칭된 event를 묶어 enqueue. surface_id가 0이면 global hook.
    HookFired {
        hook_id: u64,
        event_kind: String,
        surface_id: u32,
        /// 실제 관측된 exit code — `CommandCompleted` 발화일 때만
        /// `Some`. `resolve_hook_fired_task_waits` 가 push 완료 전략의 성공/실패
        /// 판정에 쓴다(exit 0 → Succeeded, 비-0 → Failed). 다른 이벤트는 `None`.
        exit_code: Option<i32>,
    },
    // ─── Plugin lifecycle (D.3.C.G.2) ───
    /// Plugin spawn 성공 후 hello 까지 완료.
    PluginLoaded {
        plugin_id: String,
        version: String,
    },
    /// Plugin 활성화 상태 변경 (enable=true / disable=false).
    PluginEnableToggled {
        plugin_id: String,
        enabled: bool,
    },
    /// Plugin process 가 종료됨. `reason` 은 LifecycleReason 의 serde rename
    /// (snake_case) — "user" / "ipc" / "crash".
    PluginUnloaded {
        plugin_id: String,
        reason: String,
    },
    /// Plugin spawn 실패 또는 runtime error.
    PluginError {
        plugin_id: String,
        error_kind: String,
        message: String,
    },
    /// Plugin install / remove / grant / revoke 완료. `change_kind` 는
    /// "installed" / "removed" / "permission_granted" / "permission_revoked".
    PluginRegistryChanged {
        plugin_id: String,
        change_kind: String,
        detail: serde_json::Value,
    },
    /// Plugin 의 surface_kind 가 hello 처리 직후 registry 에 등록됨.
    PluginSurfaceKindRegistered {
        plugin_id: String,
        kind: String,
        rendering: String,
    },
    /// Plugin manifest 의 `[[contributes.window]]` 항목이 hello 시점에 등록됨.
    /// 1.0 schema-only — host event `plugin.window_declared` 로 가시화.
    PluginWindowDeclared {
        plugin_id: String,
        window_id: String,
    },
}

// IdGenerator is now in core_state.rs

pub struct AppState {
    // ── Window-level UI state ──
    pub(crate) active_workspace: usize,
    /// 카테고리별 마지막 active 워크스페이스의 **id**. 카테고리 quick-switch(T4WS ②⑤)가
    /// 대상 카테고리로 점프할 때 그 카테고리의 마지막 포커스 워크스페이스로 착지하기 위한
    /// 세션-런타임 상태(영속 안 함 — "never visited" 는 first 로 폴백).
    ///
    /// **전역 인덱스가 아니라 id 다.** 인덱스를 들면 워크스페이스 제거·재정렬마다
    /// 이 맵을 함께 밀어줘야 하고, 밀어주는 것을 잊은 경로가 생기면 "같은 카테고리의
    /// 다른 워크스페이스로 착지" 하는 조용한 오작동이 된다(실제로 재정렬 경로 두 곳이
    /// 그랬다). id 는 순서 변경과 무관하므로 그 유지보수 자체가 없어진다 —
    /// 착지 시점에 id → 인덱스로 한 번 찾고, 못 찾으면(제거됐으면) first 로 폴백한다.
    pub(crate) category_last_active: std::collections::HashMap<
        tasty_utils::id::WorkspaceCategoryId,
        tasty_utils::id::WorkspaceId,
    >,
    /// Whether the settings window is open.
    pub(crate) settings_open: bool,
    /// Whether the plugins window is open.
    pub(crate) plugins_open: bool,
    /// Persistent UI state for the settings window.
    #[cfg(feature = "gui")]
    pub(crate) settings_ui_state: SettingsUiState,
    /// Cached sidebar width from settings (logical pixels).
    pub(crate) sidebar_width: LogicalPx,
    /// Sidebar visibility: false = completely hidden.
    pub(crate) sidebar_visible: bool,
    /// Sidebar collapsed: true = compact mode (narrow width, icons only).
    pub(crate) sidebar_collapsed: bool,
    /// 통합 리사이즈 커서 피드백 — `handle_cursor_moved` 가 창 가장자리 hover 를
    /// 감지하면 그 8방향을 저장하고, egui 프레임(`run_egui_frame`)이 매 프레임
    /// `set_cursor_icon` 으로 적용한다. egui 가 winit 커서를 매 프레임 덮으므로
    /// 프레임 내에서만 적용할 수 있어 상태로 보관한다. 데코 없는 Windows/Linux
    /// 창에서만 채워진다(macOS 는 네이티브 데코라 항상 None). 콘텐츠/오버레이 위에서는
    /// None 으로 리셋된다(콘텐츠 우선 입력모델).
    #[cfg(feature = "gui")]
    pub(crate) pending_resize_cursor: Option<winit::window::ResizeDirection>,
    /// switch-number overlay 활성 스냅샷. 현재 눌린 modifier 가 tab/workspace 전환
    /// 단축키와 일치하면 그 대상(+Tab 이면 focused pane id)을 담는다. `MainView` 의
    /// `ModifiersChanged` 가 [`crate::adapters::ui::switch_overlay::switch_target_for`]
    /// 로 갱신하고, 창 비활성/포커스 상실 시 `None` 으로 clear 된다. draw 경로(04 탭
    /// /05 사이드바)가 매 프레임 읽어 숫자 키캡 오버레이를 표시할지 결정한다.
    #[cfg(feature = "gui")]
    pub(crate) switch_overlay: Option<crate::adapters::ui::switch_overlay::SwitchOverlayState>,
    /// modifier-hint 오버레이 런타임 상태 — 홀드 시작 시각·anchor modifier·세션 dismiss·
    /// 진행 중 드래그 working rect. `MainView` 의 `ModifiersChanged` 가 `update_hold` 로
    /// 갱신하고, 창 포커스 상실 시 `clear` 된다. draw 경로(`overlay::draw_overlays`)가 매
    /// 프레임 `draw_modifier_hint` 로 읽어 500ms 홀드 후 오버레이를 그린다. 지오메트리 영속값은
    /// `Settings::modifier_hint`(pos/size), 이 필드는 홀드/드래그 세션 상태만.
    #[cfg(feature = "gui")]
    pub(crate) modifier_hint: crate::adapters::ui::modifier_hint_overlay::ModifierHintRuntime,
    /// 튜토리얼(마커 오버레이) 런타임 상태 — 진행 중 주제/step, 목록 팝업 선택·시작
    /// 큐. GUI 전용(사용자 클릭으로만 진행, IPC/CLI 발화 없음 — 불가침 원칙 1).
    /// `overlay::draw_overlays` 말미의 `draw_tutorial_overlay` 가 매 프레임 읽고 전이시킨다.
    #[cfg(feature = "gui")]
    pub(crate) tutorial: crate::adapters::ui::tutorial::TutorialRuntime,
    /// All transient dialog/popup state.
    pub(crate) dialogs: DialogState,
    /// Measured tab bar height in physical pixels, updated each frame by egui.
    pub(crate) tab_bar_height: PhysicalPx,
    /// Popup manager for internal popups (notification panel, etc.).
    #[cfg(feature = "gui")]
    pub(crate) popups: crate::adapters::ui::PopupManager,
    /// 활성 전체화면 무대. 창(=`MainView`)당 **최대 하나**라 `Vec` 이 아니라 `Option`
    /// 이다 — popup 과 달리 z-order·다중 인스턴스 관리가 필요 없다. 창이 여럿이면
    /// 창마다 독립적으로 무대를 가질 수 있다(각 `MainView` 가 자기 `AppState` 를
    /// 가지므로 이 필드 배치 자체가 그 계약이다). 영속화 대상이 아니다 — 재시작이
    /// 무대 상태로 부팅되면 사용자가 창을 조작할 수 없다.
    #[cfg(feature = "gui")]
    pub(crate) fullscreen_stage: Option<crate::adapters::ui::fullscreen::StageState>,
    /// 닫힌 무대의 `on_close` 대기열. 닫는 경로가 무엇이든
    /// [`AppState::close_fullscreen_stage`] 한 곳을 지나 여기 쌓이고, draw 경로의
    /// `fullscreen::drain_on_close_hooks` 가 정확히 1 회 발화시킨다(ADR-0063 패턴).
    #[cfg(feature = "gui")]
    pub(crate) stage_closed_queue: Vec<crate::adapters::ui::fullscreen::StageId>,
    /// 무대 중 DPI/모니터 전환으로 **보류된** 기본 grid 갱신이 있는지.
    ///
    /// 무대는 "원본은 진입 시점 그대로" 가 계약이라 무대 중에는
    /// `CoreState::update_grid_size`(신규 터미널의 기본 cols/rows)를 갱신하지 않는다.
    /// 그대로 버리면 무대를 나온 뒤에도 기본값이 옛 DPI 에 머물므로, 보류 사실을
    /// 여기 남겼다가 무대를 나온 첫 프레임에 한 번 적용한다
    /// (`MainView::resync_scale_factor`).
    #[cfg(feature = "gui")]
    pub(crate) stage_deferred_grid_resync: bool,
    /// Terminal text search state.
    pub(crate) search: crate::search_state::SearchState,
    /// Listening-port scanner async state machine. Driven by the port scanner
    /// popup: Idle → Loading (background thread + mpsc channel) → Ready / Failed.
    /// Reset to `Idle` when the popup closes.
    #[cfg(feature = "gui")]
    pub(crate) port_scan: crate::adapters::ui::popup::port_scanner::PortScanState,
    /// System-wide scan backing the favorites section's LISTEN/NONE badges.
    /// Independent of `port_scan`'s scope (Tasty/System) — always a full
    /// `scan_all()`, kicked only while at least one favorite is registered.
    /// Reset to `Idle` when the popup closes.
    #[cfg(feature = "gui")]
    pub(crate) port_favorites_scan: crate::adapters::ui::popup::port_scanner::PortScanState,
    /// Command palette UI state — query buffer, selection cursor, and a pending
    /// dispatch slot that MainView drains each frame.
    pub(crate) command_palette: crate::state::command_palette::CommandPaletteState,
    /// Toast manager for transient in-app notifications (copy feedback, etc.).
    /// 사용자 행동에서만 발사한다. CLI/IPC 경유 동작은 토스트를 만들지 않는다.
    #[cfg(feature = "gui")]
    pub(crate) toasts: crate::adapters::ui::ToastManager,
    /// Banner manager — 4번째 오버레이(공지+action). Toast/Popup 과 별도 매니저.
    /// 사용자 행동에서만 발사한다. IPC/release cascade 는 배너를 띄울 수 없다.
    #[cfg(feature = "gui")]
    pub(crate) banners: crate::adapters::ui::BannerManager,
    /// Cached recent files list (markdown/html open popups). Loaded from disk at
    /// startup and mutated in-place; each mutation saves back to disk.
    pub(crate) recent_files: crate::recent_files::RecentFiles,
    /// Whether the mouse is currently over an open popup (input layer state).
    /// Updated each frame by PopupManager::draw(). Mouse handlers check this
    /// to block events from reaching lower layers (terminal, dividers).
    pub(crate) popup_hovered: bool,
    /// plugin egui-mesh popup 이 하나라도 열려 있는가 (입력 계층 상태).
    ///
    /// 키/IME 게이트(`view::main`, `view::main::keyboard`)는 `PluginManager` 에
    /// 접근할 수 없다 — 그 타입은 `App` 소유이고 게이트가 있는 `handle_event` 로는
    /// 흘러들지 않는다. 그래서 `popup_hovered` 와 같은 방식으로 렌더 프레임이 채워
    /// 두는 캐시를 읽는다. 갱신은 `plugin_bridge::popup_render::draw_plugin_popups`
    /// 최상단 리셋 + popup 이 있을 때 set 이며, popup 이 없다는 두 조기 반환 경로도
    /// 리셋이 덮는다(stale `true` 는 키보드가 영영 터미널로 못 가는 상태가 된다).
    pub(crate) plugin_popup_open: bool,
    /// Whether the mouse is currently over a banner (input layer state).
    /// Updated each frame by BannerManager::draw(). 배너는 자기 영역의 마우스를
    /// 소비(뒤로 전파 X)하므로 mouse 핸들러가 이 값으로 하위 레이어 전파를 막는다.
    /// (포커스는 받지 않음 — 마우스 소비만.) popup_hovered 와 동일하게 비-gui 빌드도
    /// 필드를 갖는다(입력 가드가 공유).
    pub(crate) banner_hovered: bool,
    /// 마우스가 modifier-hint 오버레이 위인지(입력 레이어 상태). `draw_modifier_hint` 가
    /// 매 프레임 갱신한다. 오버레이는 **키보드 포커스를 받지 않고 마우스만 소비**하므로
    /// (원칙3), mouse 핸들러가 이 값으로 click-to-activate/휠/드래그가 하위 surface 로
    /// 새지 않게 막는다. banner_hovered 와 동일 성질(비-gui 빌드도 필드 보유).
    pub(crate) modifier_hint_hovered: bool,
    /// 이번 프레임에 그려진 각 popup 의 `LayerId`. `PopupManager::draw()` 가 갱신.
    /// `enforce_foreground_z_order`(`src/gfx/gpu/egui_bridge.rs`)가 modifier-hint
    /// 레이어를 부모로 이들을 `set_sublayer` 자식으로 묶을 때 읽는다 — `egui::LayerId`
    /// 의존이라 gui 전용.
    #[cfg(feature = "gui")]
    pub(crate) popup_layers: Vec<egui::LayerId>,
    /// 이번 프레임에 그려진 각 plugin egui-mesh popup 셸의 `LayerId`.
    /// `draw_plugin_popups`(`plugin_bridge/popup_render.rs`)가 갱신. host popup
    /// (`popup_layers`)과 이 목록 사이의 상대 순서를 host↔plugin popup z-order
    /// (`docs/design/systems/popup.md` 규칙 7)에 따라 `set_sublayer`로 강제할 때 쓴다
    /// (`gfx/gpu/egui_bridge.rs`) — 두 popup 종류 모두 `ctx.layer_painter`로 직접
    /// 그리는 raw layer 라 `egui::Area`를 거치지 않고, 따라서 `Areas::order`(Area 기반
    /// 위젯만 자동 등록됨)에 자연히 편입되지 않는다. 등록 호출 순서는 이 `order`에
    /// 전혀 반영되지 않으므로(각 프레임 `GraphicLayers::drain`이 `order`에 없는 레이어를
    /// 별도 맵 순회로 덧붙임 — egui 소스), `set_sublayer`로 명시적으로 관계를 걸지
    /// 않으면 두 popup 종류 사이의 상대 순서는 사실상 비결정적이다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_layers: Vec<egui::LayerId>,
    /// 이번 프레임에 그려진 host popup 들의 히트테스트 rect + z_seq
    /// (`PopupManager::draw` 가 갱신). `draw_plugin_popups` 가 **같은 프레임에** 읽어
    /// "내 위에 host popup 이 이 좌표를 덮는가" 를 판정한다(규칙 7 — 겹친 영역의
    /// 마우스 이벤트는 최상단만 받는다). host draw 가 plugin draw 보다 먼저 돌므로
    /// 이 방향은 stale 이 아니다.
    #[cfg(feature = "gui")]
    pub(crate) host_popup_hittest: Vec<crate::adapters::ui::popup::occlusion::Occluder>,
    /// 이번 프레임 Esc 를 소비할 자격이 있는 host popup(규칙 7 의 키보드 판, ADR-0084).
    /// host/plugin 통틀어 최상단이 host popup 일 때만 `Some` — plugin popup 이 위면
    /// `None` 이고, 그 프레임의 Esc 는 plugin 쪽이 가져간다. popup 의 view 가 Esc 를
    /// 소비하기 전에 이 값을 확인한다.
    #[cfg(feature = "gui")]
    pub(crate) popup_escape_owner: Option<crate::adapters::ui::popup::PopupId>,
    /// 직전 프레임에 그려진 plugin egui-mesh popup 셸 rect + z_seq
    /// (`draw_plugin_popups` 가 갱신). host 쪽 히트테스트가 읽는다 — host draw 가
    /// 먼저 돌기 때문에 **1 프레임 stale** 이다(`popup/draw.rs` 의 outside-click
    /// 분기 주석 참고). 셸 rect(마진 포함)라 `plugin_mesh_popup_regions`(콘텐츠
    /// rect, 물리 px)와는 다른 값이다.
    #[cfg(feature = "gui")]
    pub(crate) plugin_popup_hittest: Vec<crate::adapters::ui::popup::occlusion::Occluder>,
    /// 이번 프레임에 그려진 `banner_layer` Area 의 `LayerId`(banner 는 매 프레임 항상
    /// 그려지므로 첫 프레임 이후 항상 `Some`). `BannerManager::draw()` 가 갱신,
    /// `enforce_foreground_z_order` 가 읽는다.
    #[cfg(feature = "gui")]
    pub(crate) banner_layer: Option<egui::LayerId>,
    /// 이번 프레임에 `modhint_layer` Area 를 실제로 그렸으면 그 `LayerId`(표시 조건
    /// 미충족이면 `None`). `draw_modifier_hint()` 가 갱신, `enforce_foreground_z_order`
    /// 가 읽는다.
    #[cfg(feature = "gui")]
    pub(crate) modifier_hint_layer: Option<egui::LayerId>,
    /// 마우스가 창 가장자리 리사이즈 우선권을 가져야 하는 실제 인터랙티브 chrome
    /// 위젯(타이틀바 창 버튼·Windows 캡션 버튼·상태바 클릭 요소) 위인지(입력 레이어
    /// 상태). 각 위젯이 매 프레임 자신의 `Response::hovered()` 로 갱신한다 —
    /// `egui_consumed`(패널/Area 전체의 bounding rect 단위)와 달리 위젯 단위라
    /// 빈 여백까지 리사이즈를 막지 않는다. `try_begin_os_resize` 가 가장자리 margin
    /// 안에서 리사이즈를 양보할지 판단할 때만 쓰인다. popup_hovered 와 동일하게
    /// 비-gui 빌드도 필드를 갖는다(입력 가드가 공유).
    pub(crate) resize_edge_widget_hovered: bool,
    /// Preset store 의 Arc clone — Core 가 owner. UI popup 이 draw 흐름에서
    /// core 인자 없이 lock 으로 read 할 수 있도록 AppState 에 *clone 보유* 만
    /// 한다 (allocation 동일, owner 는 Core). `create_app_state` 가 inject.
    pub(crate) preset_store: std::sync::Arc<std::sync::Mutex<tasty_presets::PresetStore>>,
    /// Memory store 의 Arc clone — Core 가 owner. UI thread (popup draw_fn) 와
    /// engine state cleanup 이 dispatcher cascade 없이 직접 영속할 때 사용한다.
    /// `Core::with_memory` 와 같은 lock 정책 (poisoning 시 inner 사용).
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    /// Double-tap modifier captured from winit events, for the keybinding recorder to consume.
    pub(crate) captured_double_tap: Option<String>,
    /// Surface close lifecycle 알림 큐. close 직후 enqueue되고, App 메인 루프가
    /// drain하여 `PluginManager::notify_surface_closed`로 dispatch한다.
    /// `state/`는 `plugin/` 의존이 없어 별도 plain struct로 둔다.
    pub(crate) pending_lifecycle_events: Vec<PendingSurfaceClosed>,
    /// Event Bus 1.0 호스트 자동 발화 큐. 호스트 코드 곳곳에서 `enqueue_host_event`로
    /// push하고, App 메인 루프가 drain해 wire payload로 변환·발화한다.
    pub(crate) pending_host_events: Vec<PendingHostEvent>,
    /// `surface.focused` 발화용 변화 감지 상태. tick마다 `focused_surface_id()`와
    /// 비교해 달라졌으면 `SurfaceFocused`를 enqueue한다. focus 전환 경로가 많아
    /// (키보드/마우스/IPC/탭전환/워크스페이스전환) 각각을 hook하기보다 polling이 단순.
    pub(crate) last_focused_surface_id: Option<u32>,
    /// `workspace.activated` 발화용 변화 감지 상태. `active_workspace` 인덱스가
    /// 가리키는 워크스페이스 ID를 기록해 두고, 다음 tick에서 달라졌다면
    /// `WorkspaceActivated`를 enqueue한다.
    pub(crate) last_active_workspace_id: Option<u32>,
    /// `tab.focused` 발화용 변화 감지 상태. 활성 워크스페이스의 focused pane이 보유한
    /// 현재 active tab의 (pane_id, tab_id)를 기록. 다음 tick에서 달라졌다면
    /// `TabFocused`를 enqueue한다. pane 전환·in-pane tab 전환을 한꺼번에 다룬다.
    pub(crate) last_focused_tab: Option<(u32, u32)>,
    /// `tab.created`/`tab.closed`/`tab.moved` 발화용 polling 상태. tab_id →
    /// (pane_id, workspace_id, kind) 스냅샷. `None`은 아직 한 번도 polling하지
    /// 않은 상태(초기 로드된 탭에 대해 spurious `tab.created`가 발화되는 것을 막기
    /// 위해 첫 호출에서는 스냅샷만 만들고 이벤트를 enqueue하지 않는다).
    pub(crate) last_tab_locations: Option<std::collections::HashMap<u32, (u32, u32, String)>>,

    /// Per-surface host view state for `ExplorerPanel` (directory entry cache, selection,
    /// sidebar tree expansion). `ExplorerPanel` itself only holds navigation/tab state.
    #[cfg(feature = "gui")]
    pub(crate) explorer_views: crate::adapters::ui::surface::explorer::view::ExplorerViewStore,

    /// Per-surface host view state for `DagGraphSurface` (폴링 결과 캐시, 레이아웃
    /// 캐시, 줌/팬/선택). surface 모델은 "어떤 DAG 를 어느 방향으로" 만 들고 있다.
    #[cfg(feature = "gui")]
    pub(crate) dag_graph_views: crate::adapters::ui::surface::dag_graph::DagGraphViewStore,

    /// 사이드바 도구 메뉴 항목. 활성 plugin의 `[[contributes.tool]]`
    /// 항목을 합쳐 관리. PluginManager가 plugin 라이프사이클 변경 시
    /// `set_plugin_items(mgr.plugin_tool_items())`로 갱신한다.
    pub(crate) tool_registry: crate::plugin::tool_registry::ToolRegistry,

    /// Command palette에 노출할 plugin 전역 command snapshot. `tool_registry`와
    /// 동형 — PluginManager가 plugin 라이프사이클 변경 시
    /// `mgr.plugin_palette_commands()`로 갱신한다(`App::refresh_palette_plugin_commands`,
    /// `tool_registry_dirty`와 동일 트리거 조건). draw 함수는 `PluginManager`에 직접
    /// 접근할 수 없는 `PopupDef` 고정 시그니처 제약 때문에 이 snapshot을 대신 읽는다.
    pub(crate) palette_plugin_commands: Vec<crate::plugin::command_registry::PluginCommandEntry>,

    /// Command palette에서 plugin 전역 command를 실행했을 때의 (plugin_id, command_id)
    /// 큐. palette popup은 `&mut AppState`만 가지므로 PluginManager에 직접 접근할 수
    /// 없어, 실행 시점에 enqueue하고 App 메인 루프가 drain해
    /// `PluginManager::command_registry`로 action/IPC를 dispatch한다
    /// (`App::dispatch_pending_palette_plugin_commands`, `pending_tool_events`와 동형).
    pub(crate) pending_plugin_command_invokes: Vec<(String, String)>,

    /// 도구 메뉴 항목 클릭 시 publish해야 할 이벤트 큐. tools_menu가 `&mut AppState`만
    /// 가지므로 PluginManager에 직접 접근할 수 없어, 클릭 시점에 enqueue하고 App 메인
    /// 루프가 drain해 `PluginManager::emit_host_event`로 발화한다.
    pub(crate) pending_tool_events: Vec<(String, serde_json::Value)>,

    /// `ToolAction::OpenPopup` 클릭 시 열어야 할 popup 큐.
    /// (plugin_id, popup_id, context). App 메인 루프가 drain해
    /// `PluginManager::open_popup_instance`로 dispatch.
    pub(crate) pending_popup_opens: Vec<(String, String, serde_json::Value)>,

    /// file_handler 디스패치 결과가 plugin IPC method 일 때의 호출 큐.
    /// `(ipc_method, target)`. App 메인 루프가 drain 해 `PluginManager` 로 forward.
    pub(crate) pending_handler_ipc: Vec<(String, crate::file::format::FileTarget)>,

    /// 외부 drag&drop 으로 파일이 hover 중인 상태. `HoveredFile` 마다 path 누적,
    /// `HoveredFileCancelled` / `DroppedFile` 시 해제. 비주얼 overlay 의 입력.
    pub(crate) drop_hover: Option<DropHoverState>,

    /// `DroppedFile` 이벤트로 받은 경로 큐. frame end 에서 drain 해
    /// `DomainIntent::DispatchFile` 으로 발화.
    pub(crate) pending_file_drops: Vec<std::path::PathBuf>,

    /// plugin popup 렌더 중 감지된 close 사유 (outside-click / Escape).
    /// App 메인 루프가 drain해 `PluginManager::close_popup_instance`를 호출한다.
    pub(crate) plugin_popup_closes: Vec<(u64, tasty_plugin_protocol::PopupCloseReason)>,

    /// plugin popup 콘텐츠 영역 내부 클릭으로 z-order 순번 갱신이 필요한 instance_id 큐
    /// (`docs/design/systems/popup.md` 규칙 7 "클릭된 것이 앞"). 렌더 경로(`draw_plugin_popups`)는
    /// `&PluginManager` 불변 참조만 가지므로 직접 갱신할 수 없어 여기 적재하고, App 메인
    /// 루프가 drain해 `PluginManager::touch_popup_instance_z`를 호출한다(close queue 와 동형).
    pub(crate) plugin_popup_focus_bumps: Vec<u64>,

    /// plugin egui-mesh banner(A3) host 측 생명주기(TTL/close X)로 닫힌 사유.
    /// `draw_plugin_banners` 가 적재하고, App 메인 루프가 drain 해
    /// `PluginManager::close_banner_instance` 를 호출한다(popup closes 와 동형 — 렌더
    /// 경로가 manager 를 직접 mutate 하지 않도록 지연).
    pub(crate) plugin_banner_closes: Vec<(u64, tasty_plugin_protocol::BannerCloseReason)>,

    /// egui-mesh popup(A2) 합성 영역. `draw_plugin_popups` 가 매 egui frame 채우고,
    /// `gpu.render` 가 host egui pass *후* 각 (instance_id, 물리 콘텐츠 rect)에 plugin
    /// mesh 를 합성한다. 셸(scrim/bg/border)은 host egui 가, 내용만 plugin mesh 가 그린다.
    pub(crate) plugin_mesh_popup_regions: Vec<(u64, crate::model::PhysicalRect)>,

    /// egui-mesh popup 별 마지막으로 보낸 set_context geom `(w_px, h_px, ppp_bits)`.
    /// 정적 화면을 매 frame 무조건 보내지 않기 위한 변경 감지(surface forward 와 동형).
    pub(crate) plugin_mesh_popup_geom: std::collections::HashMap<u64, (u32, u32, u32)>,

    /// 이미 bootstrap set_context 를 보낸 egui-mesh popup 인스턴스. paint frame 이
    /// 아직 안 온 동안 set_context 를 1회만 보내기 위한 가드(surface `bootstrap_sent` 동형).
    /// frame 이 보이면 해제돼 crash 후 재bootstrap 된다. 핵심: 첫 frame(폰트 atlas 동봉)
    /// 을 host 가 반드시 decode 하도록, bootstrap 을 매 frame 스팸하지 않는다.
    pub(crate) plugin_mesh_popup_bootstrapped: std::collections::HashSet<u64>,

    /// egui-mesh popup 별 마지막으로 보낸 Theme 스냅샷. 크기/입력 무변이어도 테마가
    /// 바뀌면 set_context 재forward 를 트리거한다(surface `last_theme` 동형).
    pub(crate) plugin_mesh_popup_theme:
        std::collections::HashMap<u64, tasty_plugin_protocol::ThemeWire>,

    /// egui-mesh banner(A3) 합성 영역. `draw_plugin_banners` 가 매 egui frame 채우고,
    /// `gpu.render` 가 host egui pass *후* 각 (instance_id, 물리 콘텐츠 rect)에 plugin
    /// mesh 를 합성한다. 셸(컨테이너/border/close X/카운트다운)은 host egui(banner
    /// manager)가, 내용만 plugin mesh 가 그린다. popup regions 와 동형.
    pub(crate) plugin_mesh_banner_regions: Vec<(u64, crate::model::PhysicalRect)>,

    /// egui-mesh banner 별 마지막으로 보낸 set_context geom `(w_px, h_px, ppp_bits)`.
    /// 변경 감지(popup geom 과 동형).
    pub(crate) plugin_mesh_banner_geom: std::collections::HashMap<u64, (u32, u32, u32)>,

    /// 이미 bootstrap set_context 를 보낸 egui-mesh banner 인스턴스(popup bootstrapped 동형).
    pub(crate) plugin_mesh_banner_bootstrapped: std::collections::HashSet<u64>,

    /// egui-mesh banner 별 마지막으로 보낸 Theme 스냅샷(popup theme 과 동형).
    pub(crate) plugin_mesh_banner_theme:
        std::collections::HashMap<u64, tasty_plugin_protocol::ThemeWire>,

    /// textures_delta 체인 단절로 full 재전송이 필요한 egui-mesh popup 인스턴스.
    /// MainView 가 render 직후 gpu 의 요청 대기열을 여기로 옮기고, popup forward
    /// (`draw_plugin_popups`)가 다음 egui frame 에 소비해 `need_full_textures`
    /// set_context 를 보낸다.
    pub(crate) plugin_mesh_popup_full_requests: std::collections::HashSet<u64>,

    /// 비동기 host→plugin push(예: git-viewer 원격 조회 결과, `event.dispatch`
    /// unicast) 도착 후 강제 repaint 가 필요한 egui-mesh popup 인스턴스. 일반 dirty
    /// 판정(geom/input/theme 변경)은 이런 "plugin 내부 상태만 바뀐" 갱신을 감지하지
    /// 못하므로(`draw_plugin_popups`), 이 요청을 채워두면 다음 frame 이 geometry/입력
    /// 변화 없이도 `set_context` 를 재forward 해 plugin 이 새 데이터로 다시 그리게
    /// 한다(`plugin_mesh_popup_full_requests` 와 동형이나 텍스처가 아니라 repaint 자체를
    /// 강제).
    pub(crate) plugin_mesh_popup_pending_repaint: std::collections::HashSet<u64>,

    /// banner 대응 full 재전송 요청(popup full_requests 와 동형).
    pub(crate) plugin_mesh_banner_full_requests: std::collections::HashSet<u64>,

    /// 호스트 내부 Intent 큐. 발화자가 push 만 하고, `App::dispatch_pending_intents`
    /// 가 메인 루프에서 drain 한다. UI Intent (`Intent::Ui`) 와 Domain Intent
    /// (`Intent::Domain`) 가 한 큐 위에서 처리됨 (D.3.I.3 통합). 설계:
    /// `docs/design/flows/action-dispatch.md`, `intent-ui-vs-domain.md`.
    pub(crate) pending_intents: Vec<crate::intent::DispatchedIntent>,
}

/// A pending native context menu request.
#[derive(Clone)]
pub enum PendingNativeMenu {
    /// Tab right-click: Rename / Close
    Tab {
        pane_id: u32,
        tab_index: usize,
        x: f32,
        y: f32,
    },
    /// Pane/empty area right-click: Open Markdown... / Open Explorer / Open HTML...
    Pane { pane_id: u32, x: f32, y: f32 },
    /// Workspace right-click in sidebar: Rename title / Rename subtitle
    Workspace { ws_idx: usize, x: f32, y: f32 },
    /// Terminal surface right-click: Copy surface id (좌표는 logical px 기준)
    TerminalSurface { surface_id: u32, x: f32, y: f32 },
    /// 비-terminal surface (markdown/image/explorer/html 등) right-click (T9).
    /// 전용 항목(현재 copy surface id) + 구분선 + 잘라내기/여기로 이동. 좌표는
    /// logical px 기준. terminal 은 selection-copy 가 있어 `TerminalSurface` 로 분리.
    Surface { surface_id: u32, x: f32, y: f32 },
    /// Explorer surface 내부 우클릭 (T11): 엔트리/다중선택/빈 영역 대상 파일 메뉴.
    /// `paths` 가 비면 빈 영역(=cwd) 대상. `single_is_dir` 는 `paths.len()==1` 일 때만
    /// 유효(폴더 전용 항목 게이팅). 좌표는 logical px.
    Explorer {
        surface_id: u32,
        paths: Vec<std::path::PathBuf>,
        cwd: std::path::PathBuf,
        single_is_dir: bool,
        x: f32,
        y: f32,
    },
    /// Explorer 사이드바 즐겨찾기 항목 우클릭 → "새 탭으로 열기"/"이 폴더로 루트
    /// 설정"/"즐겨찾기에서 제거". 제거는 전역 경로만으로 되지만, "루트 설정" 이
    /// 특정 explorer surface 의 cwd 를 바꾸므로 `surface_id` 가 필요하다.
    ExplorerFavorite {
        surface_id: u32,
        path: std::path::PathBuf,
        x: f32,
        y: f32,
    },
    /// "New workspace" 버튼 우클릭 (full/collapsed sidebar 공통): 프리셋으로 새 워크스페이스 생성
    NewWorkspaceButton { x: f32, y: f32 },
    /// 확장 사이드바 카테고리 헤더 우클릭 (토글 on): 비-normal 은 이름변경/삭제 +
    /// 새 카테고리, normal 은 새 카테고리만(additive).
    WorkspaceCategoryHeader {
        cat_id: crate::model::WorkspaceCategoryId,
        x: f32,
        y: f32,
    },
    /// 확장 사이드바 빈 배경 우클릭 (카테고리 토글 on/off 공통): 새 카테고리 · 원격 워크스페이스 추가.
    SidebarBackground { x: f32, y: f32 },
    /// 탭 "+" 버튼 우클릭: 프리셋으로 탭/페인 생성
    NewTabButton { pane_id: u32, x: f32, y: f32 },
}

/// All transient UI dialog/popup state, grouped to avoid AppState bloat.
/// New dialogs should be added here, not as top-level AppState fields.
pub struct DialogState {
    /// Unified rename dialog: target + edit buffer.
    pub(crate) rename: Option<(RenameTarget, String)>,
    /// 마우스 캡처 배너 "더보기" 컨텍스트 메뉴 대상 surface_id. 메뉴가 어느
    /// surface 의 배너에 대한 것인지 `mouse_capture_menu` draw_fn 에 전달한다
    /// (`RenameTarget` 과 동일 패턴 — popup 이 대상 정보를 직접 갖지 않아서).
    pub(crate) mouse_capture_banner_menu_target: Option<u32>,
    /// Surface convert popup: target surface_id (None = closed)
    pub(crate) convert_popup: Option<u32>,
    /// Keyboard-selected index in the convert popup menu
    pub(crate) convert_popup_selected: Option<usize>,
    /// Pending native context menu
    pub(crate) pending_native_menu: Option<PendingNativeMenu>,
    /// Pending file drag request (paths to drag to external apps).
    pub(crate) pending_file_drag: Option<Vec<String>>,
    /// Tab drag-and-drop state.
    pub(crate) tab_drag: Option<TabDragState>,
    /// Workspace drag-and-drop state.
    pub(crate) ws_drag: Option<WsDragState>,
    /// 부팅 시점 정보/에러 알림용 modal 큐. 큐 head를 [확인] 버튼으로 처리한다.
    /// `crate::adapters::ui::info_modal::show_info_modal()`로 push.
    #[cfg(feature = "gui")]
    pub(crate) info_modal_queue: VecDeque<InfoModal>,
    /// 휴먼 핸드오프 — 응답 대기 중인 approval 큐. popup 의 head 가 현재 화면.
    /// `approval.request` IPC 가 push하고, 선택지 클릭 시 pop.
    pub(crate) pending_approval_ids: VecDeque<tasty_approval::ApprovalId>,
    /// approval popup 의 코멘트 입력 버퍼 (현재 head용 임시 상태).
    pub(crate) approval_comment_buffer: String,
    /// file_handler_picker popup 의 입력/선택 상태. `None` 이면 popup 미오픈.
    pub(crate) file_handler_picker: Option<FileHandlerPickerData>,
    /// 네이티브 파일 피커(04) popup 의 내비게이션/로딩/선택 상태. `None` 이면 popup 미오픈.
    pub(crate) file_picker: Option<FilePickerData>,
    /// DAG 목록 popup 의 전 상태(검색/필터/열린 DAG/그래프 뷰). `on_close` 가
    /// 통째로 기본값으로 되돌린다 — popup 은 surface 가 아니라 snapshot/restore
    /// 대상이 아니고, 닫힌 뒤에도 남는 상태는 다음 open 을 오염시킨다.
    #[cfg(feature = "gui")]
    pub(crate) dag_list: crate::adapters::ui::popup::dag_list::DagListState,
    /// 도구 메뉴 클릭 / preset save 후속 — PresetView 를 열어달라는 요청.
    /// `selection` 이 `Some` 이면 PresetView 가 열린 뒤 해당 preset 을 선택한다.
    /// App 메인 루프 `process_pending_open_preset_window` 가 drain.
    pub(crate) pending_open_preset_window: bool,
    /// PresetView 가 열린 뒤 자동 선택할 preset. `pending_open_preset_window` 와 함께 사용.
    pub(crate) pending_preset_window_selection: Option<(tasty_presets::PresetKind, String)>,
    /// 프리셋 적용 picker popup 의 현재 하이라이트 (preset name). popup 닫힘 시 None.
    pub(crate) preset_picker_selected: Option<String>,
    /// `enter_copy_mode` 단축키 트리거 신호. MainView 가 다음 frame 에 소비.
    pub(crate) pending_enter_copy_mode: bool,
    /// 축소 레일 카테고리 팝업(`rail_category`)이 대상으로 하는 카테고리 id.
    /// `---` 버튼 클릭 시 set, popup 닫힘 시 None.
    pub(crate) rail_category_popup: Option<crate::model::WorkspaceCategoryId>,
    /// 카테고리 헤더 우클릭 메뉴의 "프리셋으로부터 워크스페이스 생성" 이 연 프리셋
    /// 적용 팝업(`APPLY_WORKSPACE_POPUP_ID`)이 대상으로 하는 카테고리 id. Apply 시
    /// `take()` 로 소비해 `Intent::ApplyPreset.category` 에 실어 보낸다. 어떤 경로로
    /// 닫히든 `on_close_apply_preset_popup` 훅(`popup/preset_apply.rs`)이 `None` 으로
    /// 정리하므로, 다음 open 시점에는 방어적으로 다시 리셋할 필요가 없다.
    pub(crate) preset_apply_target_category: Option<crate::model::WorkspaceCategoryId>,
    /// 카테고리 삭제 확인 다이얼로그(`confirm_delete_category`)의 대상 카테고리 id.
    /// Delete 액션 시 set, 확인/취소/닫힘 시 None.
    pub(crate) pending_category_delete: Option<crate::model::WorkspaceCategoryId>,
    /// Lua 스크립트 TOFU 변경 확인(`script_changed_confirm`) 팝업의 보류 상태 (ADR-0031).
    /// 등록 해시와 현재 파일 해시가 다르면 단축키 발화가 실행을 보류하고 이 값을 채운다 —
    /// 사용자가 [실행] 하면 `App::dispatch_pending_script_confirm` 이 해시를 갱신·영속하고
    /// 워커에서 실행하며, [취소]/Esc 면 슬롯을 폐기한다.
    pub(crate) pending_script_confirm: Option<PendingScriptConfirm>,
    /// (09) 원격 전송 진행 팝업(`transfer_progress`) 상태. 진행 중인 파일 행들을 담고,
    /// 08 워커 진행 이벤트가 갱신한다. 모든 행이 끝나면 `None` + 팝업 self-close.
    #[cfg(feature = "gui")]
    pub(crate) transfer_progress: Option<TransferProgress>,
    /// (09) 원격 전송 실패 팝업(`transfer_error`) 큐. 전송 실패/거부 시 push, Dismiss 시
    /// pop(큐가 비면 팝업 닫힘 — info_modal 큐 패턴). head 가 현재 화면.
    #[cfg(feature = "gui")]
    pub(crate) transfer_error: VecDeque<TransferError>,
}

/// Lua 스크립트 TOFU 변경 확인 팝업의 보류 상태 (ADR-0031).
///
/// 단축키 발화 시 등록 해시(03)와 현재 파일 해시가 다르면 실행 대신 이 값을 채우고
/// 확인 팝업을 띄운다. [실행] 확정 시 `new_hash` 로 레지스트리를 갱신·영속하고 워커에서 실행.
#[derive(Debug, Clone)]
pub struct PendingScriptConfirm {
    /// 대상 스크립트 id (레지스트리 참조).
    pub(crate) script_id: String,
    /// 표시 이름(파일명 fallback 은 게이트가 미리 해소).
    pub(crate) name: String,
    /// 이미 읽은 현재 파일 소스 — 승인 시 그대로 실행(재읽기 없음, TOCTOU 축소).
    pub(crate) source: String,
    /// 현재 파일 소스의 SHA256 — 승인 시 레지스트리 해시를 이 값으로 갱신.
    pub(crate) new_hash: String,
    /// 팝업 wrapper 의 사용자 결정: `Some(true)`=실행, `Some(false)`=취소.
    pub(crate) result: Option<bool>,
}

/// Tab drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct TabDragState {
    pub(crate) pane_id: u32,
    pub(crate) tab_index: usize,
    /// Current mouse x in logical pixels (for insert position calculation).
    pub(crate) current_x: f32,
}

/// 외부 drag&drop hover 중 누적되는 파일 경로 + 시작 cursor 좌표.
/// winit `HoveredFile` 이 N 파일에 대해 N번 발화하므로 `paths` 에 누적.
#[derive(Debug, Clone, Default)]
pub struct DropHoverState {
    pub(crate) paths: Vec<std::path::PathBuf>,
    /// hover 시작 시점의 cursor position (physical pixels). `CursorLeft` 또는
    /// `CursorMoved` 가 drag 중에 발화되지 않을 수 있어 보수적으로 시작점만 기록.
    /// 향후 drop indicator 정밀화 시 read 예정.
    #[allow(dead_code)]
    pub(crate) cursor: Option<(f32, f32)>,
}

/// Workspace drag-and-drop state (UI-only, not persisted).
#[derive(Clone)]
pub struct WsDragState {
    pub(crate) ws_idx: usize,
    /// Current mouse y in logical pixels (for insert position calculation).
    pub(crate) current_y: f32,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            rename: None,
            mouse_capture_banner_menu_target: None,
            convert_popup: None,
            convert_popup_selected: None,
            pending_native_menu: None,
            pending_file_drag: None,
            tab_drag: None,
            ws_drag: None,
            #[cfg(feature = "gui")]
            info_modal_queue: VecDeque::new(),
            pending_approval_ids: VecDeque::new(),
            approval_comment_buffer: String::new(),
            file_handler_picker: None,
            file_picker: None,
            #[cfg(feature = "gui")]
            dag_list: Default::default(),
            pending_preset_window_selection: None,
            pending_open_preset_window: false,
            preset_picker_selected: None,
            pending_enter_copy_mode: false,
            rail_category_popup: None,
            preset_apply_target_category: None,
            pending_category_delete: None,
            pending_script_confirm: None,
            #[cfg(feature = "gui")]
            transfer_progress: None,
            #[cfg(feature = "gui")]
            transfer_error: VecDeque::new(),
        }
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_text_input_open(&self) -> bool {
        self.rename.is_some()
    }

    /// Returns true if any dialog/popup overlay is open.
    pub fn has_any_overlay(&self) -> bool {
        self.has_text_input_open()
    }
}

/// file_handler picker popup 의 한 행 — handler 요약.
#[derive(Debug, Clone)]
pub struct PickerHandlerSummary {
    pub(crate) id: crate::file::handler::HandlerId,
    /// 표시용 라벨. i18n key 가 있으면 번역된 값, 없으면 handler id.
    pub(crate) display: String,
}

/// file_handler picker popup 의 상태.
///
/// 호출자가 popup 을 띄울 때 채워 넣는다. picker 는 직접 dispatch 하지 않고
/// 선택 결과를 [`FileHandlerPickerData::result`] 로 남긴다. host 본체 layer 가
/// frame 끝에 result 를 확인해 실제 핸들러 실행 + RecentPicks 기록을 수행한다.
#[derive(Debug, Clone)]
pub struct FileHandlerPickerData {
    /// 원본 dispatch target. picker 가 닫힌 뒤 host 가 handler 를 실행할 때
    /// 사용한다 — `target_display` 는 화면용이라 escape/축약이 들어갈 수 있다.
    pub(crate) target: crate::file::format::FileTarget,
    /// 표시용 — picker 헤더에 보일 대상 (예: 파일 경로).
    pub(crate) target_display: String,
    /// 탐지된 detector — 없을 수도 있음 ($unknown 등 unmatched).
    pub(crate) detector: Option<crate::file::format::DetectorId>,
    /// 좌측 list 의 후보들 — handler id 사전순.
    pub(crate) candidates: Vec<PickerHandlerSummary>,
    /// `candidates` 가 detector 매칭 결과가 아니라 `FileHandlerRegistry::all_handlers()`
    /// fallback(이 포맷엔 매칭 핸들러가 없어 전체 핸들러를 대신 보여주는 경우)이면
    /// true. draw wrapper 가 "후보" 대신 "추천 없음 — 전체 핸들러" 류 heading 을
    /// 고르는 데 사용 — 1회성 dispatch 후보일 뿐 이 detector 에 영구 연결되지 않음.
    pub(crate) candidates_are_fallback: bool,
    /// 우측 list 의 recent handler ids — 현재 등록된 것만, 저장 파일 순서.
    pub(crate) recent: Vec<PickerHandlerSummary>,
    /// 현재 선택된 handler. 더블클릭/[열기]로 dispatch.
    pub(crate) selected: Option<crate::file::handler::HandlerId>,
    /// dispatch 결과. host 본체 layer 가 frame 끝에서 소비.
    pub(crate) result: Option<FileHandlerPickerResult>,
    /// 원본 dispatch 의 크기제한 bypass 플래그. picker 왕복을 통과해 선택 후
    /// `execute_handler_action` 까지 전달된다(대용량 markdown 게이트 존중).
    pub(crate) ignore_size_limit: bool,
}

/// picker 의 닫기 사유.
#[derive(Debug, Clone)]
pub enum FileHandlerPickerResult {
    /// 사용자가 handler 선택. host 본체 layer 가 실행 + recent 기록.
    Selected(crate::file::handler::HandlerId),
    /// 취소 또는 ESC — dispatch 없음.
    Cancelled,
    /// 시스템 전체 handler 가 0개(빈 상태)일 때 "설정에서 핸들러 등록" 클릭.
    /// App 레이어(`dispatch_pending_picker_results`)가 `Core::apply_file_picker_result`
    /// 로 내려보내지 않고 직접 가로채 Settings 모달을 FileHandler 탭으로 연다 —
    /// Core 는 winit `ActiveEventLoop` 에 접근할 수 없다.
    OpenSettings,
}

/// 네이티브 파일 피커(04) 의 디렉토리 로드 상태. gallery specimen `FpState` 와 1:1
/// 대응 — `ErrorConn` 은 원격(mirror) 전용, 로컬은 `ErrorPerm`/`Loaded`/`Empty` 만 쓴다.
#[derive(Debug, Clone)]
pub(crate) enum FpLoadState {
    /// 원격 조회 요청을 보내고 응답 대기 중. `sent_at` 기준 soft timeout 이 지나면
    /// wrapper 가 `ErrorConn` 으로 전이시킨다(응답 없는 "서버는 살아있는데 응답이
    /// 안 오는" 케이스 — 세션의 `disconnected` 플래그만으론 못 잡는다).
    Loading {
        request_id: u64,
        sent_at: std::time::Instant,
    },
    /// 엔트리 로드 완료(`FilePickerData::entries` 참조, 비어있지 않음).
    Loaded,
    /// 로드 완료했으나 디렉토리가 비어있음.
    Empty,
    /// 권한 거부(로컬/원격 공통 — 파일시스템 자체의 read 실패).
    ErrorPerm(String),
    /// 원격 연결 끊김/타임아웃(원격 전용 — mirror 세션의 `disconnected` 플래그 또는
    /// soft timeout).
    ErrorConn(String),
}

/// `file_picker.trigger` IPC(ADR-0058)로 popup 을 연 plugin 의 요청자 정보.
/// Tools 메뉴가 연 경우(`requester: None`)와 구분해, 확정/취소 시
/// `"file_picker.result"` 이벤트를 이 plugin 에만 unicast 하는 데 쓴다.
#[derive(Debug, Clone)]
pub(crate) struct FilePickerRequester {
    pub(crate) plugin_id: String,
    /// `next_file_picker_trigger_request_id()` 발급 값 — `FpLoadState::Loading` 의
    /// 내부 `request_id` 와는 별개 네임스페이스(`core::mod` 문서 참고).
    pub(crate) request_id: u64,
    /// 이 피커를 연 plugin popup instance(= 부모). `file_picker.trigger` 의
    /// `owner_popup_instance` 파라미터로 plugin 이 자진 신고한 값이다 — host 는
    /// popup 밖(surface 위젯 등)에서 호출한 경우를 구분할 수 없으므로 `Option`.
    ///
    /// 소유 관계의 **유일한 보관처**다(ADR-0084). 별도 레지스트리를 두면 피커
    /// 수명과 어긋날 수 있어, 피커 자신이 들고 있게 했다 — 피커가 사라지면 관계도
    /// 같이 사라진다.
    pub(crate) owner_popup_instance: Option<u64>,
}

/// 네이티브 파일 피커(04) popup 의 상태. `file_handler_picker` 와 동일하게 popup
/// 은 직접 dispatch 하지 않고 결과를 [`FilePickerData::result`] 에 남긴다 — host
/// 본체 layer(`app::dispatch::file_picker`)가 frame 끝에 소비한다.
pub(crate) struct FilePickerData {
    /// `Some(local mirror workspace id)` 면 원격 브라우징(호스트 배지 렌더), `None`
    /// 이면 로컬. 트리거 시점의 **활성 workspace**(`AppState::active_workspace`) 기준으로
    /// 1회 판별해 고정한다(트리거 이후 활성 workspace 가 바뀌어도 popup 대상은 흔들리지
    /// 않음).
    pub(crate) mirror_ws_id: Option<u32>,
    /// 헤더 host 배지 문자열(`mirror_ws_id.is_some()` 일 때만 유효).
    pub(crate) remote_host: Option<String>,
    /// 현재 표시 중인 디렉토리(로컬: 절대경로, 원격: 원격 경로 문자열).
    pub(crate) current_dir: String,
    pub(crate) load: FpLoadState,
    /// 현재 디렉토리의 엔트리(`load` 가 `Loaded`/`Empty` 일 때만 최신).
    pub(crate) entries: Vec<crate::core::fs_list::DirEntryInfo>,
    /// 선택된 엔트리 이름(현재 `current_dir` 기준). 현재는 단일 선택만 지원 —
    /// `Select` action 이 매번 통째로 교체한다(멀티 셀렉트 토글 UI 는 스코프 밖).
    pub(crate) selected: Vec<String>,
    /// Confirm/Cancel 결과. host 본체 layer 가 frame 끝에서 소비.
    pub(crate) result: Option<FilePickerResult>,
    /// `Some` 이면 `file_picker.trigger` IPC 로 이 popup 을 연 plugin.
    /// Tools 메뉴 트리거는 `None`.
    pub(crate) requester: Option<FilePickerRequester>,
    /// `file_picker.trigger` 의 `filters?: string[]`(확장자, 점 없이) — 비어 있으면
    /// 필터 없음(모든 엔트리 표시). Tools 메뉴 트리거는 항상 비어 있음. 디렉토리는
    /// 필터와 무관하게 항상 표시한다(내비게이션 대상이라 필터로 숨기면 하위로 못 감).
    pub(crate) filters: Vec<String>,
}

/// 네이티브 파일 피커(04) 의 닫기 사유.
#[derive(Debug, Clone)]
pub(crate) enum FilePickerResult {
    /// 취소 또는 ESC — dispatch 없음.
    Cancelled,
    /// 사용자가 [열기] — 선택된 절대/원격 경로들과 원격 여부.
    Confirmed { paths: Vec<String>, is_remote: bool },
}

/// What is being renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    /// Workspace name.
    WorkspaceName { ws_idx: usize },
    /// Workspace subtitle.
    WorkspaceSubtitle { ws_idx: usize },
    /// Tab name.
    TabName { pane_id: u32, tab_index: usize },
    /// Explorer 파일/폴더 이름 변경 (T11). 대상 surface + 현재 경로.
    ExplorerEntry {
        surface_id: u32,
        path: std::path::PathBuf,
    },
    /// Explorer 즐겨찾기 추가 (T11). rename 팝업과 동일 골격 — buffer = 표시 라벨.
    /// 대상 경로를 그 라벨로 전역 즐겨찾기에 등록한다.
    ExplorerAddFavorite { path: std::path::PathBuf },
    /// 새 워크스페이스 카테고리 생성 — buffer 빈 문자열로 시작, 확인 시 인라인 검증.
    NewCategory,
    /// 카테고리 이름 변경 — 대상 카테고리 id. buffer 초기값 = 현재 이름.
    CategoryName {
        cat_id: crate::model::WorkspaceCategoryId,
    },
}

impl RenameTarget {
    /// i18n key for the dialog heading.
    pub fn heading_key(&self) -> &'static str {
        match self {
            Self::WorkspaceName { .. } => "rename_dialog.title_heading",
            Self::WorkspaceSubtitle { .. } => "rename_dialog.subtitle_heading",
            Self::TabName { .. } => "rename_dialog.tab_heading",
            Self::ExplorerEntry { .. } => "explorer.popup.rename.title",
            Self::ExplorerAddFavorite { .. } => "explorer.popup.add_favorite.title",
            Self::NewCategory => "rename_dialog.new_category_heading",
            Self::CategoryName { .. } => "rename_dialog.category_heading",
        }
    }

    /// Popup scope matching the rename target.
    pub fn popup_scope(&self) -> crate::model::popup_kind::PopupScope {
        match self {
            Self::WorkspaceName { ws_idx } => {
                crate::model::popup_kind::PopupScope::Workspace(*ws_idx)
            }
            Self::WorkspaceSubtitle { ws_idx } => {
                crate::model::popup_kind::PopupScope::Workspace(*ws_idx)
            }
            Self::TabName { pane_id, tab_index } => {
                crate::model::popup_kind::PopupScope::Tab(*pane_id, *tab_index)
            }
            Self::ExplorerEntry { surface_id, .. } => {
                crate::model::popup_kind::PopupScope::Surface(*surface_id)
            }
            // 즐겨찾기는 전역이라 윈도우 스코프(특정 surface 에 묶이지 않음).
            Self::ExplorerAddFavorite { .. } => crate::model::popup_kind::PopupScope::Window,
            // 카테고리는 특정 워크스페이스/surface 에 묶이지 않으므로 윈도우 스코프.
            Self::NewCategory | Self::CategoryName { .. } => {
                crate::model::popup_kind::PopupScope::Window
            }
        }
    }
}

impl AppState {
    /// Memory store 의 lock 안에서 함수를 실행한다. Mutex poisoning 시
    /// poison 해제 후 inner 를 사용 — `Core::with_memory` 와 동일한 정책.
    /// state cleanup / popup draw_fn 등 Core 인자가 cascade 로 도달하지 못하는
    /// 표면에서 동일 port handle 로 접근한다.
    pub(crate) fn with_memory<R>(
        &self,
        f: impl FnOnce(&mut dyn tasty_memory::MemoryStorage) -> R,
    ) -> R {
        let mut guard = crate::poison::recover_mutex(
            self.memory.lock(),
            crate::core::MEMORY_WHAT,
            &crate::core::MEMORY_POISONED,
        );
        f(&mut *guard)
    }

    /// Creates initial state with one workspace, one pane, one tab, one terminal.
    pub fn new(
        engine: &mut CoreState,
        preset_store: std::sync::Arc<std::sync::Mutex<tasty_presets::PresetStore>>,
        memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    ) -> Self {
        let sidebar_width = engine.settings.appearance.sidebar_width;
        let active_workspace = engine.restored_active_workspace.take().unwrap_or(0);
        Self {
            preset_store,
            memory,
            active_workspace,
            category_last_active: std::collections::HashMap::new(),
            settings_open: false,
            plugins_open: false,
            #[cfg(feature = "gui")]
            settings_ui_state: SettingsUiState::new(),
            sidebar_width,
            sidebar_visible: true,
            sidebar_collapsed: false,
            #[cfg(feature = "gui")]
            pending_resize_cursor: None,
            #[cfg(feature = "gui")]
            switch_overlay: None,
            #[cfg(feature = "gui")]
            modifier_hint: crate::adapters::ui::modifier_hint_overlay::ModifierHintRuntime::default(
            ),
            #[cfg(feature = "gui")]
            tutorial: crate::adapters::ui::tutorial::TutorialRuntime::default(),
            dialogs: DialogState::new(),
            tab_bar_height: PhysicalPx(24.0),
            captured_double_tap: None,
            pending_lifecycle_events: Vec::new(),
            pending_host_events: Vec::new(),
            last_focused_surface_id: None,
            last_active_workspace_id: None,
            last_focused_tab: None,
            last_tab_locations: None,
            popup_hovered: false,
            plugin_popup_open: false,
            banner_hovered: false,
            modifier_hint_hovered: false,
            #[cfg(feature = "gui")]
            popup_layers: Vec::new(),
            #[cfg(feature = "gui")]
            plugin_popup_layers: Vec::new(),
            #[cfg(feature = "gui")]
            host_popup_hittest: Vec::new(),
            #[cfg(feature = "gui")]
            popup_escape_owner: None,
            #[cfg(feature = "gui")]
            plugin_popup_hittest: Vec::new(),
            #[cfg(feature = "gui")]
            banner_layer: None,
            #[cfg(feature = "gui")]
            modifier_hint_layer: None,
            resize_edge_widget_hovered: false,
            recent_files: crate::recent_files::RecentFiles::load(),
            #[cfg(feature = "gui")]
            popups: {
                let mut pm = crate::adapters::ui::PopupManager::new();
                let ui_zoom = engine.settings.appearance.ui_scale_factor();
                for def in crate::adapters::ui::popup::defs::all_defs() {
                    pm.register_def(def, ui_zoom);
                }
                pm
            },
            #[cfg(feature = "gui")]
            fullscreen_stage: None,
            #[cfg(feature = "gui")]
            stage_closed_queue: Vec::new(),
            #[cfg(feature = "gui")]
            stage_deferred_grid_resync: false,
            search: crate::search_state::SearchState::new(),
            #[cfg(feature = "gui")]
            port_scan: crate::adapters::ui::popup::port_scanner::PortScanState::Idle,
            #[cfg(feature = "gui")]
            port_favorites_scan: crate::adapters::ui::popup::port_scanner::PortScanState::Idle,
            command_palette: crate::state::command_palette::CommandPaletteState::default(),
            #[cfg(feature = "gui")]
            toasts: crate::adapters::ui::ToastManager::new(),
            #[cfg(feature = "gui")]
            banners: crate::adapters::ui::BannerManager::new(),
            #[cfg(feature = "gui")]
            explorer_views: Default::default(),
            #[cfg(feature = "gui")]
            dag_graph_views: Default::default(),
            tool_registry: crate::plugin::tool_registry::ToolRegistry::new(),
            palette_plugin_commands: Vec::new(),
            pending_plugin_command_invokes: Vec::new(),
            pending_tool_events: Vec::new(),
            pending_popup_opens: Vec::new(),
            pending_handler_ipc: Vec::new(),
            drop_hover: None,
            pending_file_drops: Vec::new(),
            plugin_popup_closes: Vec::new(),
            plugin_popup_focus_bumps: Vec::new(),
            plugin_banner_closes: Vec::new(),
            plugin_mesh_popup_regions: Vec::new(),
            plugin_mesh_popup_geom: std::collections::HashMap::new(),
            plugin_mesh_popup_bootstrapped: std::collections::HashSet::new(),
            plugin_mesh_popup_theme: std::collections::HashMap::new(),
            plugin_mesh_banner_regions: Vec::new(),
            plugin_mesh_banner_geom: std::collections::HashMap::new(),
            plugin_mesh_banner_bootstrapped: std::collections::HashSet::new(),
            plugin_mesh_banner_theme: std::collections::HashMap::new(),
            plugin_mesh_popup_full_requests: std::collections::HashSet::new(),
            plugin_mesh_popup_pending_repaint: std::collections::HashSet::new(),
            plugin_mesh_banner_full_requests: std::collections::HashSet::new(),
            pending_intents: Vec::new(),
        }
    }

    /// Intent 발화. `App::dispatch_pending_intents` 가 메인 루프에서 drain.
    /// UI Intent / Domain Intent 모두 본 큐로 발화 (D.3.I.3 두 큐 통합).
    pub fn dispatch_intent(&mut self, intent: crate::intent::DispatchedIntent) {
        self.pending_intents.push(intent);
    }

    /// 현재까지 발화된 Intent 를 모두 꺼내고 큐를 비운다.
    pub fn take_pending_intents(&mut self) -> Vec<crate::intent::DispatchedIntent> {
        std::mem::take(&mut self.pending_intents)
    }

    /// 파일 기반 surface 를 여는 인텐트가 수렴하는 지점에서 `kind` 의 최근 목록에 1회
    /// 기록한다. surface params 의 `file` 키를 정규화 dedup 으로 기록하며, `file` 키가
    /// 없거나 비어 있으면 no-op(파일 없는 변환·kind 만 바뀌는 케이스는 기록 대상 아님).
    ///
    /// **generic per-kind**: host 는 특정 kind 이름을 모른다. 호출부가 매니페스트
    /// `records_recent` capability 로 기록 대상 여부를 판정한 뒤 이 함수에 kind 를 넘긴다.
    ///
    /// 배치 근거: 파일-open 진입점(파일-열기 팝업·주소창 navigate·링크 클릭·convert)이
    /// 모두 `Intent::NewTab`/`Intent::ConvertSurface`(또는 file-dispatch 의 직접
    /// `CreateTab`)로 수렴하므로, 그 인텐트 계층에서 공용으로 1회 기록한다. generic
    /// surface factory 는 `AppState` 접근이 없어 여기서 처리한다.
    pub(crate) fn record_recent(&mut self, kind: &str, params: &serde_json::Value) {
        if let Some(file) = params.get("file").and_then(|v| v.as_str())
            && !file.is_empty()
        {
            self.recent_files.add(kind, file.to_string());
        }
    }

    /// `convert_requires_input` kind 의 파일 입력 팝업을 여는 요청을 enqueue 한다.
    ///
    /// host 는 kind 이름을 모른다 — registry 의 `convert_input_popup`(등록 시점에
    /// `<plugin_id>/<popup_id>` 로 qualify 됨) 데이터만 따라 `open_popup_instance` 로
    /// 여는 요청을 `pending_popup_opens` 에 넣는다(App 메인 루프가 drain →
    /// PluginManager). `convert_surface_id` 가 `Some` 이면 context 에 실어 plugin 이
    /// 제자리 변환(`markdown.navigate`), `None` 이면 새 탭으로 연다.
    ///
    /// 반환값: 요청을 enqueue 했으면 `true`, kind/팝업 미상이면 `false`(warn 로그).
    pub(crate) fn enqueue_convert_input_popup(
        &mut self,
        engine: &CoreState,
        kind: &str,
        convert_surface_id: Option<u32>,
    ) -> bool {
        let Some(popup_ref) = engine
            .surface_registry
            .get(kind)
            .and_then(|d| d.convert_input_popup.clone())
        else {
            tracing::warn!(
                "convert-input popup: kind '{kind}' has no convert_input_popup capability"
            );
            return false;
        };
        let Some((plugin_id, local_id)) = popup_ref.split_once('/') else {
            tracing::warn!("convert-input popup: malformed convert_input_popup '{popup_ref}'");
            return false;
        };
        let cwd = self
            .resolve_inherit_cwd(engine)
            .map(|p| p.to_string_lossy().into_owned());
        let mut context = serde_json::json!({ "cwd": cwd });
        if let Some(sid) = convert_surface_id {
            context["surface_id"] = serde_json::json!(sid);
        }
        self.pending_popup_opens
            .push((plugin_id.to_string(), local_id.to_string(), context));
        true
    }

    /// Returns true if any dialog with text input is open.
    pub fn has_input_dialog_open(&self) -> bool {
        self.dialogs.has_text_input_open()
    }

    /// 키/IME 를 host egui 로 들여보낼지(그리고 터미널 포워딩을 막을지) 판정한다.
    ///
    /// 같은 식을 두 게이트(`view::main` 의 egui feed, `view::main::keyboard` 의 터미널
    /// 포워딩)가 **각자** 계산하던 것을 단일 출처로 합쳤다 — 한쪽만 바뀌면 "egui 에는
    /// 먹였는데 터미널로도 갔다"(이중 처리) 또는 "egui 에 안 먹였는데 터미널도
    /// 차단"(입력 유실)이 된다.
    pub(crate) fn keyboard_overlay_open(&self) -> bool {
        #[cfg(feature = "gui")]
        let host_popup_focused = self.popups.has_focused();
        #[cfg(not(feature = "gui"))]
        let host_popup_focused = false;
        keyboard_overlay_open(
            self.settings_open,
            self.has_input_dialog_open(),
            host_popup_focused,
            self.plugin_popup_open,
        )
    }

    /// 이 plugin popup instance 가 연 host popup(자식)이 아직 살아 있는가 (ADR-0084).
    ///
    /// 소유 관계는 자식 쪽(`FilePickerRequester.owner_popup_instance`)에만 기록되므로
    /// 이 조회가 곧 단일 진실이다 — 부모 쪽에 사본을 두지 않아 둘이 어긋날 수 없다.
    /// host 는 plugin id/kind 를 보지 않는다(핵심 원칙 2 — generic 계약).
    pub(crate) fn plugin_popup_has_open_child(&self, instance_id: u64) -> bool {
        self.dialogs
            .file_picker
            .as_ref()
            .and_then(|d| d.requester.as_ref())
            .and_then(|r| r.owner_popup_instance)
            == Some(instance_id)
    }

    /// Returns true if any egui overlay is visible.
    ///
    /// plugin egui-mesh popup 도 센다 — 이 값의 소비처(webview 가리기)는 "네이티브
    /// 뷰가 egui 오버레이를 덮지 않게" 하는 목적이고, plugin popup 도 같은 wgpu
    /// 표면 위에 그려지므로 host popup 과 구분할 이유가 없다.
    pub fn has_egui_overlay_open(&self) -> bool {
        let open = self.settings_open
            || self.plugins_open
            || self.dialogs.has_any_overlay()
            || self.plugin_popup_open;
        // 전체화면 무대도 오버레이로 친다. 이 판정의 소비자 중 하나가 WebView 표시
        // 여부(`MainView::sync_webviews`)인데, WebView 는 OS 네이티브 자식 뷰라 wgpu
        // 표면 **위**에 있다 — 안 그리는 것만으로는 사라지지 않고 무대를 뚫고 나온다.
        // 반드시 `set_visible(false)` 가 필요하고, 그 게이트가 바로 이 함수다.
        #[cfg(feature = "gui")]
        let open = open || self.popups.has_any_open() || self.fullscreen_stage.is_some();
        open
    }

    /// 전체화면 무대 진입. 정의 테이블에 없는 id 는 거부하고 `false` 를 반환한다
    /// (선언하지 않은 것은 무대에 올라갈 수 없다).
    ///
    /// 이미 다른 무대가 올라와 있으면 **그 무대를 닫고**(닫힘 훅 경유) 새 무대를
    /// 올린다 — 무대는 창당 하나라는 계약을 호출부가 신경 쓰지 않아도 되게 한다.
    /// 같은 id 를 다시 열면 no-op 이다(닫았다 여는 것이 아니다).
    ///
    /// 호출부는 이 뒤에 `mark_dirty()` 로 프레임을 유도해야 한다. `AppState` 라우팅을
    /// 지나는 IPC 경로는 `dirty` 가 이미 서지만, App 레벨 경로
    /// (`debug.fullscreen.*`)는 직접 세운다.
    // 사용자 진입 경로는 popup 타이틀바의 전체화면 버튼
    // (`popup::frame::draw_popup_layer`), 에이전트 진입 경로는 debug 전용
    // `debug.fullscreen.open` (`docs/dev-guide/debug-ipc.md`).
    #[cfg(feature = "gui")]
    pub fn open_fullscreen_stage(&mut self, id: &str) -> bool {
        let Some(def) = crate::adapters::ui::fullscreen::defs::find(id) else {
            return false;
        };
        if self
            .fullscreen_stage
            .as_ref()
            .is_some_and(|s| s.id == def.id())
        {
            return true;
        }
        self.close_fullscreen_stage();
        self.fullscreen_stage = Some(crate::adapters::ui::fullscreen::StageState { id: def.id() });
        true
    }

    /// 전체화면 무대 종료 — **닫는 경로 전부가 지나는 유일한 지점**(ADR-0063 패턴).
    /// 닫힌 무대 id 를 훅 대기열에 넣고, draw 경로가 `on_close` 를 1 회 발화한다.
    /// 활성 무대가 없었으면 `false`.
    #[cfg(feature = "gui")]
    pub fn close_fullscreen_stage(&mut self) -> bool {
        match self.fullscreen_stage.take() {
            Some(stage) => {
                self.stage_closed_queue.push(stage.id);
                true
            }
            None => false,
        }
    }

    /// 지금 올라와 있는 무대 id.
    #[cfg(feature = "gui")]
    pub fn fullscreen_stage_id(&self) -> Option<crate::adapters::ui::fullscreen::StageId> {
        self.fullscreen_stage.as_ref().map(|s| s.id)
    }

    /// 전체화면 무대가 활성인지. 렌더 파이프라인 게이트가 읽는다.
    ///
    /// headless 빌드에는 무대 개념이 없으므로 항상 `false` — 무대는 화면 투영이라
    /// 대응 도메인이 없다(`docs/identity.md` §2.2).
    pub fn fullscreen_stage_active(&self) -> bool {
        #[cfg(feature = "gui")]
        {
            self.fullscreen_stage.is_some()
        }
        #[cfg(not(feature = "gui"))]
        {
            false
        }
    }

    /// 닫히는 surface 의 `(surface_id, scrollback_persist_id)` 를 추출. Tab/Pane/Workspace
    /// 닫기 전에 layout 을 한 번 walk 해 결과를 모아두고, 닫기 후에 `cleanup_surface` 에
    /// 전달한다. TerminalSurface 와 deferred EmptySurface 두 케이스를 모두 처리한다.
    pub(crate) fn collect_close_targets(
        tab: &crate::model::Tab,
        engine: &CoreState,
        out: &mut Vec<(u32, Option<String>)>,
    ) {
        tab.for_each_surface(&mut |s| {
            if let Some(ts) = s.as_any().downcast_ref::<crate::model::TerminalSurface>() {
                out.push((
                    ts.id,
                    engine
                        .terminals
                        .scrollback_persist_id(ts.id)
                        .map(str::to_string),
                ));
            } else if let Some(es) = s.as_any().downcast_ref::<crate::model::EmptySurface>() {
                let pid = es
                    .deferred_spawn()
                    .and_then(|sp| sp.scrollback_persist_id.clone());
                out.push((es.id, pid));
            } else if let Some(sid) = s.surface_id() {
                // 나머지 kind (plugin RemoteSurface / EguiMeshSurface / webview 등).
                // scrollback persist 는 없지만 cleanup_surface + surface.closed
                // lifecycle(→ 소유 plugin 에 surface.destroy 통지) 대상이다. 이 분기가
                // 없으면 cleanup 대상에서 조용히 빠져 plugin 프로세스의 per-surface
                // 상태가 영원히 남는다 (soak S6 실측: markdown 사이클당 ~30MB 누수).
                out.push((sid, None));
            }
        });
    }

    /// Clean up all state associated with a closed surface:
    /// surface metadata, per-surface host view state, and memory entries
    /// scoped to this surface (regular + secret).
    ///
    /// `persist_id` 는 닫히는 surface 가 들고 있던 `scrollback_persist_id` 필드값을
    /// 호출자가 미리 뽑아 넘긴다. `Some` 일 때만 `~/.tasty/scrollback/<id>.bin` 파일이
    /// 삭제된다.
    pub(crate) fn cleanup_surface(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
    ) {
        let mut sink = crate::close_trace::CleanupSums::default();
        self.cleanup_surface_traced(engine, surface_id, persist_id, &mut sink);
    }

    /// `cleanup_surface` 와 같은 일을 하되 단계별 소요를 `sums` 에 누적한다
    /// (close 계측 C5a~C5e). surface 마다 로그를 찍지 않고 합계만 모으는 이유는
    /// `crate::close_trace` 모듈 문서 참조.
    pub(crate) fn cleanup_surface_traced(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
        sums: &mut crate::close_trace::CleanupSums,
    ) {
        use std::time::Instant;
        sums.surfaces += 1;
        let t = Instant::now();
        Self::delete_scrollback_persist(persist_id);
        sums.scrollback_delete += t.elapsed();
        let t = Instant::now();
        self.drop_terminal(engine, surface_id);
        sums.terminal_drop += t.elapsed();
        let t = Instant::now();
        self.drop_surface_indices(engine, surface_id);
        sums.indices_drop += t.elapsed();
        let t = Instant::now();
        self.purge_surface_memory_scope(surface_id);
        sums.memory_purge += t.elapsed();
        // surface 가 사라졌으니 그 자리의 점유 흔적도 지운다. 안 지우면 레지스트리가
        // 없는 surface 를 점유 중이라고 계속 말한다(`attach.list` · `surface_held_by`).
        // 워크스페이스 락은 건드리지 않는다 — 형제 surface 는 아직 살아 있다.
        engine.attach.forget_closed_surface(surface_id);
    }

    fn delete_scrollback_persist(persist_id: Option<String>) {
        if let Some(pid) = persist_id {
            crate::scrollback_store::delete(&pid);
        }
    }

    /// **D.3.E.4.f** — TerminalStore 의 Terminal/부속 데이터 cascade 정리.
    /// store.remove 가 Terminal drop → PTY SIGHUP 발사 + busy/scrollback_persist
    /// /deferred/pending_scrollback_inject 까지 함께 정리.
    fn drop_terminal(&mut self, engine: &mut CoreState, surface_id: u32) {
        engine.pending_scrollback_inject.remove(&surface_id);
        if let Some(old_terminal) = engine.terminals.remove(surface_id) {
            drop(old_terminal); // SIGHUP — 명시 drop.
        }
    }

    /// per-surface 로 유지되는 host-side 인덱스/게이트를 모두 잊는다 — 미제거 시
    /// surface 마다 영구 누적(누수).
    fn drop_surface_indices(&mut self, engine: &mut CoreState, surface_id: u32) {
        #[cfg(feature = "gui")]
        {
            self.explorer_views.drop_view(surface_id);
            self.dag_graph_views.drop_view(surface_id);
        }
        engine.command_index.drop_surface(surface_id);
        engine.observer_router.drop_surface(surface_id);
        engine.forget_shell_integration_hint(surface_id);
        // waker dedup 게이트 제거 — 미제거 시 surface 마다 영구 누적(누수).
        if let Some(factory) = engine.waker_factory.as_ref() {
            factory.forget_surface(surface_id);
        }
    }

    /// 닫힌 surface 의 memory scope 를 통째로 정리한다 — regular + secret 양쪽.
    ///
    /// **surface close 당 `purge_scope(Scope::Surface)` 는 여기 한 번뿐이다.** 과거엔
    /// `SurfaceMetaStore::remove` 도 같은 인자로 같은 함수를 불러 surface 마다 2회
    /// 돌았고, `purge_scope` 는 매 호출 끝에 `SELECT SUM(LENGTH(value)) FROM memory`
    /// 풀스캔을 하므로 탭 N 개 워크스페이스 close 가 풀스캔 2N 회를 렌더 스레드에서
    /// 직렬로 태웠다. scope 전체 teardown 은 meta 키 네임스페이스 facade 가 아니라
    /// 이 memory 수명 경로가 소유한다 — `Scope::Surface` 에는 plugin/Lua 가 memory
    /// API 로 직접 쓴 키도 들어 있어 meta 만의 관심사가 아니기 때문이다.
    fn purge_surface_memory_scope(&mut self, surface_id: u32) {
        let scope = tasty_memory::Scope::Surface(surface_id);
        match self.with_memory(|m| m.purge_scope(&scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                surface_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-surface scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(surface_id, "memory: purge_scope failed: {e}"),
        }
    }

    // ── Workspace close — `close_case_workspace`(pane.rs)/`close_workspace_at`
    //    (workspace.rs) 공유 헬퍼. 두 함수는 "ws 안 마지막 pane 이 닫혀 workspace
    //    자체가 사라지는" 동일 로직이라 여기 모아 dedup 한다. ──

    /// 지정 workspace 의 `ClosedItem` snapshot 을 만든다(push 는 호출자 책임 —
    /// 두 호출자 모두 조건부다: `close_case_workspace` 는 `save_snapshot` 인자로,
    /// `close_workspace_at` 은 [`WorkspaceCloseOrigin`] 에서 파생한 값으로 가른다).
    fn capture_workspace_snapshot(engine: &CoreState, ws_idx: usize) -> crate::model::ClosedItem {
        let mut snap_fn = crate::core::surface_registry::snapshot_fn_for(&engine.surface_registry);
        let ws = &engine.workspaces[ws_idx];
        let terminals = &engine.terminals;
        crate::model::ClosedItem::from_workspace(ws, &mut snap_fn, &|id| terminals.get(id))
    }

    /// workspace 전체(모든 pane 의 모든 tab)의 leaf surface `(id, persist_id)` 를
    /// 제거 전에 수집한다.
    fn collect_workspace_close_targets(
        engine: &CoreState,
        ws_idx: usize,
    ) -> Vec<(u32, Option<String>)> {
        let mut targets = Vec::new();
        let ws = &engine.workspaces[ws_idx];
        for pid in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    Self::collect_close_targets(tab, engine, &mut targets);
                }
            }
        }
        targets
    }

    /// 워크스페이스가 `engine.workspaces` 에서 **제거된 직후** 반드시 도는 뒷정리 —
    /// plugin 에 나가는 `workspace.closed` 발화 + workspace scope memory purge.
    ///
    /// **제거 경로가 셋이라 초크포인트로 모았다.** 각자 쏘던 때 실제로 하나가
    /// 빠져 있었다(인라인 cascade — 마지막 터미널이 스스로 종료돼 워크스페이스가
    /// 사라지는 경로에서 `workspace.closed` 가 안 나갔다). 넷째 경로가 생겨도
    /// 여기를 지나기만 하면 같은 누락이 반복되지 않는다. 현재 호출자:
    ///
    /// - [`AppState::close_workspace_at`] — GUI 닫기 · `workspace.close` IPC
    /// - `AppState::close_case_workspace`(`state/pane.rs`) — 인라인 cascade
    /// - `app::dispatch_domain::cascade_surface_closed` — Core cascade
    ///
    /// `path` 는 close 계측의 경로 구분값(`"gui"`/`"ipc"`/`"inline"`/`"cascade"`)이다.
    pub(crate) fn after_workspace_removed(&mut self, workspace_id: u32, path: &'static str) {
        self.enqueue_host_event(PendingHostEvent::WorkspaceClosed { workspace_id });
        let t = std::time::Instant::now();
        self.purge_workspace_memory_scope(workspace_id);
        crate::close_trace::log_ws_purge(t, path);
    }

    /// workspace scope 의 memory entry 정리(안의 surface 들은 각자
    /// `cleanup_surface` 가 자기 scope 를 purge). 발화와 짝지어 돌아야 하므로
    /// 직접 부르지 말고 [`AppState::after_workspace_removed`] 를 쓴다.
    fn purge_workspace_memory_scope(&mut self, workspace_id: u32) {
        let ws_scope = tasty_memory::Scope::Workspace(workspace_id);
        match self.with_memory(|m| m.purge_scope(&ws_scope)) {
            Ok(stats) if stats.regular + stats.secret > 0 => tracing::debug!(
                workspace_id,
                regular = stats.regular,
                secret = stats.secret,
                "memory: purged closed-workspace scope",
            ),
            Ok(_) => {}
            Err(e) => tracing::warn!(workspace_id, "memory: purge_scope failed: {e}"),
        }
    }

    /// 수집된 `(surface_id, persist_id, kind)` 각각을 cleanup + lifecycle 알림
    /// enqueue. `kind` 를 어느 시점에 구하는지(remove 전/후)는 두 호출자가 서로
    /// 다르므로 — remove 후엔 `surface_kind` 가 None 을 반환할 수 있다 — 여기서
    /// 재계산하지 않고 호출자가 미리 resolve 한 값을 그대로 받는다.
    fn cleanup_targets(
        &mut self,
        engine: &mut CoreState,
        targets: Vec<(u32, Option<String>, Option<&'static str>)>,
        is_user_close: bool,
        trace: Option<&'static str>,
    ) {
        let t_loop = std::time::Instant::now();
        let mut sums = crate::close_trace::CleanupSums::default();
        for (sid, pid, kind) in targets {
            self.cleanup_surface_traced(engine, sid, pid, &mut sums);
            self.enqueue_surface_closed(sid, kind, is_user_close);
        }
        if let Some(path) = trace {
            sums.log(t_loop.elapsed(), path);
        }
    }

    /// Determine the type of the currently focused surface.
    pub fn focused_surface_type(&self, engine: &CoreState) -> FocusedSurfaceType {
        let pane = match self.focused_pane(engine) {
            Some(p) => p,
            None => return FocusedSurfaceType::None,
        };
        let tab = match pane.tabs.get(pane.active_tab) {
            Some(t) => t,
            None => return FocusedSurfaceType::None,
        };

        // Find the focused leaf surface in the layout
        if let Some(leaf) = tab.layout().find_surface(tab.focused_surface) {
            return Self::surface_to_type(leaf);
        }

        FocusedSurfaceType::None
    }

    fn surface_to_type(surface: &dyn crate::model::Surface) -> FocusedSurfaceType {
        match surface.kind() {
            "terminal" => FocusedSurfaceType::Terminal,
            other => FocusedSurfaceType::Kind(other.to_string()),
        }
    }

    // Explorer 관련 호스트 헬퍼 (focused_explorer_*, explorer_file_*, explorer_select_all
    // 등)는 ExplorerPanel과 함께 제거됨 — 동일 동작을 com.tasty.explorer plugin이
    // 자체 RemoteSurface 안에서 처리한다.

    /// Surface가 close되기 직전에 `kind` 식별자를 얻는다. plugin lifecycle 알림에
    /// payload로 채워 보낸다. None이면 lifecycle 알림을 발행하지 않는다.
    pub fn surface_kind(&self, engine: &CoreState, surface_id: u32) -> Option<&'static str> {
        engine.find_surface_by_id(surface_id).map(|s| s.kind())
    }

    /// Surface close lifecycle 알림 큐에 항목을 추가한다. App 메인 루프가
    /// `take_pending_lifecycle_events`로 drain해서 plugin manager로 dispatch한다.
    pub fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: Option<&'static str>,
        is_user_close: bool,
    ) {
        self.pending_lifecycle_events.push(PendingSurfaceClosed {
            surface_id,
            kind,
            is_user_close,
        });
    }

    /// Surface close lifecycle 큐를 비우고 항목을 반환한다.
    pub fn take_pending_lifecycle_events(&mut self) -> Vec<PendingSurfaceClosed> {
        std::mem::take(&mut self.pending_lifecycle_events)
    }

    /// Event Bus 자동 발화 큐에 항목을 추가한다.
    pub fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        self.pending_host_events.push(event);
    }

    /// Event Bus 자동 발화 큐를 비우고 항목을 반환한다.
    pub fn take_pending_host_events(&mut self) -> Vec<PendingHostEvent> {
        std::mem::take(&mut self.pending_host_events)
    }

    /// Cascade 가 host event 를 enqueue 한 직후 polling baseline 을 동기화한다.
    /// 다음 `detect_tab_lifecycle` 호출이 같은 변경을 중복 enqueue 하지 않도록
    /// baseline 을 *현재 engine 상태* 와 일치시키는 역할. baseline 이 아직 `None`
    /// (한 번도 polling 안 함) 이면 no-op — 첫 detect 가 알아서 현재 상태를
    /// 베이스라인으로 잡는다.
    pub fn lifecycle_baseline_insert_tab(
        &mut self,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    ) {
        if let Some(map) = self.last_tab_locations.as_mut() {
            map.insert(tab_id, (pane_id, workspace_id, kind));
        }
    }

    /// `lifecycle_baseline_insert_tab` 의 close 대응. 닫힌 tab 을 baseline 에서
    /// 제거해 polling 이 중복 `TabClosed` 발화하지 않도록 한다.
    pub fn lifecycle_baseline_remove_tab(&mut self, tab_id: u32) {
        if let Some(map) = self.last_tab_locations.as_mut() {
            map.remove(&tab_id);
        }
    }

    /// Get the working directory to inherit from the focused surface, if enabled.
    ///
    /// 사용자가 현재 포커스한 surface 본인의 `source_cwd()`를 사용한다.
    /// (terminal/explorer/markdown/html → 자체 cwd, image/empty/clipboard → None)
    pub(crate) fn resolve_inherit_cwd(&self, engine: &CoreState) -> Option<std::path::PathBuf> {
        if !engine.settings.general.inherit_cwd || engine.workspaces.is_empty() {
            return None;
        }
        let sid = self.focused_surface_id(engine)?;
        cwd_from_surface(engine, sid)
    }

    /// Get the working directory to inherit from a specific surface, if enabled.
    pub(crate) fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &CoreState,
        surface_id: u32,
    ) -> Option<std::path::PathBuf> {
        if !engine.settings.general.inherit_cwd {
            return None;
        }
        cwd_from_surface(engine, surface_id)
    }
}

/// surface_id 의 source_cwd 를 결정. Terminal kind 는 store 의 Terminal.get_cwd(),
/// 그 외 kind 는 Surface trait 의 default source_cwd() (markdown 은 파일 부모 등).
fn cwd_from_surface(engine: &CoreState, surface_id: u32) -> Option<std::path::PathBuf> {
    let surface = engine.find_surface_by_id(surface_id)?;
    if surface.kind() == "terminal" {
        engine.terminals.get(surface_id).and_then(|t| t.get_cwd())
    } else {
        surface.source_cwd()
    }
}

/// [`AppState::keyboard_overlay_open`] 의 순수 술어 — 상태 접근 없이 단언할 수 있게
/// 분리했다.
///
/// `plugin_popup_open` 이 술어에 들어가는 이유: plugin egui-mesh popup 은 host
/// `PopupManager` 소속이 아니라 `has_focused()` 로 잡히지 않는데, 그 popup 의 키 입력은
/// host egui 의 `ctx.input` 을 거쳐 plugin 으로 forward 된다. 게이트가 닫혀 있으면 키가
/// egui 큐에 아예 안 들어가 forward 소스가 비고, 그 키는 그대로 터미널로 샌다.
pub(crate) fn keyboard_overlay_open(
    settings_open: bool,
    input_dialog_open: bool,
    host_popup_focused: bool,
    plugin_popup_open: bool,
) -> bool {
    settings_open || input_dialog_open || host_popup_focused || plugin_popup_open
}

#[cfg(test)]
mod keyboard_overlay_tests {
    use super::keyboard_overlay_open;

    /// plugin popup 하나만 열려 있어도 키는 egui 로 가야 한다 — 이 케이스가 빠져 있어
    /// 팝업 입력창 대신 뒤 터미널에 글자가 찍혔다.
    #[test]
    fn plugin_popup_alone_opens_the_gate() {
        assert!(keyboard_overlay_open(false, false, false, true));
    }

    #[test]
    fn nothing_open_keeps_the_gate_closed() {
        assert!(!keyboard_overlay_open(false, false, false, false));
    }

    /// 기존 세 술어의 동작은 그대로다(회귀 고정).
    #[test]
    fn each_existing_predicate_still_opens_the_gate() {
        assert!(keyboard_overlay_open(true, false, false, false));
        assert!(keyboard_overlay_open(false, true, false, false));
        assert!(keyboard_overlay_open(false, false, true, false));
    }
}
