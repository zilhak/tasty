pub(crate) mod approval;
pub(crate) mod command_palette;
pub(crate) mod confirm_delete_category;
pub(crate) mod convert;
pub(crate) mod dag_list;
pub(crate) mod defs;
mod draw;
pub(crate) mod file_handler_picker;
pub(crate) mod file_picker;
pub(crate) mod frame;
pub(crate) mod occlusion;
pub(crate) mod port_scanner;
pub(crate) mod port_scanner_favorites;
pub(crate) mod preset_apply;
pub(crate) mod rail_category;
pub(crate) mod remote_attach;
pub(crate) mod remote_tool;
pub(crate) mod script_confirm;
pub(crate) mod transfer;

use crate::state::AppState;
use tasty_type_geometry::length::LogicalPx;

// 참고: 기존 `PopupContent` trait는 PopupDef(데이터 지향)로 대체되었다. 새 popup을
// 추가하려면 `popup::defs` 의 `all_defs()`에 항목을 추가하라.

pub use crate::model::popup_kind::{PopupId, PopupScope};

/// Result of a popup's draw call.
pub enum PopupAction {
    /// No action needed.
    None,
    /// The popup requests to be closed.
    Close,
}

/// 팝업이 자기 드래그(이동) 영역을 선언하는 방식. 타이틀바가 없어도 팝업이
/// `PopupState`(pos/size)로부터 전용 핸들 띠를 직접 계산할 수 있게 한다.
///
/// 핸들 영역이 클릭/드래그 위젯과 겹쳐도 안전하다 — `PopupManager::draw` 가
/// 콘텐츠 렌더 뒤 `is_using_pointer()` 로 **위젯 우선 중재**를 하므로, 어떤
/// 위젯이 프레스를 가져가면 그 프레임 이동/리사이즈는 발동하지 않는다. 따라서
/// `Region` 작성자는 헤더 띠 전체처럼 넓은 영역을 핸들로 선언해도 된다(띠 안의
/// 검색 입력·버튼 클릭은 중재로 보호됨).
#[derive(Clone, Copy)]
pub enum DragHandle {
    /// 이동 불가.
    None,
    /// 기존 동작 — 타이틀바(`title_rect`)가 핸들. headless 팝업에서는 타이틀바가
    /// 없으므로 핸들도 없음(None 과 동일).
    TitleBar,
    /// 팝업이 pos/size 로부터 전용 핸들 띠를 계산. headless 팝업도 이동 가능.
    Region(fn(&PopupState) -> egui::Rect),
}

impl std::fmt::Debug for DragHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DragHandle::None => write!(f, "DragHandle::None"),
            DragHandle::TitleBar => write!(f, "DragHandle::TitleBar"),
            DragHandle::Region(_) => write!(f, "DragHandle::Region(..)"),
        }
    }
}

/// 리사이즈 중 사용자가 잡은 테두리 엣지 조합. 모서리는 인접한 두 엣지가 함께 true.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResizeEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// Result of PopupManager::draw(), including input layer hit information.
pub struct PopupDrawResult {
    /// Popup IDs that were closed this frame.
    pub closed: Vec<PopupId>,
    /// Whether the mouse is currently over any open popup.
    pub hovered: bool,
    /// 이번 프레임에 그린 각 popup 의 `LayerId`. 중앙 집중식 z-order 강제
    /// (`enforce_foreground_z_order`, `src/gfx/gpu/egui_bridge.rs`)가 modifier-hint
    /// 레이어를 부모로 이들을 `Context::set_sublayer` 자식으로 묶어, popup 이 항상
    /// modifier-hint 바로 위에 오도록 고정할 때 쓴다.
    pub layers: Vec<egui::LayerId>,
    /// 이번 프레임에 타이틀바 전체화면 버튼이 눌린 popup 이 올리려는 무대 id.
    /// 무대 진입은 `AppState` 소유라 매니저가 직접 열지 않고 호출부
    /// (`popup::frame::draw_popup_layer`)에 되돌려준다 — close 와 같은 관례.
    pub fullscreen_requested: Option<crate::adapters::ui::fullscreen::StageId>,
    /// 이번 프레임에 **실제로 그려진**(open + scope 가시) popup 들의 히트테스트 rect 와
    /// z_seq. plugin popup 쪽 판정(`plugin_bridge::popup_render`)이 "내 위에 host popup
    /// 이 이 좌표를 덮는가" 를 보려면 host 가 그 프레임에 확정한 rect 가 필요하다 —
    /// scope 로 숨겨진 popup 은 그려지지도 않으므로 여기 담기지 않는다(숨은 popup 이
    /// 남의 클릭을 가로채는 일이 없다).
    pub hit_rects: Vec<occlusion::Occluder>,
}

/// Static, data-oriented popup definition. 등록 시점에 불변으로 고정되는 속성과
/// 매 프레임 호출되는 draw 함수. 기존 trait 기반 `PopupContent`를 대체한다.
pub struct PopupDef {
    pub id: PopupId,
    /// i18n 키. `t()`로 런타임 번역하여 popup title로 사용.
    pub title_key: &'static str,
    /// 동적 타이틀. 매 프레임 호출. `title_key` 대신 사용된다. (예: rename popup의
    /// 대상별 제목)
    pub title_fn: Option<fn(&AppState, &crate::core::CoreState) -> String>,
    /// 기본 크기. 동적 크기가 필요하면 `sizer`로 덮어쓸 수 있다.
    pub default_size: egui::Vec2,
    /// 선택적 동적 크기 계산. popup open 시점에 1회 호출되어 `PopupState.size`에 반영.
    pub sizer: Option<fn(&AppState, &crate::core::CoreState) -> egui::Vec2>,
    pub default_scope: PopupScope,
    pub close_on_outside_click: bool,
    /// true면 타이틀바·닫기 버튼 없이 콘텐츠만 렌더링한다 (컨텍스트 메뉴 스타일).
    pub headless: bool,
    /// true면 팝업 바깥 클릭해도 키보드 포커스가 유지된다.
    /// 닫기(Escape 등)로만 포커스 해제 가능. 검색 바 같은 오버레이용.
    pub sticky_focus: bool,
    /// 이동(드래그) 핸들 선언. `None`이면 이동 불가. `movable` 여부는 별도 bool 없이
    /// 이 값으로 표현한다.
    pub drag_handle: DragHandle,
    /// true면 테두리 8방향 드래그로 크기 조절 가능.
    pub resizable: bool,
    /// 리사이즈 최소 크기. `None`이면 `default_size`를 최소로 사용.
    pub min_size: Option<egui::Vec2>,
    /// 렌더링 함수. 매 프레임 호출. AppState에서 필요한 데이터를 꺼낸다.
    pub draw_fn: fn(&mut egui::Ui, &mut AppState, &mut crate::core::CoreState) -> PopupAction,
    /// 이 popup 의 타이틀바에 **전체화면 버튼**을 노출할지 + 눌렀을 때 올릴 무대 id.
    /// `None`(대부분의 popup)이면 버튼 자체가 없다 — 노출 여부와 대상이 한 필드라
    /// "버튼은 있는데 갈 곳이 없다" 는 상태를 만들 수 없다.
    ///
    /// 무대에 올라가는 것은 이 popup 인스턴스가 **아니라** 같은 형상으로 구성된
    /// 별개 콘텐츠다(`docs/design/systems/fullscreen-stage.md` §모델). 버튼을 눌러도
    /// 원본 popup 은 열린 채 남는다 — 무대가 덮으므로 보이지 않을 뿐이다.
    ///
    /// headless popup(타이틀바가 없다)은 이 값과 무관하게 버튼이 그려지지 않는다.
    pub fullscreen_stage: Option<crate::adapters::ui::fullscreen::StageId>,
    /// 닫힘 뒷정리 훅. `PopupManager::close()`(닫는 경로 전부가 거치는 유일한
    /// 지점)를 통해 어떤 경로로 닫히든 정확히 1회 발화한다(`closed_queue` +
    /// `popup::frame` 의 `drain_on_close_hooks` 참고) — draw_fn 이 `Close` 를 반환하는
    /// 경로나 X 버튼/외부 클릭에만 붙던 기존 뒷정리(`draw_popups`)와 달리
    /// `UiIntent::ClosePopup`/`TogglePopup`/App 직접 호출/debug IPC 경로도 모두
    /// 잡는다. 그리는 게 없으므로 `&mut Ui` 가 아니라 `&egui::Context` 를 받는다
    /// (`remote_attach`/`remote_tool` 의 `clear_ui(ctx)` 처럼 egui temp memory
    /// 정리가 필요한 훅이 있어서 — `Ui` 로는 접근 불가).
    pub on_close: Option<fn(&egui::Context, &mut AppState, &mut crate::core::CoreState)>,
}

/// State for a single popup instance.
#[derive(Debug, Clone)]
pub struct PopupState {
    /// Unique identifier.
    pub id: PopupId,
    /// Title text displayed in the title bar.
    pub title: String,
    /// Whether the popup is currently visible.
    pub open: bool,
    /// Position in logical pixels (top-left corner).
    pub pos: egui::Pos2,
    /// Size in logical pixels.
    pub size: egui::Vec2,
    /// Whether the popup is currently being dragged.
    dragging: bool,
    /// Drag offset from popup top-left to mouse position.
    drag_offset: egui::Vec2,
    /// Scope determines visibility and boundary clamping.
    pub scope: PopupScope,
    /// Whether this popup currently has keyboard focus.
    /// When focused, keyboard input should NOT be forwarded to the terminal.
    pub focused: bool,
    /// If true, clicking outside this popup will close it (not just unfocus).
    pub close_on_outside_click: bool,
    /// true면 타이틀바·닫기 버튼 없이 콘텐츠만 렌더링한다.
    pub headless: bool,
    /// true면 팝업 바깥 클릭해도 키보드 포커스가 유지된다.
    pub sticky_focus: bool,
    /// If true, PopupManager will center this popup on the next draw and clear the flag.
    pub request_center: bool,
    /// If true, PopupManager will position this popup at the top of its scope on the
    /// next draw (horizontally centered, small margin from top) and clear the flag.
    pub request_top: bool,
    /// 이동(드래그) 핸들 선언. `register_def`가 `PopupDef`에서 전파한다.
    drag_handle: DragHandle,
    /// 테두리 드래그 리사이즈 허용 여부.
    resizable: bool,
    /// 리사이즈 최소 크기. `default_size`로 초기화된 뒤 `with_min_size`로 덮인다.
    min_size: egui::Vec2,
    /// 리사이즈 진행 중이면 잡은 엣지 조합. `None`이면 리사이즈 중 아님.
    resizing: Option<ResizeEdges>,
    /// host↔plugin popup 통합 z-order 순번(`tasty_host_plugin::next_popup_z_seq`).
    /// open/bring-to-front 시마다 갱신되며, plugin popup(`PopupInstance.z_seq`)과 같은
    /// 전역 카운터를 공유해 서로 다른 매니저의 popup 을 하나의 순서로 비교할 수 있게
    /// 한다(`docs/design/systems/popup.md` 규칙 7). `popups: Vec` 안 위치 자체도
    /// z-order 지만(호스트 popup 끼리는 그것으로 충분) 이 필드는 plugin popup 과의
    /// cross-manager 비교에만 쓰인다.
    z_seq: u64,
    /// 리사이즈 시작 시점의 팝업 rect (드래그 누적 계산 기준).
    resize_start_rect: egui::Rect,
    /// 사용자가 한 번이라도 리사이즈했으면 true. true 동안 sizer 의 size 덮어쓰기를
    /// 막는다(`popup::frame::draw_popup_layer`). `close()`에서 리셋되어 다음 open 시 sizer 복원.
    pub size_user_overridden: bool,
    /// 타이틀바 전체화면 버튼이 올릴 무대 id. `register_def` 가 `PopupDef` 에서
    /// 전파한다. `None` 이면 버튼 없음.
    fullscreen_stage: Option<crate::adapters::ui::fullscreen::StageId>,
}

/// Popup 타이틀바 높이 — `Theme.item_height_interactive` (디자인 28px) 의 round_ui.
/// `with_colors_and_zoom` 가 토큰 자체에 host UI zoom 을 박으므로 본 함수도
/// 매 호출마다 현재 zoom 이 반영된 높이를 반환한다.
///
/// 논리 px 라 `LogicalPx` 를 반환한다. `round_ui` 는 egui 트레이트라 `f32` 위에서만
/// 도므로 그 한 줄에서만 벗기고 곧바로 다시 싼다 — 호출처가 벗기지 않게 하는 것이
/// 이 시그니처의 목적이다.
pub fn title_bar_height() -> LogicalPx {
    use egui::emath::GuiRounding as _;
    LogicalPx(
        crate::theme::theme()
            .item_height_interactive
            .value()
            .round_ui(),
    )
}

/// Popup 콘텐츠 영역 inner margin — `Theme.spacing_xs` (디자인 4px) 의 round_ui.
///
/// 논리 px 라 `LogicalPx` 를 반환한다. 사유는 [`title_bar_height`] 와 같다 — 둘은
/// popup 높이를 만드는 한 식에서 더해지므로 시그니처도 함께 넓혀야 그 식이 타입을
/// 유지한다.
pub fn content_margin() -> LogicalPx {
    use egui::emath::GuiRounding as _;
    LogicalPx(crate::theme::theme().spacing_xs.value().round_ui())
}

/// 타이틀바 우측 버튼 사이 간격 — `Theme.spacing_xs`(디자인 4px 그리드) 의 round_ui.
pub fn title_btn_gap() -> f32 {
    use egui::emath::GuiRounding as _;
    crate::theme::theme().spacing_xs.value().round_ui()
}

/// 헤더 드래그 rect 를 담는 egui temp memory Id (popup id 로 네임스페이스).
fn header_drag_rect_id(popup_id: PopupId) -> egui::Id {
    egui::Id::new("popup.header_drag_rect").with(popup_id)
}

/// 뷰가 자신의 실측 헤더 rect(전체폭 × 실제 헤더 높이)를 egui temp memory 에 보고한다.
///
/// headless 패널 팝업(port_scanner / remote_tool)은 헤더 높이가 서로 다르고 host UI
/// zoom 에도 좌우된다. 정적 리터럴로 추정하는 대신 각 뷰가 렌더 시점의 실제 rect 를
/// 여기로 보고하면, `PopupManager::draw` 의 hit-test 가 이 rect 를 드래그 핸들로
/// 우선 사용해 헤더 전체를 이동 영역으로 만든다. 매 프레임 재보고되므로 stale 위험이
/// 낮고, popup id 로 네임스페이스해 팝업 간 rect 가 섞이지 않는다.
pub fn report_header_drag_rect(ctx: &egui::Context, popup_id: PopupId, rect: egui::Rect) {
    ctx.memory_mut(|m| m.data.insert_temp(header_drag_rect_id(popup_id), rect));
}

/// 뷰가 보고한 헤더 드래그 rect 를 읽는다(hit-test 용). 아직 보고 전이면 None.
fn reported_header_drag_rect(ctx: &egui::Context, popup_id: PopupId) -> Option<egui::Rect> {
    ctx.memory(|m| m.data.get_temp(header_drag_rect_id(popup_id)))
}

/// `draw_fn` 내부에서 egui 네이티브 API(`popup_below_widget` 등)로 그려지는 자식
/// 오버레이(드롭다운) rect 레지스트리를 담는 egui temp memory Id. 전역 단일 슬롯 —
/// 개별 오버레이는 `overlay_key` 로 서로 구분되어 report 호출 순서와 무관하게
/// 클로버링되지 않는다(예: port_scanner 는 state_filter/column_chooser 두 드롭다운을
/// 매 프레임 함께 report 한다).
fn child_overlay_registry_id() -> egui::Id {
    egui::Id::new("popup.child_overlay_registry")
}

type ChildOverlayMap = std::collections::HashMap<&'static str, (PopupId, egui::Rect)>;

/// 자식 오버레이(드롭다운 등)의 실측 rect 를 보고한다. `PopupManager::draw` 의
/// outside-click/hover 판정이 부모 `popup_rect` 뿐 아니라 이 rect 도 히트테스트에
/// 포함해, 드롭다운이 팝업 경계를 넘어가도 그 위 클릭이 "바깥 클릭"으로 오판되지
/// 않는다. `overlay_key` 는 오버레이별 고유 문자열(예: 드롭다운의 egui popup id
/// 문자열) — 같은 `popup_id` 에 오버레이가 여러 개(port_scanner 의 state_filter +
/// column_chooser)여도 서로 덮어쓰지 않는다. 오버레이가 닫히면 반드시 `None` 으로
/// 보고해 stale rect 가 남지 않게 한다.
pub fn report_child_overlay_rect(
    ctx: &egui::Context,
    popup_id: PopupId,
    overlay_key: &'static str,
    rect: Option<egui::Rect>,
) {
    let id = child_overlay_registry_id();
    ctx.memory_mut(|m| {
        let mut map: ChildOverlayMap = m.data.get_temp(id).unwrap_or_default();
        match rect {
            Some(r) => {
                map.insert(overlay_key, (popup_id, r));
            }
            None => {
                map.remove(overlay_key);
            }
        }
        m.data.insert_temp(id, map);
    });
}

/// `popup_id` 소유의 자식 오버레이(`overlay_key`)가 지금 열려 있는가.
///
/// Esc 우선순위 판정용이다 — 드롭다운이 열려 있으면 Esc 는 그것만 닫고 부모 popup 은
/// 유지해야 하는데, 부모의 Esc 가드는 프레임 앞머리(본문을 그리기 전)에서 키를 읽으므로
/// 드롭다운이 자기 Esc 를 소비할 기회를 얻기 전에 창을 닫아버린다. 레지스트리 등록
/// 자체가 "지금 열려 있음" 이라 별도 플래그 없이 여기서 양보 여부를 판정할 수 있다
/// (rect 는 매 프레임 report 되고, 닫히면 `None` 으로 지워진다).
pub fn child_overlay_open(
    ctx: &egui::Context,
    popup_id: PopupId,
    overlay_key: &'static str,
) -> bool {
    let id = child_overlay_registry_id();
    ctx.memory(|m| {
        let map: ChildOverlayMap = m.data.get_temp(id).unwrap_or_default();
        map.get(overlay_key)
            .is_some_and(|(pid, _)| *pid == popup_id)
    })
}

/// `popup_id` 소유의 자식 오버레이 중 `pos` 를 포함하는 것이 있는지 hit-test.
fn child_overlay_hit(ctx: &egui::Context, popup_id: PopupId, pos: egui::Pos2) -> bool {
    let id = child_overlay_registry_id();
    ctx.memory(|m| {
        let map: ChildOverlayMap = m.data.get_temp(id).unwrap_or_default();
        map.values()
            .any(|(pid, rect)| *pid == popup_id && rect.contains(pos))
    })
}

impl PopupState {
    pub fn new(id: PopupId, title: impl Into<String>, default_size: egui::Vec2) -> Self {
        Self {
            id,
            title: title.into(),
            open: false,
            pos: egui::pos2(100.0, 100.0),
            size: default_size,
            dragging: false,
            drag_offset: egui::Vec2::ZERO,
            scope: PopupScope::Window,
            focused: false,
            close_on_outside_click: false,
            headless: false,
            sticky_focus: false,
            request_center: false,
            request_top: false,
            // 기본값 TitleBar — `register_def` 미경유로 직접 생성되는 팝업
            // (settings keybinding_conflict 등 타이틀바 팝업)의 기존 드래그 동작 보존.
            drag_handle: DragHandle::TitleBar,
            resizable: false,
            min_size: default_size,
            resizing: None,
            z_seq: 0,
            resize_start_rect: egui::Rect::ZERO,
            size_user_overridden: false,
            // 기본값 None — `register_def` 미경유로 직접 생성되는 팝업에는 버튼이
            // 붙지 않는다(기존 타이틀바 렌더 무변경).
            fullscreen_stage: None,
        }
    }

    /// Create a popup with a specific scope.
    pub fn with_scope(mut self, scope: PopupScope) -> Self {
        self.scope = scope;
        self
    }

    /// Set whether clicking outside this popup should close it.
    pub fn with_close_on_outside_click(mut self, v: bool) -> Self {
        self.close_on_outside_click = v;
        self
    }

    /// Set headless mode (no title bar / close button).
    pub fn with_headless(mut self, v: bool) -> Self {
        self.headless = v;
        self
    }

    /// Set sticky focus (keyboard focus persists even when clicking outside).
    pub fn with_sticky_focus(mut self, v: bool) -> Self {
        self.sticky_focus = v;
        self
    }

    /// 드래그(이동) 핸들 선언을 설정한다.
    pub fn with_drag_handle(mut self, h: DragHandle) -> Self {
        self.drag_handle = h;
        self
    }

    /// 테두리 드래그 리사이즈 허용 여부를 설정한다.
    pub fn with_resizable(mut self, v: bool) -> Self {
        self.resizable = v;
        self
    }

    /// 리사이즈 최소 크기를 설정한다.
    pub fn with_min_size(mut self, sz: egui::Vec2) -> Self {
        self.min_size = sz;
        self
    }

    /// 타이틀바 전체화면 버튼이 올릴 무대를 설정한다. `None` 이면 버튼 없음.
    pub fn with_fullscreen_stage(
        mut self,
        stage: Option<crate::adapters::ui::fullscreen::StageId>,
    ) -> Self {
        self.fullscreen_stage = stage;
        self
    }

    fn popup_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, self.size)
    }

    fn title_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(
            self.pos,
            egui::vec2(self.size.x, title_bar_height().value()),
        )
    }

    /// 현재 이동(드래그) 핸들 영역. `None`이면 이동 불가.
    /// - `TitleBar`: 타이틀바(`title_rect`). 단 headless 면 타이틀바가 없으므로 None.
    /// - `Region(f)`: 팝업이 pos/size 로부터 계산한 전용 핸들 띠.
    fn drag_handle_rect(&self) -> Option<egui::Rect> {
        match self.drag_handle {
            DragHandle::None => None,
            DragHandle::TitleBar => {
                if self.headless {
                    None
                } else {
                    Some(self.title_rect())
                }
            }
            DragHandle::Region(f) => Some(f(self)),
        }
    }

    /// hit-test 가 실제로 쓰는 이동 핸들 rect. 뷰가 `report_header_drag_rect` 로
    /// 보고한 실측 헤더 rect 가 있으면 그것을(헤더 전체), 없으면 정적 선언
    /// (`drag_handle_rect`)으로 폴백한다. 보고는 hit-test 보다 뒤(콘텐츠 렌더 시점)라
    /// 이번 프레임엔 직전 프레임 값을 쓴다(1프레임 지연, 사실상 인지 불가). open
    /// 첫 프레임엔 보고가 없어 기존 핸들 띠로 폴백한다.
    fn effective_drag_handle_rect(&self, ctx: &egui::Context) -> Option<egui::Rect> {
        reported_header_drag_rect(ctx, self.id).or_else(|| self.drag_handle_rect())
    }

    fn content_rect(&self) -> egui::Rect {
        let popup = self.popup_rect();
        // 디자인상 컨테이너 패딩이 0 이고 각 구역이 자체 패딩을 가지는 popup 은
        // content_margin 을 0 으로 둬 draw_fn 이 popup 가장자리부터 구역별 패딩을
        // 직접 준다 (design-parity: 통짜 패딩으로 뭉개지 않기 위함).
        let margin = if matches!(
            self.id,
            "remote_tool"
                | "remote_attach"
                | "command_palette"
                | "port_scanner"
                | transfer::TRANSFER_PROGRESS_POPUP_ID
                | transfer::TRANSFER_ERROR_POPUP_ID
        ) {
            LogicalPx(0.0)
        } else {
            content_margin()
        };
        let top_offset = if self.headless {
            margin
        } else {
            title_bar_height() + margin
        };
        egui::Rect::from_min_max(
            egui::pos2(
                popup.min.x + margin.value(),
                popup.min.y + top_offset.value(),
            ),
            egui::pos2(popup.max.x - margin.value(), popup.max.y - margin.value()),
        )
    }

    fn close_btn_rect(&self) -> egui::Rect {
        let title = self.title_rect();
        // 버튼 한 변과 우측 끝 여백. 종전에는 둘 다 이 함수 안의 리터럴(20.0 · 4.0)
        // 이었다 — 갤러리에는 이미 이름이 있었는데 본체가 그것을 모르고 있었다.
        // 배율을 먹이는 이유는 `POPUP_TITLE_BTN_SIZE` 의 doc 에 있다(그릇과 내용).
        let th = crate::theme::theme();
        let size = super::zoomed_px(&th, tasty_ui_widgets::tokens::POPUP_TITLE_BTN_SIZE).value();
        let edge_pad = th.spacing_xs.value();
        let center = egui::pos2(title.max.x - size * 0.5 - edge_pad, title.center().y);
        egui::Rect::from_center_size(center, egui::vec2(size, size))
    }

    /// 타이틀바 전체화면 버튼 rect. 버튼이 없으면 `None` — **두 조건 중 하나라도**
    /// 걸리면 그려지지 않는다: headless(타이틀바 자체가 없다) / 무대 미지정.
    ///
    /// close 버튼과 같은 크기로 그 **왼쪽**에 [`title_btn_gap`] 만큼 띄워 놓는다
    /// (close 는 타이틀바 우측 끝 고정이라 이 rect 가 생겨도 움직이지 않는다 —
    /// 버튼을 달지 않은 popup 의 타이틀바가 변하지 않는 이유).
    fn fullscreen_btn_rect(&self) -> Option<egui::Rect> {
        if self.headless || self.fullscreen_stage.is_none() {
            return None;
        }
        let close = self.close_btn_rect();
        Some(egui::Rect::from_center_size(
            egui::pos2(
                close.center().x - close.width() - title_btn_gap(),
                close.center().y,
            ),
            close.size(),
        ))
    }

    /// 제목 텍스트가 침범하면 안 되는 **우측 버튼군의 왼쪽 경계**. 버튼이 하나면
    /// close 버튼의 좌변, 둘이면 전체화면 버튼의 좌변이다 — 제목 elide 가용 폭이
    /// 이 값을 기준으로 잡히므로 버튼이 늘면 제목이 그만큼 일찍 줄어든다.
    fn title_buttons_left_x(&self) -> f32 {
        self.fullscreen_btn_rect()
            .unwrap_or_else(|| self.close_btn_rect())
            .min
            .x
    }

    /// Clamp position so popup stays within the given screen rect.
    fn clamp_to_screen(&mut self, screen: egui::Rect) {
        self.size.x = self.size.x.min(screen.width());
        self.size.y = self.size.y.min(screen.height());
        self.pos.x = self
            .pos
            .x
            .clamp(screen.min.x, (screen.max.x - self.size.x).max(screen.min.x));
        self.pos.y = self
            .pos
            .y
            .clamp(screen.min.y, (screen.max.y - self.size.y).max(screen.min.y));
    }
}

/// Manager for all internal popups. Handles z-ordering, dragging, and window clamping.
pub struct PopupManager {
    /// Popups in z-order (last = topmost).
    popups: Vec<PopupState>,
    /// `close()` 로 실제로 닫힌(= 그 호출 직전엔 열려 있던) popup id 대기열.
    /// `on_close` 훅 drain 이 프레임당 1회 `take_closed_queue()` 로 비운다.
    closed_queue: Vec<PopupId>,
}

impl PopupManager {
    pub fn new() -> Self {
        Self {
            popups: Vec::new(),
            closed_queue: Vec::new(),
        }
    }

    /// Register a popup from a PopupDef. title은 `t()`로 번역하여 사용하며,
    /// 이후 locale이 바뀌면 draw 루프에서 재번역된다(draw_popups 참고).
    ///
    /// `ui_zoom` 은 host UI zoom 배율 (medium=1.0). sizer 가 없는 popup 의 초기
    /// default_size 에만 곱해진다 — sizer 가 있는 popup 은 sizer 가 매 프레임
    /// 직접 zoomed token 으로 재계산하므로 추가 곱셈하면 이중 곱셈이 된다.
    pub fn register_def(&mut self, def: &PopupDef, ui_zoom: f32) {
        if self.popups.iter().any(|p| p.id == def.id) {
            return;
        }
        let initial_size = if def.sizer.is_some() {
            def.default_size
        } else {
            def.default_size * ui_zoom
        };
        // min_size 도 sizer 없는 팝업은 default 와 동일하게 ui_zoom 을 곱해 baseline 정합.
        let resolved_min = def.min_size.unwrap_or(def.default_size);
        let resolved_min = if def.sizer.is_some() {
            resolved_min
        } else {
            resolved_min * ui_zoom
        };
        let popup = PopupState::new(def.id, crate::i18n::t(def.title_key), initial_size)
            .with_scope(def.default_scope.clone())
            .with_close_on_outside_click(def.close_on_outside_click)
            .with_headless(def.headless)
            .with_sticky_focus(def.sticky_focus)
            .with_drag_handle(def.drag_handle)
            .with_resizable(def.resizable)
            .with_min_size(resolved_min)
            .with_fullscreen_stage(def.fullscreen_stage);
        self.popups.push(popup);
    }

    /// Register a popup. Call once during init. Does nothing if already registered.
    pub fn register(&mut self, popup: PopupState) {
        if !self.popups.iter().any(|p| p.id == popup.id) {
            self.popups.push(popup);
        }
    }

    /// Open a popup by id, bringing it to the front.
    pub fn open(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].z_seq = tasty_host_plugin::next_popup_z_seq();
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Open a popup centered on screen, with focus.
    pub fn open_centered_focused(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].focused = true;
            self.popups[i].request_center = true;
            self.popups[i].z_seq = tasty_host_plugin::next_popup_z_seq();
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Open a popup centered on screen **without** focus (agent-initiated).
    /// 사용자의 포커스를 훔치지 않는다. CLI/IPC 경유 open에 사용.
    pub fn open_centered(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].focused = false;
            self.popups[i].request_center = true;
            self.popups[i].z_seq = tasty_host_plugin::next_popup_z_seq();
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Open a popup centered within a specific scope, with focus.
    pub fn open_with_scope(&mut self, id: PopupId, scope: PopupScope) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].focused = true;
            self.popups[i].request_center = true;
            self.popups[i].scope = scope;
            self.popups[i].z_seq = tasty_host_plugin::next_popup_z_seq();
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Open a popup at the top of a specific scope, with focus.
    /// scope rect의 상단에 가로 중앙 정렬로 배치한다.
    pub fn open_at_top_of_scope(&mut self, id: PopupId, scope: PopupScope) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].focused = true;
            self.popups[i].request_center = false;
            self.popups[i].request_top = true;
            self.popups[i].scope = scope;
            self.popups[i].z_seq = tasty_host_plugin::next_popup_z_seq();
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Open a popup at a specific position, with focus.
    pub fn open_at_focused(&mut self, id: PopupId, pos: egui::Pos2) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            self.popups[i].open = true;
            self.popups[i].focused = true;
            self.popups[i].pos = pos;
            self.popups[i].request_center = false;
            self.popups[i].z_seq = tasty_host_plugin::next_popup_z_seq();
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Close a popup by id.
    ///
    /// **모든 close 경로가 거치는 유일한 지점**(`grep "open = false"` 1건) —
    /// `on_close` 훅 발화 지점이기도 하다. 이미 닫혀 있던 popup 에 다시 호출되면
    /// (예: 같은 프레임에 여러 경로가 겹치는 경우) 중복 발화를 막기 위해
    /// `closed_queue` 에 push 하지 않는다 — `p.open` 이 이 호출 **직전** 이미
    /// `false` 였다면 이번 호출은 실질적인 전이가 아니다.
    pub fn close(&mut self, id: PopupId) {
        if let Some(p) = self.popups.iter_mut().find(|p| p.id == id) {
            let was_open = p.open;
            p.open = false;
            p.dragging = false;
            p.focused = false;
            // 리사이즈 상태 리셋 → 다음 open 시 sizer 가 크기를 다시 결정하도록 복원.
            p.resizing = None;
            p.size_user_overridden = false;
            if was_open {
                self.closed_queue.push(id);
            }
        }
    }

    /// 진행 중인 포인터 제스처(이동 드래그 · 테두리 리사이즈)를 **확정하지 않고**
    /// 폐기한다. popup 을 닫지 않고 상태도 되돌리지 않는다 — 지금까지 따라온 위치/
    /// 크기는 그대로 두고 "잡고 있음" 만 푼다.
    ///
    /// 전체화면 무대처럼 popup 이 그려지지 않는 프레임으로 전환될 때 필요하다.
    /// 드래그/리사이즈 해제는 `draw()` 안에서 release 를 보고 일어나는데, 그리지
    /// 않는 동안에는 그 코드가 돌지 않아 popup 이 커서에 붙어 다니는 상태로 남는다.
    pub fn cancel_pointer_interactions(&mut self) {
        for p in &mut self.popups {
            p.dragging = false;
            p.resizing = None;
        }
    }

    /// `closed_queue` 를 비우고 반환한다. `on_close` 훅 drain(`popup::frame`)이
    /// 프레임당 1회 호출 — 재진입(훅이 다른 popup 을 닫음)을 지원하려면 호출자가
    /// 반환값을 순회하는 동안 `state.popups` 를 다시 만질 수 있어야 하므로, 이 fn
    /// 자체는 순회를 하지 않고 `mem::take` 만 한다.
    pub fn take_closed_queue(&mut self) -> Vec<PopupId> {
        std::mem::take(&mut self.closed_queue)
    }

    /// Check if a popup is open.
    pub fn is_open(&self, id: PopupId) -> bool {
        self.popups.iter().any(|p| p.id == id && p.open)
    }

    /// Check if any popup currently has keyboard focus.
    pub fn has_focused(&self) -> bool {
        self.popups.iter().any(|p| p.open && p.focused)
    }

    /// Check whether a specific popup currently has keyboard focus.
    pub fn is_focused(&self, id: PopupId) -> bool {
        self.popups
            .iter()
            .any(|p| p.id == id && p.open && p.focused)
    }

    /// Set the keyboard-focus flag of a specific popup. 다른 popup 의 포커스는
    /// 건드리지 않는다 (검색창↔터미널 포커스 토글용).
    pub fn set_focused(&mut self, id: PopupId, focused: bool) {
        if let Some(p) = self.popups.iter_mut().find(|p| p.id == id) {
            p.focused = focused;
        }
    }

    /// Check if any popup is currently open.
    pub fn has_any_open(&self) -> bool {
        self.popups.iter().any(|p| p.open)
    }

    /// Bring a popup to the front (topmost z-order). 클릭에 의한 승격도 open() 계열과
    /// 같은 전역 순번을 받아야 plugin popup 과의 비교가 정확해진다(규칙 7 "클릭된 것이 앞").
    fn bring_to_front(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            let mut popup = self.popups.remove(i);
            popup.z_seq = tasty_host_plugin::next_popup_z_seq();
            self.popups.push(popup);
        }
    }

    /// 현재 열려 있는 host popup 중 가장 큰 z_seq(=가장 최근에 열리거나 클릭된 것).
    /// plugin popup 쪽 z_seq 최댓값과 비교해 셸/콘텐츠 렌더 순서를 정하는 데 쓰인다
    /// (`docs/design/systems/popup.md` 규칙 7, `gfx/gpu/egui_bridge.rs`).
    pub fn max_open_z_seq(&self) -> Option<u64> {
        self.popups.iter().filter(|p| p.open).map(|p| p.z_seq).max()
    }

    /// 열려 있는 popup 의 `(z_seq, 화면 rect)`. 닫혀 있거나 없으면 `None`.
    ///
    /// `PopupState` 내부(z_seq / `popup_rect`)를 밖으로 넓히지 않고 debug 관찰면
    /// (`debug.host_popup.list`)에 필요한 만큼만 내주는 좁은 접근자다.
    // 이유: 호출부가 debug.rs(`#[cfg(debug_assertions)]`)뿐이라 release 빌드에서
    // 미사용으로 잡힌다.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn open_geometry(&self, id: PopupId) -> Option<(u64, egui::Rect)> {
        self.popups
            .iter()
            .find(|p| p.id == id && p.open)
            .map(|p| (p.z_seq, p.popup_rect()))
    }

    /// Get mutable access to a popup's state.
    pub fn get_mut(&mut self, id: PopupId) -> Option<&mut PopupState> {
        self.popups.iter_mut().find(|p| p.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY_ID: PopupId = "on_close_hook_test_dummy";

    fn dummy(close_on_outside_click: bool) -> PopupState {
        PopupState::new(DUMMY_ID, "dummy", egui::vec2(200.0, 100.0))
            .with_close_on_outside_click(close_on_outside_click)
    }

    /// 타이틀바 전체화면 버튼용 더미 — 크기/위치를 고정해 rect 산술을 단정한다.
    fn titled(
        stage: Option<crate::adapters::ui::fullscreen::StageId>,
        headless: bool,
    ) -> PopupState {
        PopupState::new(DUMMY_ID, "dummy", egui::vec2(300.0, 200.0))
            .with_headless(headless)
            .with_fullscreen_stage(stage)
    }

    /// 버튼은 **무대를 선언한 non-headless popup 에만** 그려진다. 플래그가 없으면
    /// (대부분의 popup) rect 자체가 없어 렌더/hit-test 블록이 통째로 돌지 않는다.
    #[test]
    fn fullscreen_button_rect_only_when_flag_set() {
        assert!(titled(Some("blank"), false).fullscreen_btn_rect().is_some());
        assert!(titled(None, false).fullscreen_btn_rect().is_none());
        // headless 는 타이틀바 자체가 없다 — 플래그와 무관하게 버튼 없음.
        assert!(titled(Some("blank"), true).fullscreen_btn_rect().is_none());
    }

    /// 버튼이 늘어도 **close 버튼은 움직이지 않는다** — 버튼을 달지 않은 popup 의
    /// 타이틀바가 이전과 같아야 한다는 요구를 rect 수준에서 고정한다.
    #[test]
    fn close_button_rect_is_untouched_by_the_fullscreen_button() {
        assert_eq!(
            titled(Some("blank"), false).close_btn_rect(),
            titled(None, false).close_btn_rect()
        );
    }

    /// 제목 elide 기준(우측 버튼군의 좌변)은 버튼이 없으면 close 좌변 그대로이고,
    /// 버튼이 생기면 정확히 "버튼 폭 + 간격" 만큼 왼쪽으로 이동한다.
    #[test]
    fn title_elide_basis_accounts_for_the_fullscreen_button() {
        let without = titled(None, false);
        assert_eq!(
            without.title_buttons_left_x(),
            without.close_btn_rect().min.x
        );

        let with = titled(Some("blank"), false);
        let close = with.close_btn_rect();
        assert_eq!(
            with.title_buttons_left_x(),
            close.min.x - close.width() - title_btn_gap()
        );
        // 두 버튼은 겹치지 않는다.
        assert!(with.fullscreen_btn_rect().unwrap().max.x <= close.min.x);
    }

    /// `close()` 는 호출 직전 `open` 이었던 popup 만 `closed_queue` 에 push 한다.
    #[test]
    fn close_pushes_to_queue_only_when_was_open() {
        let mut mgr = PopupManager::new();
        mgr.register(dummy(false));

        // 아직 열지 않은 상태에서 close() — 중복 발화 방지 가드가 push 를 막아야 한다.
        mgr.close(DUMMY_ID);
        assert!(mgr.take_closed_queue().is_empty());

        mgr.open(DUMMY_ID);
        mgr.close(DUMMY_ID);
        assert_eq!(mgr.take_closed_queue(), vec![DUMMY_ID]);
    }

    /// 이미 닫힌 popup 에 close() 를 다시 호출해도 큐가 다시 채워지지 않는다
    /// (같은 프레임에 여러 경로가 겹쳐 호출돼도 훅이 중복 발화하지 않아야 함).
    #[test]
    fn close_on_already_closed_popup_does_not_repush() {
        let mut mgr = PopupManager::new();
        mgr.register(dummy(false));
        mgr.open(DUMMY_ID);
        mgr.close(DUMMY_ID);
        assert_eq!(mgr.take_closed_queue(), vec![DUMMY_ID]);

        mgr.close(DUMMY_ID); // 이미 닫힌 상태에서 재호출.
        assert!(mgr.take_closed_queue().is_empty());
    }

    /// `take_closed_queue()` 는 호출 시점의 큐 내용을 비워서 반환한다 — 연속
    /// 호출 시 두 번째는 항상 빈 벡터.
    #[test]
    fn take_closed_queue_drains() {
        let mut mgr = PopupManager::new();
        mgr.register(dummy(false));
        mgr.open(DUMMY_ID);
        mgr.close(DUMMY_ID);

        assert_eq!(mgr.take_closed_queue(), vec![DUMMY_ID]);
        assert!(mgr.take_closed_queue().is_empty());
    }

    /// close 경로 2(외부 클릭) — `PopupManager::draw()` 자체의 포인터 처리가
    /// `self.close(id)` 를 거쳐 `closed_queue` 를 채우는지 확인
    /// (`popup/draw.rs` 의 "Apply close" 블록). 이 경로는 `defs::all_defs()` 나
    /// draw_fn 과 무관하게 매니저 내부에서만 일어나므로 더미 popup 으로 충분하다.
    #[test]
    fn outside_click_close_path_pushes_to_queue() {
        let mut mgr = PopupManager::new();
        mgr.register(dummy(true));
        mgr.open_at_focused(DUMMY_ID, egui::pos2(500.0, 500.0));

        let ctx = egui::Context::default();
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1920.0, 1080.0),
            )),
            ..Default::default()
        };
        // 팝업(rect ≈ [500,500]-[700,600]) 에서 멀리 떨어진 바깥 좌표 클릭.
        raw.events.push(egui::Event::PointerButton {
            pos: egui::pos2(10.0, 10.0),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });

        drop(ctx.run(raw, |ctx| {
            mgr.draw(ctx, &mut |_, _| {}, None, &[]);
        }));

        assert_eq!(mgr.take_closed_queue(), vec![DUMMY_ID]);
    }
}
