//! winit 이벤트 루프와 proxy를 만든다.

use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::AppEvent;

/// macOS 기본 메뉴를 끄고 Tasty가 직접 종료·새 창 동작을 등록한다.
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
