//! Popup 분류 enum / id — UI 본문이 아닌 *분류* 만 model 에 둔다.
//!
//! `PopupState` / `PopupManager` (UI 동작 본문) 는 [`crate::adapters::ui::popup`] 잔류.
//! headless 빌드에서도 intent 큐가 PopupScope 를 enqueue 할 수 있도록 GUI 의존 0 으로 유지.

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
