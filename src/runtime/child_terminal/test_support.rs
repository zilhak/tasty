//! Legacy synchronous assertions wait on the real adapter; runtime never uses these barriers.
use super::*;
use crate::app::file_writer;
use std::time::{Duration, Instant};
use tasty_utils::batch_writer::{Limits, SystemClock};

pub(super) fn wait(receipt: Ticket) -> Result<Option<u32>, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = receipt.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "child file receipt missing");
        std::thread::yield_now();
    }
}
impl Hub {
    pub(crate) fn test_attach_writer(&self) {
        let parts = file_writer::start(
            Arc::new(tasty_settings::FileSettingsStorage),
            Limits::default(),
            Arc::new(SystemClock),
            Arc::new(|| {}),
        )
        .unwrap();
        self.attach(parts.writer.handle());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.ready() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        *self.fixture_writer.lock().unwrap() = Some(parts);
    }
}
impl ChildTerminalRegistry {
    #[cfg(all(unix, feature = "gui"))]
    pub(crate) fn flush_for_test(&mut self) {
        wait(self.submit().expect("real child file admission"))
            .expect("real child file replacement");
    }
}
