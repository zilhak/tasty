//! 보기 모드 토글의 바깥 틀 색을 그린 도형으로 확인한다.

use super::*;
use crate::painted_test_support::{paint, themes};

/// 보기 토글의 바깥 틀은 raised 채움이라 외곽이 border-strong이다.
#[test]
fn the_view_toggle_frame_edges_with_border_strong() {
    for theme in themes() {
        let strong = theme.border_strong().to_egui();
        assert_ne!(strong, theme.border_default().to_egui());
        let p = paint(|ui| {
            let mut action = None;
            seg_toggle(ui, &theme, ExplorerViewMode::Grid, &mut action);
        });
        let raised = theme.surface_raised().to_egui();
        let frames: Vec<_> = p
            .stroked_rects()
            .into_iter()
            .filter(|(r, fill, _)| *fill == raised && r.width() == seg_toggle_width(&theme))
            .collect();
        assert_eq!(frames.len(), 1, "바깥 틀 하나: {frames:?}");
        assert_eq!(frames[0].2, strong);
    }
}
