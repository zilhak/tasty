//! MockClipboard — in-memory ClipboardSystem.

use std::sync::Mutex;

use crate::ports::clipboard::ClipboardSystem;

#[derive(Debug, Default)]
pub struct MockClipboard {
    text: Mutex<Option<String>>,
}

impl MockClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClipboardSystem for MockClipboard {
    fn read_text(&self) -> anyhow::Result<String> {
        let t = self.text.lock().expect("MockClipboard poisoned");
        t.clone().ok_or_else(|| anyhow::anyhow!("clipboard empty"))
    }

    fn write_text(&self, text: &str) -> anyhow::Result<()> {
        let mut t = self.text.lock().expect("MockClipboard poisoned");
        *t = Some(text.to_string());
        Ok(())
    }
}
