//! Application-owned clipboard; retain the selection owner across RPC/OSC52 requests.

use crate::clipboard::{
    ClipboardContext,
    error::{ClipboardError, DisplayBackend},
};
use crate::ports::clipboard::ClipboardSystem;
use std::sync::Mutex;

#[derive(Default)]
pub struct ArboardClipboard {
    clipboard: Mutex<Option<ClipboardContext>>,
}

impl ArboardClipboard {
    fn with_clipboard<T>(
        &self,
        operation: impl FnOnce(&mut ClipboardContext) -> Result<T, ClipboardError>,
    ) -> anyhow::Result<T> {
        let mut slot = self.clipboard.lock().map_err(|_| {
            ClipboardError::BackendUnavailable("clipboard owner lock poisoned".into())
        })?;
        let clipboard = slot.get_or_insert_with(|| ClipboardContext::new(DisplayBackend::Unknown));
        Ok(operation(clipboard)?)
    }
}

impl ClipboardSystem for ArboardClipboard {
    fn read_text(&self) -> anyhow::Result<String> {
        self.with_clipboard(|cb| cb.get_text())?
            .ok_or_else(|| anyhow::anyhow!("clipboard has no text"))
    }

    fn write_text(&self, text: &str) -> anyhow::Result<()> {
        self.with_clipboard(|cb| cb.set_text(text))
    }
}
