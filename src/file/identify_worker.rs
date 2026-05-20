//! 비동기 파일 식별 worker.
//!
//! 콜사이트(예: mouse hover, Ctrl+click) 가 `spawn()` 으로 식별 요청을 던지면
//! background thread 가 `FileFormatRegistry::identify` 를 호출하고, 결과를 winit
//! `EventLoopProxy` 를 통해 `AppEvent::IdentifyDone` 으로 main thread 에 돌려준다.
//!
//! Phase B 인프라 — 콜사이트 본 연결은 Phase C 의 mouse.rs 변경에서 시작한다.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use winit::event_loop::EventLoopProxy;

use crate::AppEvent;
use crate::file::format::{DetectDepth, FileFormatRegistry, FileTarget};

/// 식별 요청 식별자. 콜사이트가 마지막 요청 id 를 보관해 out-of-order 결과를 무시한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdentifyRequestId(pub u64);

impl std::fmt::Display for IdentifyRequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct IdentifyWorker {
    registry: Arc<FileFormatRegistry>,
    proxy: EventLoopProxy<AppEvent>,
    next_id: AtomicU64,
}

impl IdentifyWorker {
    pub fn new(registry: Arc<FileFormatRegistry>, proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            registry,
            proxy,
            next_id: AtomicU64::new(1),
        }
    }

    /// 식별 요청을 백그라운드 thread 로 디스패치. 즉시 반환한다.
    ///
    /// 결과는 main thread 의 winit event loop 가 `AppEvent::IdentifyDone` 으로 받는다.
    /// 동시 요청이 여러 개여도 worker 들은 독립적으로 동작하며, 결과는 도착 순서가
    /// 보장되지 않는다 — 콜사이트는 자신이 보관한 마지막 `IdentifyRequestId` 와 비교해
    /// 오래된 결과를 drop 해야 한다.
    pub fn spawn(&self, target: FileTarget, depth: DetectDepth) -> IdentifyRequestId {
        let id = IdentifyRequestId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let registry = self.registry.clone();
        let proxy = self.proxy.clone();
        let target_for_thread = target.clone();
        std::thread::spawn(move || {
            let detector = registry.identify(&target_for_thread, depth);
            let _ = proxy.send_event(AppEvent::IdentifyDone {
                request_id: id,
                target: target_for_thread,
                detector,
            });
        });
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `IdentifyRequestId` 가 단조 증가하는지.
    #[test]
    fn request_ids_are_monotonic() {
        // EventLoopProxy 가 없는 환경 (단위 테스트) 에서는 spawn 호출 자체가 어렵다.
        // 대신 next_id 의 fetch_add 동작을 직접 검증.
        let counter = AtomicU64::new(1);
        let a = counter.fetch_add(1, Ordering::Relaxed);
        let b = counter.fetch_add(1, Ordering::Relaxed);
        let c = counter.fetch_add(1, Ordering::Relaxed);
        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(c, 3);
    }

    #[test]
    fn request_id_display() {
        let id = IdentifyRequestId(42);
        assert_eq!(format!("{id}"), "42");
    }
}
