//! ClipboardViewer surface — 탭 내부에 고정 뷰어로 배치. 검색어/선택/clear 확인
//! 같은 휘발성 GUI 상태는 host의 `ClipboardViewerViewStore`에 둔다.

use super::SurfaceId;
use super::surface_trait::Surface;

pub struct ClipboardViewerPanel {
    pub id: SurfaceId,
}

impl ClipboardViewerPanel {
    pub fn new(id: SurfaceId) -> Self {
        Self { id }
    }
}

impl Surface for ClipboardViewerPanel {
    fn kind(&self) -> &'static str {
        "clipboard_viewer"
    }
    fn type_name(&self) -> &'static str {
        "ClipboardViewer"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    fn display_name(&self) -> String {
        crate::i18n::t("clipboard_viewer.tab_title").to_string()
    }

    fn as_clipboard_viewer(&self) -> Option<&ClipboardViewerPanel> {
        Some(self)
    }

    fn as_clipboard_viewer_mut(&mut self) -> Option<&mut ClipboardViewerPanel> {
        Some(self)
    }
}
