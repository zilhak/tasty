//! 사이드바 하단 Favorites 고정 영역의 높이(design `favPinHeight`)와 낮은 본문에서의 숨김
//! (design Short cell).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// 기본 고정 높이.
const FAV_PIN_BASE_H: LogicalPx = LogicalPx(240.0);
/// 사이드바 본문 높이가 이 값 미만이면 고정 높이 대신 비율(`FAV_PIN_RATIO`)을 쓴다.
const FAV_PIN_THRESHOLD_H: LogicalPx = LogicalPx(600.0);
/// 좁은 사이드바에서 Favorites 가 차지하는 본문 높이 비율.
const FAV_PIN_RATIO: f32 = 0.4;
/// Favorites 고정 영역 최소 높이 하한.
const FAV_PIN_MIN_H: LogicalPx = LogicalPx(120.0);

/// 본문 높이 `body_h` 에서 Favorites 영역 높이. 본문이 `explorer-favorites-hide-below` 보다
/// 낮으면 Favorites 를 그리지 않으므로 None 이다. 본문이 다시 그 높이에 닿으면 사다리로 돌아온다.
pub(super) fn height(theme: &Theme, body_h: f32) -> Option<f32> {
    (body_h >= theme.explorer_favorites_hide_below().value()).then(|| pin_height(body_h))
}

/// 본문 높이가 `FAV_PIN_THRESHOLD_H` 이상이면 `FAV_PIN_BASE_H` 고정, 미만이면 본문 높이의
/// `FAV_PIN_RATIO` 를 4px 그리드로 스냅한 값과 `FAV_PIN_MIN_H` 중 큰 값.
fn pin_height(body_h: f32) -> f32 {
    if body_h <= 0.0 || body_h >= FAV_PIN_THRESHOLD_H.value() {
        return FAV_PIN_BASE_H.value();
    }
    ((body_h * FAV_PIN_RATIO / 4.0).round() * 4.0).max(FAV_PIN_MIN_H.value())
}

#[cfg(test)]
mod tests {
    use super::{height, pin_height};

    /// design 시안 pin 높이 사다리: 본문 높이 → 고정 높이.
    #[test]
    fn pin_height_matches_design_ladder() {
        assert_eq!(pin_height(620.0), 240.0);
        assert_eq!(pin_height(600.0), 240.0);
        assert_eq!(pin_height(560.0), 224.0);
        assert_eq!(pin_height(420.0), 168.0);
        assert_eq!(pin_height(300.0), 120.0);
        assert_eq!(pin_height(100.0), 120.0);
        assert_eq!(pin_height(0.0), 240.0);
    }

    /// design Short cell: 본문 240 미만은 Files 만, 240 이상은 사다리 그대로.
    #[test]
    fn favorites_drop_below_the_hide_threshold() {
        let theme = crate::theme::theme();
        let below = theme.explorer_favorites_hide_below().value();
        assert_eq!(height(&theme, 300.0), Some(120.0));
        assert_eq!(height(&theme, below), Some(pin_height(below)));
        assert_eq!(height(&theme, below - 1.0), None);
        assert_eq!(height(&theme, 90.0), None);
    }
}
