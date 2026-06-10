//! Tab content padding frame — 탭 내부 콘텐츠를 모달 테두리에서 일정
//! 거리 (`tokens::TAB_CONTENT_PADDING`) 만큼 띄우는 단순 wrapper.
//!
//! 본체 settings modal 의 ScrollArea 내부와 갤러리 `layout_2depth::draw_content`
//! 가 공통으로 사용.

use crate::tokens;

/// 4 면 동일 inner_margin (`TAB_CONTENT_PADDING`) 을 가진 빈 `Frame` 안에서
/// `content` 클로저를 실행한다.
pub fn tab_content_frame(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(tokens::TAB_CONTENT_PADDING))
        .show(ui, |ui| content(ui));
}
