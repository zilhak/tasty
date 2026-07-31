pub(crate) mod approval;
pub(crate) mod command_palette;
pub(crate) mod confirm_delete_category;
pub(crate) mod convert;
pub(crate) mod defs;
mod draw;
pub(crate) mod file_handler_picker;
pub(crate) mod file_picker;
pub(crate) mod port_scanner;
pub(crate) mod preset_apply;
pub(crate) mod rail_category;
pub(crate) mod remote_attach;
pub(crate) mod remote_tool;
pub(crate) mod script_confirm;
pub(crate) mod transfer;

use crate::state::AppState;

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
    /// 리사이즈 시작 시점의 팝업 rect (드래그 누적 계산 기준).
    resize_start_rect: egui::Rect,
    /// 사용자가 한 번이라도 리사이즈했으면 true. true 동안 sizer 의 size 덮어쓰기를
    /// 막는다(notification.rs). `close()`에서 리셋되어 다음 open 시 sizer 복원.
    pub size_user_overridden: bool,
}

/// Popup 타이틀바 높이 — `Theme.item_height_interactive` (디자인 28px) 의 round_ui.
/// `with_colors_and_zoom` 가 토큰 자체에 host UI zoom 을 박으므로 본 함수도
/// 매 호출마다 현재 zoom 이 반영된 높이를 반환한다. const 였던 시절과 시그니처
/// 호환을 위해 `f32` 반환.
pub fn title_bar_height() -> f32 {
    use egui::emath::GuiRounding as _;
    crate::theme::theme()
        .item_height_interactive
        .value()
        .round_ui()
}

/// Popup 콘텐츠 영역 inner margin — `Theme.spacing_xs` (디자인 4px) 의 round_ui.
pub fn content_margin() -> f32 {
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
            resize_start_rect: egui::Rect::ZERO,
            size_user_overridden: false,
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

    fn popup_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, self.size)
    }

    fn title_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, egui::vec2(self.size.x, title_bar_height()))
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
            0.0
        } else {
            content_margin()
        };
        let top_offset = if self.headless {
            margin
        } else {
            title_bar_height() + margin
        };
        egui::Rect::from_min_max(
            egui::pos2(popup.min.x + margin, popup.min.y + top_offset),
            egui::pos2(popup.max.x - margin, popup.max.y - margin),
        )
    }

    fn close_btn_rect(&self) -> egui::Rect {
        let title = self.title_rect();
        let size = 20.0;
        let center = egui::pos2(title.max.x - size * 0.5 - 4.0, title.center().y);
        egui::Rect::from_center_size(center, egui::vec2(size, size))
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
}

impl PopupManager {
    pub fn new() -> Self {
        Self { popups: Vec::new() }
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
            .with_min_size(resolved_min);
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
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Close a popup by id.
    pub fn close(&mut self, id: PopupId) {
        if let Some(p) = self.popups.iter_mut().find(|p| p.id == id) {
            p.open = false;
            p.dragging = false;
            p.focused = false;
            // 리사이즈 상태 리셋 → 다음 open 시 sizer 가 크기를 다시 결정하도록 복원.
            p.resizing = None;
            p.size_user_overridden = false;
        }
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

    /// Bring a popup to the front (topmost z-order).
    fn bring_to_front(&mut self, id: PopupId) {
        if let Some(i) = self.popups.iter().position(|p| p.id == id) {
            let popup = self.popups.remove(i);
            self.popups.push(popup);
        }
    }

    /// Get mutable access to a popup's state.
    pub fn get_mut(&mut self, id: PopupId) -> Option<&mut PopupState> {
        self.popups.iter_mut().find(|p| p.id == id)
    }
}
