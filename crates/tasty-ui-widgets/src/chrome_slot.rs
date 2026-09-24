//! 본체와 갤러리가 창 오른쪽 위 컨트롤의 위치 계산을 공유한다.

/// 영역 오른쪽 위에서 가로·세로로 pad만큼 띄운 정사각형을 반환한다.
pub fn top_right_inset_square(area: egui::Rect, pad: f32, side: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(area.right() - pad - side, area.top() + pad),
        egui::Vec2::splat(side),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 서로 다른 원점·여백·크기를 사용해 좌표를 잘못 섞은 경우를 검출한다.
    #[test]
    fn it_sits_pad_away_from_the_top_right_corner() {
        let area = egui::Rect::from_min_max(egui::pos2(10.0, 30.0), egui::pos2(210.0, 130.0));
        let slot = top_right_inset_square(area, 8.0, 20.0);
        assert_eq!(slot.min, egui::pos2(182.0, 38.0));
        assert_eq!(slot.max, egui::pos2(202.0, 58.0));
    }

    #[test]
    fn it_is_square() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(300.0, 200.0));
        let slot = top_right_inset_square(area, 12.0, 28.0);
        assert_eq!(slot.width(), 28.0);
        assert_eq!(slot.height(), 28.0);
    }

    /// 왼쪽·아래 경계를 바꿔도 오른쪽 위 기준 배치는 같아야 한다.
    #[test]
    fn only_the_top_right_corner_decides_it() {
        let a = egui::Rect::from_min_max(egui::pos2(0.0, 30.0), egui::pos2(210.0, 130.0));
        let b = egui::Rect::from_min_max(egui::pos2(90.0, 30.0), egui::pos2(210.0, 999.0));
        assert_eq!(
            top_right_inset_square(a, 8.0, 20.0),
            top_right_inset_square(b, 8.0, 20.0)
        );
    }
}
