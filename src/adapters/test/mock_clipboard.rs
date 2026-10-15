//! MockClipboard — in-memory ClipboardSystem.

use std::sync::Mutex;

use crate::ports::clipboard::{ClipboardImage, ClipboardSystem};

#[derive(Debug, Default)]
pub struct MockClipboard {
    text: Mutex<Option<String>>,
    image: Mutex<Option<ClipboardImage>>,
}

impl MockClipboard {
    /// 마지막 게시가 함께 올린 이미지. 텍스트만 쓰면 지워진다(OS 클립보드와 같다).
    #[cfg(test)]
    pub fn image(&self) -> Option<ClipboardImage> {
        self.image.lock().expect("MockClipboard poisoned").clone()
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
        *self.image.lock().expect("MockClipboard poisoned") = None;
        Ok(())
    }

    fn write_text_and_image(&self, text: &str, image: &ClipboardImage) -> anyhow::Result<()> {
        self.write_text(text)?;
        *self.image.lock().expect("MockClipboard poisoned") = Some(image.clone());
        Ok(())
    }
}
