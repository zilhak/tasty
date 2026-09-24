//! engine이 winit에 의존하지 않도록 호스트에서 WakerFactory를 구현한다.

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
    /// 전체 engine drain 깨움은 factory마다 하나로 합친다.
    default_gate: Arc<AtomicBool>,
    /// 특정 surface만 처리하는 깨움은 서로 합치지 않도록 ID별로 관리한다.
    targeted_gates: Mutex<HashMap<u32, Arc<AtomicBool>>>,
    /// poison 경고를 한 번만 남긴다.
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
        // 닫힌 surface의 표지가 계속 쌓이지 않도록 제거한다.
        crate::waker::recover_gate_lock(
            self.targeted_gates.lock(),
            "WinitWakerFactory targeted_gates",
            &self.poison_reported,
        )
        .remove(&surface_id);
    }
}
