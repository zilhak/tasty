//! winit EventLoop + proxy 빌드.

use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::AppEvent;

/// `EventLoop<AppEvent>` + proxy 생성. winit 빌드 실패는 그대로 상위 전파.
pub(crate) fn build() -> anyhow::Result<(EventLoop<AppEvent>, EventLoopProxy<AppEvent>)> {
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    Ok((event_loop, proxy))
}
