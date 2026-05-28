//! MockClipboard — in-memory ClipboardSystem.

use std::sync::Mutex;

use crate::ports::clipboard::{ClipboardImage, ClipboardSystem};

#[derive(Debug, Default)]
pub struct MockClipboard {
    text: Mutex<Option<String>>,
    image: Mutex<Option<ClipboardImage>>,
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

    fn read_image(&self) -> anyhow::Result<ClipboardImage> {
        let i = self.image.lock().expect("MockClipboard poisoned");
        i.clone().ok_or_else(|| anyhow::anyhow!("no image"))
    }

    fn write_image(&self, image: &ClipboardImage) -> anyhow::Result<()> {
        let mut i = self.image.lock().expect("MockClipboard poisoned");
        *i = Some(image.clone());
        Ok(())
    }
}
