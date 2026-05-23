pub(crate) mod approval;
pub(crate) mod command_palette;
pub(crate) mod convert;
pub(crate) mod defs;
mod draw;
pub(crate) mod file_handler_picker;
pub(crate) mod file_open;
pub(crate) mod port_scanner;
pub(crate) mod preset_apply;
pub(crate) mod update;

use crate::state::AppState;

// 참고: 기존 `PopupContent` trait는 PopupDef(데이터 지향)로 대체되었다. 새 popup을
// 추가하려면 `popup::defs` 의 `all_defs()`에 항목을 추가하라.

/// Unique identifier for a popup instance.
pub type PopupId = &'static str;

/// Result of a popup's draw call.
pub enum PopupAction {
    /// No action needed.
    None,
    /// The popup requests to be closed.
    Close,
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
    pub title_fn: Option<fn(&AppState, &crate::engine_state::EngineState) -> String>,
    /// 기본 크기. 동적 크기가 필요하면 `sizer`로 덮어쓸 수 있다.
    pub default_size: egui::Vec2,
    /// 선택적 동적 크기 계산. popup open 시점에 1회 호출되어 `PopupState.size`에 반영.
    pub sizer: Option<fn(&AppState, &crate::engine_state::EngineState) -> egui::Vec2>,
    pub default_scope: PopupScope,
    pub close_on_outside_click: bool,
    /// true면 타이틀바·닫기 버튼 없이 콘텐츠만 렌더링한다 (컨텍스트 메뉴 스타일).
    pub headless: bool,
    /// true면 팝업 바깥 클릭해도 키보드 포커스가 유지된다.
    /// 닫기(Escape 등)로만 포커스 해제 가능. 검색 바 같은 오버레이용.
    pub sticky_focus: bool,
    /// 렌더링 함수. 매 프레임 호출. AppState에서 필요한 데이터를 꺼낸다.
    pub draw_fn:
        fn(&mut egui::Ui, &mut AppState, &mut crate::engine_state::EngineState) -> PopupAction,
}

/// Scope determines where a popup is anchored and when it's visible.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupScope {
    /// Always visible, clamped to window bounds.
    Window,
    /// Visible only when the specified workspace is active.
    Workspace(usize),
    /// Visible only when the specified pane is visible, clamped to pane bounds.
    Pane(u32),
    /// Visible only when the specified tab is active, clamped to pane bounds.
    Tab(u32, usize),
    /// Visible only when the specified surface is visible, clamped to surface bounds.
    Surface(u32),
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
}

pub const TITLE_BAR_HEIGHT: f32 = 28.0;
pub const CONTENT_MARGIN: f32 = 4.0;

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

    fn popup_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, self.size)
    }

    fn title_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(self.pos, egui::vec2(self.size.x, TITLE_BAR_HEIGHT))
    }

    fn content_rect(&self) -> egui::Rect {
        let popup = self.popup_rect();
        let top_offset = if self.headless {
            CONTENT_MARGIN
        } else {
            TITLE_BAR_HEIGHT + CONTENT_MARGIN
        };
        egui::Rect::from_min_max(
            egui::pos2(popup.min.x + CONTENT_MARGIN, popup.min.y + top_offset),
            egui::pos2(popup.max.x - CONTENT_MARGIN, popup.max.y - CONTENT_MARGIN),
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
    pub fn register_def(&mut self, def: &PopupDef) {
        if self.popups.iter().any(|p| p.id == def.id) {
            return;
        }
        let popup = PopupState::new(def.id, crate::i18n::t(def.title_key), def.default_size)
            .with_scope(def.default_scope.clone())
            .with_close_on_outside_click(def.close_on_outside_click)
            .with_headless(def.headless)
            .with_sticky_focus(def.sticky_focus);
        self.popups.push(popup);
    }

    /// `sizer`가 정의된 popup에 한해 open 직전 크기를 재계산한다. caller가 사이즈
    /// 갱신 후 `open*` 계열을 호출하는 규약.
    pub fn refresh_size(&mut self, id: PopupId, size: egui::Vec2) {
        if let Some(p) = self.popups.iter_mut().find(|p| p.id == id) {
            p.size = size;
        }
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
        }
    }

    /// Toggle a popup open/closed.
    pub fn toggle(&mut self, id: PopupId) {
        if self.is_open(id) {
            self.close(id);
        } else {
            self.open(id);
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
