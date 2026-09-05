//! 창/무대 chrome 이 앉는 **자리** — 위젯이 아니라 그 위젯을 놓는 위치다.
//!
//! 전체화면 무대에는 CSD 타이틀바가 없어서 우상단의 종료 버튼이 유일한 마우스 탈출
//! 수단이다. 그 버튼을 본체와 갤러리가 각각 그리는데, **버튼은 이미 공유된 위젯이고
//! 여백·크기도 같은 토큰에서 오는데 위치 공식만 두 벌**이었다:
//!
//! ```text
//! Rect::from_min_size(pos2(area.right() - pad - side, area.top() + pad), 한 변 side)
//! ```
//!
//! 값이 토큰에서 오니 값 비교로는 안 갈리고, 매핑이 아니니 규칙 비교로도 안 갈린다.
//! **공식이 갈리면 화면 말고는 아무 신호가 없다.** 그래서 공식을 한 벌만 둔다 — 가드로
//! 두 벌을 감시하는 것보다 한 벌로 만드는 쪽이 싸고 확실하다.
//!
//! 집이 이 크레이트인 것은 게이트 비용을 피한 선택이 아니라 이것이 **위젯 배치
//! 관용구**이기 때문이다. `ControlSize`·`IconButton` 이 여기 살고, `tokens` 가 이미 같은
//! 성격의 레이아웃 값을 든다.

/// 영역 우상단에 한 변 `side` 인 정사각형을 `pad` 만큼 안쪽으로 앉힌다.
///
/// `pad` 는 위와 오른쪽 양쪽에 같은 값으로 들어간다 — 우상단 chrome 은 모서리에서
/// 대각으로 같은 거리에 놓이는 것이 이 관용구의 정의다.
pub fn top_right_inset_square(area: egui::Rect, pad: f32, side: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(area.right() - pad - side, area.top() + pad),
        egui::Vec2::splat(side),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 네 수를 전부 다르게 잡는다. `pad` 나 `side` 가 0 이거나 영역이 원점에서
    /// 시작하면 좌표가 겹쳐 **어떤 어긋남도 안 보인다** — 그때의 초록은 미측정이다.
    #[test]
    fn it_sits_pad_away_from_the_top_right_corner() {
        let area = egui::Rect::from_min_max(egui::pos2(10.0, 30.0), egui::pos2(210.0, 130.0));
        let slot = top_right_inset_square(area, 8.0, 20.0);
        assert_eq!(slot.min, egui::pos2(182.0, 38.0));
        assert_eq!(slot.max, egui::pos2(202.0, 58.0));
    }

    /// 정사각형인 것이 이름의 절반이다 — 한 변이 어긋나면 아이콘이 찌그러진다.
    #[test]
    fn it_is_square() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(300.0, 200.0));
        let slot = top_right_inset_square(area, 12.0, 28.0);
        assert_eq!(slot.width(), 28.0);
        assert_eq!(slot.height(), 28.0);
    }

    /// 왼쪽 가장자리·아래 가장자리는 자리를 안 정한다 — 영역이 어디서 시작하든
    /// 우상단으로부터의 거리만으로 결정된다.
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
