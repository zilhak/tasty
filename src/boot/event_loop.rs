//! winit EventLoop + proxy 빌드.

use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::AppEvent;

/// `EventLoop<AppEvent>` + proxy 생성. winit 빌드 실패는 그대로 상위 전파.
///
/// macOS 에서는 winit 의 자동 메뉴(⌘Q→terminate: 포함)를 끄고 tasty 가 직접 menubar 를
/// 등록한다 (`src/platform/macos_delegate.rs::setup_main_menu`).
pub(crate) fn build() -> anyhow::Result<(EventLoop<AppEvent>, EventLoopProxy<AppEvent>)> {
    let mut builder = EventLoop::<AppEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        builder.with_default_menu(false);
    }
    let event_loop = builder.build()?;
    let proxy = event_loop.create_proxy();
    Ok((event_loop, proxy))
}
