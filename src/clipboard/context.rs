//! 시스템 클립보드 wrapper + 백그라운드 polling 스레드가 감지한 데이터 enum.

/// Wrapper for the system clipboard (arboard).
pub(crate) struct ClipboardContext {
    inner: arboard::Clipboard,
}

impl ClipboardContext {
    pub(crate) fn new() -> Option<Self> {
        arboard::Clipboard::new().ok().map(|c| Self { inner: c })
    }

    pub(crate) fn get_text(&mut self) -> Option<String> {
        self.inner.get_text().ok()
    }

    pub(crate) fn get_image(&mut self) -> Option<arboard::ImageData<'static>> {
        self.inner.get_image().ok()
    }

    pub(crate) fn set_text(&mut self, text: &str) {
        if let Err(e) = self.inner.set_text(text.to_string()) {
            tracing::warn!("clipboard set_text failed: {e}");
        }
    }
}

/// Clipboard data detected by the background polling thread.
pub(crate) enum ClipboardData {
    Text(String),
    Image(crate::clipboard_history::ImageData),
}

impl std::fmt::Debug for ClipboardData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipboardData::Text(t) => write!(f, "Text({}B)", t.len()),
            ClipboardData::Image(img) => write!(f, "Image({}x{})", img.width, img.height),
        }
    }
}
