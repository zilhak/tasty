//! Application-owned clipboard; retain the selection owner across RPC/OSC52 requests.

use crate::clipboard::{
    ClipboardContext,
    error::{ClipboardError, DisplayBackend},
};
use crate::ports::clipboard::{ClipboardImage, ClipboardSystem};
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

    /// arboard 의 setter 는 매번 이전 형식을 지워 두 형식을 함께 올리지 못한다. 파일 클립보드와 같은
    /// 플랫폼 소유자로 한 번에 올린다. 이 소유자가 선택을 가져가면 arboard 의 이전 텍스트는 사라진다.
    fn write_text_and_image(&self, text: &str, image: &ClipboardImage) -> anyhow::Result<()> {
        Ok(crate::clipboard::files::write_text_and_image(text, image)?)
    }
}
