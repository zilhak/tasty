//! `View` — GUI 어댑터. winit 윈도우, egui, modal/focus 식별자, event loop proxy,
//! windows HashMap 등 *사용자의 화면* 측면을 모은다.
//!
//! D.3.E.3.d — 옛 `src/adapters/ui/window/` 의 9 파일 + 3 서브디렉터리가
//! `src/view/` 로 평탄화 이동. trait 본체는 `src/view/ui.rs`, 구현체 모듈은
//! 본 모듈 안 형제.

pub(crate) mod base;
pub(crate) mod editor;
pub(crate) mod main;
pub(crate) mod modal;
pub(crate) mod plugins;
pub(crate) mod preset;
pub(crate) mod quit;
pub(crate) mod settings;
pub(crate) mod terminal_host;
pub(crate) mod ui;

pub(crate) use base::ViewBase;
pub(crate) use main::MainWindow;
pub(crate) use modal::ModalView;
pub(crate) use plugins::PluginsWindow;
pub(crate) use preset::PresetWindow;
pub(crate) use quit::QuitWindow;
pub(crate) use settings::SettingsWindow;
pub(crate) use terminal_host::TerminalHostView;

use std::collections::HashMap;

use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::AppEvent;

/// `Box<dyn View>`에서 `MainWindow` 소유권을 추출한다.
/// 인자가 MainWindow가 아니면 `None` — 호출자가 인지 후 다르게 처리.
pub(crate) fn unbox_main(w: Box<dyn ui::View>) -> Option<Box<MainWindow>> {
    if !w.as_any().is::<MainWindow>() {
        return None;
    }
    let any: Box<dyn std::any::Any> = w;
    any.downcast::<MainWindow>().ok()
}

/// 윈도우의 모달리티.
///
/// 도메인 용어(`docs/design/ubiquitous-language.md`): View는 modality
/// (Modeless/Modal)와 계열(ModalView/TerminalHostView/EditorView)을 속성으로 갖는다.
/// 현재 trait dispatch 경로에서 호출 0이지만 5개 구현체가 `fn modality()`로
/// 반환하는 도메인 표현이라 보존한다. modal 활성 판정 dispatch가 도입되면 활성화.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Modality {
    /// 일반 윈도우. 다른 윈도우와 독립적으로 포커스됨.
    Modeless,
    /// 모달 윈도우. 활성 상태에선 다른 모든 윈도우의 입력이 차단됨.
    /// 엔진 전역에서 최대 1개만 존재한다.
    Modal,
}

/// 이벤트 처리 결과로 윈도우가 요청하는 동작.
#[must_use]
pub(crate) enum ViewAction {
    /// 아무 일도 하지 않음.
    None,
    /// 이 윈도우를 닫음.
    Close,
    /// 이 윈도우를 닫고 AppEvent를 발행함.
    CloseWithEvent(AppEvent),
}

/// 이벤트 핸들러에 함께 전달되는 맥락.
pub(crate) struct ViewCtx<'a> {
    pub(crate) event_loop: &'a ActiveEventLoop,
    /// 현재 모달 윈도우가 활성 상태인지. true면 비모달 윈도우는 입력을 차단해야 한다.
    pub(crate) modal_active: bool,
    /// 현재 active plugin manager. 메인 윈도우가 frame prepare 시 plugin canvas의
    /// SharedMemory와 dirty rect에 접근하기 위해 사용한다. plugin 비활성 빌드/초기 시점에는 None.
    pub(crate) plugin_manager: Option<&'a crate::plugin::PluginManager>,
}

pub(crate) struct ViewRegistry {
    /// winit event loop 의 proxy. AppEvent 를 enqueue 하기 위한 채널.
    /// View 영역은 GUI 어댑터로서 winit 과 직접 결합되어 있다.
    pub proxy: EventLoopProxy<AppEvent>,
    /// When Some, a modal view is active and all other views should ignore input.
    /// At most one modal can exist at a time.
    pub active_modal_id: Option<WindowId>,
    /// The view that currently has focus (receives IPC commands targeting "focused" view).
    pub focused_window_id: Option<WindowId>,
    /// 모든 View(모달 포함). `active_modal_id`로 현재 활성 모달을 식별한다.
    /// 모달도 여기에 들어가며, 모달은 엔진 전역에 최대 1개라는 불변식을 유지한다.
    /// D.3.E.3.a — 옛 `App.windows` 가 이쪽으로 이동. key 는 winit `WindowId`.
    pub windows: HashMap<WindowId, Box<dyn ui::View>>,
}

impl ViewRegistry {
    pub(crate) fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            proxy,
            active_modal_id: None,
            focused_window_id: None,
            windows: HashMap::new(),
        }
    }

    /// Check if a modal is active.
    pub fn is_modal_active(&self) -> bool {
        self.active_modal_id.is_some()
    }
}
