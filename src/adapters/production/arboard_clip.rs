//! ArboardClipboard — `arboard` crate 기반 production ClipboardSystem.

use crate::ports::clipboard::ClipboardSystem;

#[derive(Debug, Default)]
pub struct ArboardClipboard;

impl ClipboardSystem for ArboardClipboard {
    fn read_text(&self) -> anyhow::Result<String> {
        let mut cb = arboard::Clipboard::new()?;
        Ok(cb.get_text()?)
    }

    fn write_text(&self, text: &str) -> anyhow::Result<()> {
        let mut cb = arboard::Clipboard::new()?;
        cb.set_text(text)?;
        Ok(())
    }
}
