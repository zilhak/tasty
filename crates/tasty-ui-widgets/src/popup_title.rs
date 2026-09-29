//! 본체와 갤러리가 공용 popup 타이틀바의 제목 영역 계산을 공유한다.

use tasty_type_geometry::length::LogicalPx;

/// 오른쪽 버튼 배치에 따른 제목 정렬 방식.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopupTitleAlign {
    /// 오른쪽 예약 폭을 왼쪽에도 똑같이 비워 제목 중심을 스트립 중심에 둔다.
    /// 닫기 버튼 하나이거나 버튼이 없는 타이틀바에 쓴다.
    Symmetric,
    /// 가장 왼쪽 버튼 앞까지를 제목 영역으로 둔다.
    /// 버튼이 둘인 타이틀바의 배치 규칙은 시안에 아직 없어 기존 배치를 유지한다.
    BeforeButtons,
}

impl PopupTitleAlign {
    /// 오른쪽 버튼이 닫기 하나 이하면 대칭, 둘 이상이면 기존 배치를 쓴다.
    pub fn for_button_count(count: usize) -> Self {
        if count <= 1 {
            Self::Symmetric
        } else {
            Self::BeforeButtons
        }
    }
}

/// 제목을 그리고 말줄임할 사각형을 반환한다. 폭은 0 이상이며 버튼 영역과 겹치지 않는다.
/// `buttons_left_x`는 가장 왼쪽 버튼의 왼쪽 경계이고 버튼이 없으면 `title_rect.max.x`다.
pub fn popup_title_text_rect(
    title_rect: egui::Rect,
    buttons_left_x: f32,
    pad: LogicalPx,
    align: PopupTitleAlign,
) -> egui::Rect {
    let pad = pad.value();
    let buttons_left_x = buttons_left_x.min(title_rect.max.x);
    let before_buttons = || {
        let left = title_rect.min.x + pad;
        (left, (buttons_left_x - pad).max(left))
    };
    let (left, right) = match align {
        PopupTitleAlign::Symmetric => {
            let reserve = title_rect.max.x - buttons_left_x + pad;
            let (left, right) = (title_rect.min.x + reserve, title_rect.max.x - reserve);
            // 좁아서 양쪽 예약이 겹치면 버튼 앞 영역으로 대신해 버튼과 겹치지 않게 한다.
            if right < left {
                before_buttons()
            } else {
                (left, right)
            }
        }
        PopupTitleAlign::BeforeButtons => before_buttons(),
    };
    egui::Rect::from_min_max(
        egui::pos2(left, title_rect.min.y),
        egui::pos2(right, title_rect.max.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(x0: f32, w: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x0, 50.0), egui::vec2(w, 28.0))
    }

    /// 닫기 버튼 20 + 가장자리 4 → 버튼 왼쪽 경계 = 오른쪽 끝 − 24.
    fn close_only_left(r: egui::Rect) -> f32 {
        r.max.x - 24.0
    }

    #[test]
    fn a_single_button_centres_the_title_on_the_strip() {
        let r = strip(382.0, 440.0);
        let t = popup_title_text_rect(
            r,
            close_only_left(r),
            LogicalPx(8.0),
            PopupTitleAlign::Symmetric,
        );
        assert_eq!(t.center().x, r.center().x);
        assert_eq!(t.min.x, r.min.x + 32.0);
        assert_eq!(t.max.x, r.max.x - 32.0);
        assert!(t.max.x <= close_only_left(r));
        assert_eq!((t.min.y, t.max.y), (r.min.y, r.max.y));
    }

    #[test]
    fn two_buttons_keep_the_area_before_the_buttons() {
        let r = strip(0.0, 352.0);
        // 전체화면 20 + 간격 4 + 닫기 20 + 가장자리 4.
        let left = r.max.x - 48.0;
        let t = popup_title_text_rect(r, left, LogicalPx(8.0), PopupTitleAlign::BeforeButtons);
        assert_eq!(t.min.x, 8.0);
        assert_eq!(t.max.x, left - 8.0);
        assert_eq!(t.center().x, r.center().x - 24.0);
    }

    #[test]
    fn no_buttons_reserve_only_the_padding() {
        let r = strip(10.0, 200.0);
        let t = popup_title_text_rect(r, r.max.x, LogicalPx(8.0), PopupTitleAlign::Symmetric);
        assert_eq!(t.min.x, 18.0);
        assert_eq!(t.max.x, 202.0);
    }

    #[test]
    fn a_narrow_strip_never_yields_a_negative_width() {
        for align in [PopupTitleAlign::Symmetric, PopupTitleAlign::BeforeButtons] {
            let r = strip(100.0, 40.0);
            let t = popup_title_text_rect(r, close_only_left(r), LogicalPx(8.0), align);
            assert!(t.width() >= 0.0, "{align:?}: {}", t.width());
            assert!(t.max.x <= close_only_left(r), "{align:?}: {}", t.max.x);
            // 버튼 칸보다 좁아 버튼 경계가 스트립 왼쪽 밖으로 나간 경우.
            let r = strip(100.0, 20.0);
            let t = popup_title_text_rect(r, close_only_left(r), LogicalPx(8.0), align);
            assert!(t.width() >= 0.0, "{align:?}: {}", t.width());
        }
    }

    #[test]
    fn the_button_count_picks_the_alignment() {
        assert_eq!(
            PopupTitleAlign::for_button_count(0),
            PopupTitleAlign::Symmetric
        );
        assert_eq!(
            PopupTitleAlign::for_button_count(1),
            PopupTitleAlign::Symmetric
        );
        assert_eq!(
            PopupTitleAlign::for_button_count(2),
            PopupTitleAlign::BeforeButtons
        );
    }
}
