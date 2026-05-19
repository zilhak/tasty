//! 1초 간격 busy ticker spawn.
//!
//! 메인 스레드가 받아서 모든 surface 의 foreground 프로세스를 다시 조회하고
//! 캐시를 갱신한다. PID 조회 자체는 가볍지만 매 프레임 호출하면 과하므로
//! 별도 스레드에서 ticking 만 한다.

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

pub(crate) fn spawn(proxy: EventLoopProxy<AppEvent>) {
    std::thread::spawn(move || {
        let interval = std::time::Duration::from_secs(1);
        loop {
            std::thread::sleep(interval);
            if proxy.send_event(AppEvent::BusyPoll).is_err() {
                break;
            }
        }
    });
}
