//! MockTerminalWaker — wake 호출 *기록만*.

use std::sync::Mutex;

use crate::ports::pty::TerminalWaker;

#[derive(Debug, Default)]
pub struct MockTerminalWaker {
    pub wakes: Mutex<Vec<Option<u32>>>,
}

impl MockTerminalWaker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn drain(&self) -> Vec<Option<u32>> {
        std::mem::take(&mut *self.wakes.lock().expect("MockTerminalWaker poisoned"))
    }
}

impl TerminalWaker for MockTerminalWaker {
    fn wake(&self, surface_id: Option<u32>) {
        self.wakes
            .lock()
            .expect("MockTerminalWaker poisoned")
            .push(surface_id);
    }
}
