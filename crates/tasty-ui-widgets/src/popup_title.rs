//! 본체와 갤러리가 공용 popup 타이틀바의 제목 영역 계산을 공유한다.
//!
//! 제목은 오른쪽 버튼 수와 관계없이 스트립 중앙에 둔다. 양쪽 예약 폭은
//! `popup-title-edge-inset + N × popup-title-btn-size + (N − 1) × popup-title-btn-gap +
//! popup-title-text-gap`이다(버튼 하나 32, 둘 60).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::tooltip::Tooltip;

/// 제목을 그리고 말줄임할 사각형을 반환한다. 폭은 0 이상이며 버튼 영역과 겹치지 않는다.
/// `buttons_left_x`는 가장 왼쪽 버튼의 왼쪽 경계이고 버튼이 없으면 `title_rect.max.x`다.
/// `text_gap`은 버튼 묶음과 제목 사이 간격(`popup-title-text-gap`)이다.
///
/// 오른쪽 예약 폭(스트립 오른쪽 끝 − 버튼 경계 + `text_gap`)을 왼쪽에도 똑같이 비운다.
/// 스트립이 좁아 양쪽 예약이 겹치면 버튼 앞 영역을 대신 써서 버튼과 겹치지 않게 한다.
pub fn popup_title_text_rect(
    title_rect: egui::Rect,
    buttons_left_x: f32,
    text_gap: LogicalPx,
) -> egui::Rect {
    let gap = text_gap.value();
    let buttons_left_x = buttons_left_x.min(title_rect.max.x);
    let reserve = title_rect.max.x - buttons_left_x + gap;
    let (mut left, mut right) = (title_rect.min.x + reserve, title_rect.max.x - reserve);
    if right < left {
        left = title_rect.min.x + gap;
        right = (buttons_left_x - gap).max(left);
    }
    egui::Rect::from_min_max(
        egui::pos2(left, title_rect.min.y),
        egui::pos2(right, title_rect.max.y),
    )
}

/// 제목이 `max_width`에 들어가지 않으면 뒤를 잘라 `…`를 붙인다. 폭이 0 이하면 빈 문자열이다.
/// 결과가 원래 제목과 다르면 잘린 것이다.
pub fn elide_popup_title(
    ctx: &egui::Context,
    text: &str,
    font: egui::FontId,
    max_width: f32,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    let width_of = |t: &str| {
        ctx.fonts(|f| {
            f.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                .rect
                .width()
        })
    };
    if width_of(text) <= max_width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if width_of(&candidate) <= max_width {
            return candidate;
        }
    }
    "…".to_owned()
}

/// 잘린 제목의 전체 문구를 제목 띠 `band`에 붙인 Tooltip으로 보여 준다. 배치는 위, 들어가지
/// 않으면 아래다. `show`는 호출부가 판정한다(호버 지연 경과 또는 강제 표시). 제목이 잘리지 않았으면
/// 호출하지 않는다.
pub fn show_popup_title_tooltip(
    ctx: &egui::Context,
    theme: &Theme,
    id: egui::Id,
    title: &str,
    band: egui::Rect,
) {
    Tooltip::new(title)
        .id_source(id)
        .placement_top_then_bottom(ctx, theme, band, ctx.screen_rect())
        .show_in(ctx, theme, band);
}

/// painter만 쓰는 타이틀바에 아이콘을 그린다. SVG 텍스처가 아직 준비되지 않았으면
/// 다음 프레임을 요청한다. 로드 실패는 호출부가 기록한다.
pub fn paint_popup_title_glyph(
    ctx: &egui::Context,
    painter: &egui::Painter,
    icon: tasty_icons::Icon,
    rect: egui::Rect,
    tint: egui::Color32,
) -> Result<(), egui::load::LoadError> {
    match icon
        .image(rect.width(), tint)
        .load_for_size(ctx, rect.size())?
    {
        egui::load::TexturePoll::Ready { texture } => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture.id, rect, uv, tint);
        }
        egui::load::TexturePoll::Pending { .. } => ctx.request_repaint(),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 디자인 값: 버튼 24, 간격 4, 가장자리 4, 제목 간격 4.
    const BTN: f32 = 24.0;
    const GAP: f32 = 4.0;
    const EDGE: f32 = 4.0;
    const TEXT_GAP: LogicalPx = LogicalPx(4.0);

    fn strip(x0: f32, w: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x0, 50.0), egui::vec2(w, 28.0))
    }

    /// 버튼 N개일 때 가장 왼쪽 버튼의 왼쪽 경계.
    fn buttons_left(r: egui::Rect, n: usize) -> f32 {
        r.max.x - EDGE - n as f32 * BTN - n.saturating_sub(1) as f32 * GAP
    }

    #[test]
    fn a_single_button_reserves_32_on_both_sides() {
        let r = strip(382.0, 440.0);
        let t = popup_title_text_rect(r, buttons_left(r, 1), TEXT_GAP);
        assert_eq!(t.center().x, r.center().x);
        assert_eq!(t.min.x, r.min.x + 32.0);
        assert_eq!(t.max.x, r.max.x - 32.0);
        assert!(t.max.x <= buttons_left(r, 1));
        assert_eq!((t.min.y, t.max.y), (r.min.y, r.max.y));
    }

    #[test]
    fn two_buttons_reserve_60_on_both_sides() {
        let r = strip(0.0, 352.0);
        let t = popup_title_text_rect(r, buttons_left(r, 2), TEXT_GAP);
        assert_eq!(t.center().x, r.center().x);
        assert_eq!(t.min.x, 60.0);
        assert_eq!(t.max.x, 352.0 - 60.0);
        assert!(t.max.x <= buttons_left(r, 2));
    }

    #[test]
    fn no_buttons_reserve_only_the_text_gap() {
        let r = strip(10.0, 200.0);
        let t = popup_title_text_rect(r, r.max.x, TEXT_GAP);
        assert_eq!(t.min.x, 14.0);
        assert_eq!(t.max.x, 206.0);
    }

    #[test]
    fn a_narrow_strip_never_yields_a_negative_width() {
        for n in [1, 2] {
            let r = strip(100.0, 80.0);
            let t = popup_title_text_rect(r, buttons_left(r, n), TEXT_GAP);
            assert!(t.width() >= 0.0, "{n}: {}", t.width());
            assert!(t.max.x <= buttons_left(r, n), "{n}: {}", t.max.x);
            // 버튼 칸보다 좁아 버튼 경계가 스트립 왼쪽 밖으로 나간 경우.
            let r = strip(100.0, 20.0);
            let t = popup_title_text_rect(r, buttons_left(r, n), TEXT_GAP);
            assert!(t.width() >= 0.0, "{n}: {}", t.width());
        }
    }
}
