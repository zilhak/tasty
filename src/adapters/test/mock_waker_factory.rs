//! PTY 생성·회수의 waker 등록/해제 호출을 기록한다.
//! 실제 factory의 비공개 상태 대신 이 구현을 주입해 정리 여부를 확인한다.

use std::sync::{Arc, Mutex};

use tasty_terminal::Waker;
use tasty_terminal::waker_factory::WakerFactory;

#[derive(Default)]
pub struct RecordingWakerFactory {
    made: Mutex<Vec<u32>>,
    forgotten: Mutex<Vec<u32>>,
}

impl RecordingWakerFactory {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// `make_targeted_waker` 로 게이트가 생성된 id 목록.
    pub fn made(&self) -> Vec<u32> {
        self.made.lock().expect("made poisoned").clone()
    }

    /// `forget_surface` 로 정리된 id 목록.
    pub fn forgotten(&self) -> Vec<u32> {
        self.forgotten.lock().expect("forgotten poisoned").clone()
    }
}

impl WakerFactory for RecordingWakerFactory {
    fn make_targeted_waker(&self, surface_id: u32) -> Waker {
        self.made.lock().expect("made poisoned").push(surface_id);
        Arc::new(|| {})
    }

    fn make_default_waker(&self) -> Waker {
        Arc::new(|| {})
    }

    fn note_drained(&self, _surface_id: Option<u32>) {}

    fn forget_surface(&self, surface_id: u32) {
        self.forgotten
            .lock()
            .expect("forgotten poisoned")
            .push(surface_id);
    }
}
