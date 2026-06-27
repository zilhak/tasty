//! 시스템 클립보드 wrapper — 터미널 선택 복사 / vi-copy / OSC52 / 붙여넣기에 사용.

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

    /// Linux primary selection (vim `*` register). Other OSes fall back to
    /// the regular system clipboard since they have no primary-selection
    /// concept.
    pub(crate) fn set_text_primary(&mut self, text: &str) {
        #[cfg(target_os = "linux")]
        {
            use arboard::{LinuxClipboardKind, SetExtLinux};
            if let Err(e) = self
                .inner
                .set()
                .clipboard(LinuxClipboardKind::Primary)
                .text(text.to_string())
            {
                tracing::warn!("clipboard set_text_primary failed: {e}");
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.set_text(text);
        }
    }
}
