//! `WakerFactory` 의 winit 구현.
//!
//! `main.rs`에서 EventLoop 준비 후 생성하여 `CoreState`에 주입한다.
//! `tasty-core`는 winit 의존이 0건이므로 어댑터는 본체에만 둔다.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::waker::WakerFactory;
use tasty_terminal::Waker;
use winit::event_loop::EventLoopProxy;

use crate::AppEvent;

pub struct WinitWakerFactory {
    proxy: EventLoopProxy<AppEvent>,
    /// `make_default_waker` (`TerminalOutput(None)`) 의 dedup 게이트. None 은
    /// 핸들러가 전체 engine 을 drain 하므로 합쳐도 무손실 — factory 당 1 개.
    default_gate: Arc<AtomicBool>,
    /// `make_targeted_waker` (`TerminalOutput(Some(sid))`) 의 surface 별 dedup
    /// 게이트. Some 은 해당 surface 만 drain 하므로 surface 마다 독립 게이트 필요.
    targeted_gates: Mutex<HashMap<u32, Arc<AtomicBool>>>,
    /// `targeted_gates` poison 을 이미 보고했는가 — 로그 폭주 방지용 1 회 게이트.
    poison_reported: AtomicBool,
}

impl WinitWakerFactory {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            proxy,
            default_gate: Arc::new(AtomicBool::new(false)),
            targeted_gates: Mutex::new(HashMap::new()),
            poison_reported: AtomicBool::new(false),
        }
    }
}

impl WakerFactory for WinitWakerFactory {
    fn make_targeted_waker(&self, surface_id: u32) -> Waker {
        let proxy = self.proxy.clone();
        let gate = crate::waker::recover_gate_lock(
            self.targeted_gates.lock(),
            "WinitWakerFactory targeted_gates",
            &self.poison_reported,
        )
        .entry(surface_id)
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone();
        Arc::new(move || {
            // 직전 wake 가 아직 소화되지 않았으면(게이트 true) 큐잉을 스킵해 폭주 방지.
            if gate.swap(true, Ordering::AcqRel) {
                return;
            }
            crate::shortcuts::send_app_event(&proxy, AppEvent::TerminalOutput(Some(surface_id)));
        })
    }

    fn make_default_waker(&self) -> Waker {
        let proxy = self.proxy.clone();
        let gate = self.default_gate.clone();
        Arc::new(move || {
            if gate.swap(true, Ordering::AcqRel) {
                return;
            }
            crate::shortcuts::send_app_event(&proxy, AppEvent::TerminalOutput(None));
        })
    }

    fn note_drained(&self, surface_id: Option<u32>) {
        match surface_id {
            Some(sid) => {
                if let Some(gate) = crate::waker::recover_gate_lock(
                    self.targeted_gates.lock(),
                    "WinitWakerFactory targeted_gates",
                    &self.poison_reported,
                )
                .get(&sid)
                {
                    gate.store(false, Ordering::Release);
                }
            }
            None => self.default_gate.store(false, Ordering::Release),
        }
    }

    fn forget_surface(&self, surface_id: u32) {
        // surface 닫힘 — 게이트 제거(미제거 시 surface 마다 영구 누적).
        crate::waker::recover_gate_lock(
            self.targeted_gates.lock(),
            "WinitWakerFactory targeted_gates",
            &self.poison_reported,
        )
        .remove(&surface_id);
    }
}
