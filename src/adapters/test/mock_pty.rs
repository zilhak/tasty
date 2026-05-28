//! MockPtyService — PTY spawn 을 *기록만* 함. 반환되는 `TerminalProcess` 는
//! `tasty_terminal::testing::MockTerminal`.

use std::sync::{Arc, Mutex};

use tasty_terminal::testing::MockTerminal;
use tasty_terminal::{TerminalConfig, TerminalProcess};

use crate::ports::pty::{PtyService, TerminalWaker};

#[derive(Debug, Clone)]
pub struct SpawnRecord {
    pub surface_id: u32,
    pub cols: usize,
    pub rows: usize,
    pub shell: Option<String>,
}

#[derive(Debug, Default)]
pub struct MockPtyService {
    pub spawns: Mutex<Vec<SpawnRecord>>,
}

impl MockPtyService {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PtyService for MockPtyService {
    fn spawn(
        &self,
        config: TerminalConfig<'_>,
        _waker: Arc<dyn TerminalWaker>,
    ) -> anyhow::Result<Box<dyn TerminalProcess>> {
        self.spawns
            .lock()
            .expect("MockPtyService poisoned")
            .push(SpawnRecord {
                surface_id: config.surface_id,
                cols: config.cols,
                rows: config.rows,
                shell: config.shell.map(|s| s.to_string()),
            });
        Ok(Box::new(MockTerminal::new(config.cols, config.rows)))
    }
}
