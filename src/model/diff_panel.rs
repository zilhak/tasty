//! Diff surface — 변경 제안 검토 UI.
//!
//! `before` / `after` 두 텍스트 블록을 좌/우로 표시한다. 호스트 host view 가 실제
//! 렌더링과 라인 매칭/색상 처리를 담당한다.
//!
//! `apply_action` 은 사용자가 Apply 버튼을 클릭했을 때 새 터미널에서 spawn 될 명령
//! 라인. metadata 로만 보관하고 도메인 layer 는 실행에 관여하지 않는다.

use super::SurfaceId;
use super::surface_trait::Surface;

pub struct DiffPanel {
    pub id: u32,
    pub title: String,
    pub before: String,
    pub after: String,
    pub apply_action: Option<String>,
}

impl DiffPanel {
    pub fn new(id: u32, title: String, before: String, after: String) -> Self {
        Self {
            id,
            title,
            before,
            after,
            apply_action: None,
        }
    }

    pub fn with_apply_action(mut self, action: Option<String>) -> Self {
        self.apply_action = action;
        self
    }
}

impl Surface for DiffPanel {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "diff"
    }
    fn type_name(&self) -> &'static str {
        "Diff"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }
    fn display_name(&self) -> String {
        if self.title.is_empty() {
            "Diff".to_string()
        } else {
            self.title.clone()
        }
    }
}
