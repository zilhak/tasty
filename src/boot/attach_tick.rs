//! attach/detach 작업 J — 3초 간격 attach poll ticker.
//!
//! `busy_tick` 와 동형(거기는 1초). 메인 스레드가 `AppEvent::AttachPoll` 을 받아
//! ① 서버측 readonly 뷰의 display mirror 를 live grid 스냅샷으로 갱신하고
//! ② client mirror 의 누적 출력 버퍼를 적용해 화면을 repaint 한다.
//!
//! 원격 워크스페이스/surface 의 readonly·mirror 뷰는 **실시간 stream 이 아니라
//! 3초 polling** 으로 갱신한다(plan §4, 사용자 확정 UX). 전송(tap forwarder)은
//! 그대로 두고 *렌더 갱신 cadence* 만 이 ticker 가 3초로 게이트한다.

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

/// attach poll 주기. 사용자 확정 UX = 3초/회.
pub(crate) const ATTACH_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);

pub(crate) fn spawn(proxy: EventLoopProxy<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(ATTACH_POLL_INTERVAL);
            if proxy.send_event(AppEvent::AttachPoll).is_err() {
                break;
            }
        }
    });
}
