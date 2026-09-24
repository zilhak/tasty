//! GUI에 의존하지 않는 팝업 ID와 표시 범위. 실제 표시와 관리는 호스트 UI가 맡는다.

/// Popup 인스턴스의 고유 식별자. 정의 시점에 고정되는 static 문자열.
pub type PopupId = &'static str;

/// Popup 의 visibility scope. 어떤 컨텍스트에서 보이고 어디로 clamp 되는지 결정.
#[derive(Debug, Clone, PartialEq)]
pub enum PopupScope {
    /// 윈도우 전체에 클램프.
    Window,
    /// 지정된 워크스페이스가 활성일 때만 표시.
    Workspace(usize),
    /// 지정된 pane 이 보일 때만 표시 (pane 영역 클램프).
    Pane(u32),
    /// 지정된 tab 이 활성일 때만 표시 (pane 영역 클램프).
    Tab(u32, usize),
    /// 지정된 surface 가 보일 때만 표시 (surface 영역 클램프).
    Surface(u32),
}
