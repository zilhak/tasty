//! ClipboardViewer surface — 탭 내부에 고정 뷰어로 배치. 각 인스턴스가 자체
//! 검색어/선택 상태를 가진다 (popup의 `DialogState.clipboard_viewer`와 독립).

use super::SurfaceId;
use super::surface_trait::Surface;

/// Popup/Surface 양쪽에서 유지하는 뷰어 상태.
#[derive(Debug, Default, Clone)]
pub struct ClipboardViewerState {
    /// 검색어. 빈 문자열이면 전체 표시.
    pub search: String,
    /// 키보드 선택 인덱스 (필터된 결과 기준).
    pub selected: Option<usize>,
    /// 전체 비우기 확인 대기 플래그.
    pub pending_clear: bool,
}

pub struct ClipboardViewerPanel {
    pub id: SurfaceId,
    pub state: ClipboardViewerState,
}

impl ClipboardViewerPanel {
    pub fn new(id: SurfaceId) -> Self {
        Self {
            id,
            state: ClipboardViewerState::default(),
        }
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
