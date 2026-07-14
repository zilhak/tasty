//! Test `WakerFactory` — waker dedup 게이트 누수 회귀 테스트용.
//!
//! `make_targeted_waker`/`forget_surface` 호출을 기록해, headless PTY 종료·승격 경로
//! (`pty.kill`/idle sweep/`AdoptTerminal`)가 pty_id 게이트를 실제로 정리하는지
//! (`forget_surface` 호출 여부) 관찰한다. production `WinitWakerFactory`/
//! `HeadlessWakerFactory` 는 내부 게이트 맵이 private 이라 직접 관찰이 안 되므로
//! 테스트는 이 recording mirror 를 주입한다.

use std::sync::{Arc, Mutex};

use tasty_terminal::Waker;
use tasty_terminal::waker_factory::WakerFactory;

/// `make_targeted_waker` 로 만들어진 게이트 id 와 `forget_surface` 로 정리된 id 를
/// 호출 순서대로 기록하는 test factory.
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
