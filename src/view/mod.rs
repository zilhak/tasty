//! winit 창, egui 입력과 렌더링, 모달·포커스를 관리하는 GUI 어댑터.
//! View trait은 src/view/ui.rs에 있다.

pub(crate) mod base;
pub(crate) mod state;
pub(crate) mod main;
pub(crate) mod modal;
pub(crate) mod plugins;
pub(crate) mod preset;
pub(crate) mod quit;
pub(crate) mod repaint;
pub(crate) mod settings;
pub(crate) mod ui;

pub(crate) use base::ViewBase;
pub(crate) use main::MainView;
pub(crate) use modal::ModalView;
pub(crate) use plugins::PluginsView;
pub(crate) use preset::PresetView;
pub(crate) use quit::QuitView;
pub(crate) use repaint::RepaintSource;
pub(crate) use settings::SettingsView;

use std::collections::HashMap;

use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

use crate::AppEvent;

/// `Box<dyn View>`에서 `MainView` 소유권을 추출한다.
/// 인자가 MainView가 아니면 `None` — 호출자가 인지 후 다르게 처리.
pub(crate) fn unbox_main(w: Box<dyn ui::View>) -> Option<Box<MainView>> {
    if !w.as_any().is::<MainView>() {
        return None;
    }
    let any: Box<dyn std::any::Any> = w;
    any.downcast::<MainView>().ok()
}

/// 이벤트 처리 결과로 View 가 요청하는 동작.
#[must_use]
pub(crate) enum ViewAction {
    /// 아무 일도 하지 않음.
    None,
    /// 이 View 를 닫음.
    Close,
    /// 이 View 를 닫고 AppEvent를 발행함.
    CloseWithEvent(AppEvent),
}

/// 이벤트 핸들러에 함께 전달되는 맥락.
pub(crate) struct ViewCtx<'a> {
    pub(crate) event_loop: &'a ActiveEventLoop,
    /// 현재 모달 View 가 활성 상태인지. true면 비모달 View 는 입력을 차단해야 한다.
    pub(crate) modal_active: bool,
    /// 현재 active plugin manager. MainView 가 frame prepare 시 plugin canvas의
    /// SharedMemory와 dirty rect에 접근하기 위해 사용한다. plugin 비활성 빌드/초기 시점에는 None.
    pub(crate) plugin_manager: Option<crate::app::plugin_display::PluginDisplay<'a>>,
    /// 이 창에 연결된 engine. App의 engine registry가 창 ID로 찾아 넘긴다. 모달 View는 None이다.
    pub(crate) engine: Option<crate::runtime::engine_read::EngineRead<'a>>,
}

/// 열린 모달의 종류. 창을 열 때 기록해 debug 조회에서 모달을 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModalKind {
    Settings,
    Plugins,
    Quit,
}

impl ModalKind {
    /// debug IPC 응답에 쓰는 이름. 내부 판정은 enum을 사용한다.
    #[cfg(debug_assertions)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Settings => "settings",
            Self::Plugins => "plugins",
            Self::Quit => "quit",
        }
    }
}

/// 활성 모달의 창 ID와 종류. 둘은 항상 함께 바뀐다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveModal {
    pub(crate) id: WindowId,
    pub(crate) kind: ModalKind,
}

pub(crate) struct ViewRegistry {
    /// winit event loop 의 proxy. AppEvent 를 enqueue 하기 위한 채널.
    /// View 영역은 GUI 어댑터로서 winit 과 직접 결합되어 있다.
    pub proxy: EventLoopProxy<AppEvent>,
    /// 활성 모달의 유일한 원본. Some이면 다른 View는 입력을 무시한다. 모달은 최대 1개다.
    /// 쓰기는 `set_active_modal`/`take_active_modal`만 사용하도록 필드를 비공개로 둔다.
    active_modal: Option<ActiveModal>,
    /// The view that currently has focus (receives IPC commands targeting "focused" view).
    pub focused_view_id: Option<WindowId>,
    /// 모든 View(모달 포함). `active_modal`로 현재 활성 모달을 식별한다.
    /// 모달도 여기에 들어가며, 모달은 엔진 전역에 최대 1개라는 불변식을 유지한다.
    /// key 는 winit `WindowId`.
    pub views: HashMap<WindowId, Box<dyn ui::View>>,
}

impl ViewRegistry {
    pub(crate) fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            proxy,
            active_modal: None,
            focused_view_id: None,
            views: HashMap::new(),
        }
    }

    /// Check if a modal is active.
    pub fn is_modal_active(&self) -> bool {
        self.active_modal.is_some()
    }

    /// debug ui.state 투영만 종류까지 읽는다. 다른 호출부는 `active_modal_id`를 사용한다.
    #[cfg(debug_assertions)]
    pub(crate) fn active_modal(&self) -> Option<ActiveModal> {
        self.active_modal
    }

    pub(crate) fn active_modal_id(&self) -> Option<WindowId> {
        self.active_modal.map(|m| m.id)
    }

    pub(crate) fn set_active_modal(&mut self, id: WindowId, kind: ModalKind) {
        self.active_modal = Some(ActiveModal { id, kind });
    }

    pub(crate) fn take_active_modal(&mut self) -> Option<ActiveModal> {
        self.active_modal.take()
    }
}
