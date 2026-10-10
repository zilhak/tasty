//! 현재 검색 매치 글자색(`search_match_active_fg`)이 직렬화 값에 없을 때의 도출.
//! 이 필드보다 먼저 나온 host 는 값을 보내지 않으므로, 역직렬화는 투명 표지를 넣고
//! Theme 을 만들 때 base 와 text 중 active 채움 위에서 대비가 큰 쪽을 고른다.
//! 내장 테마에서는 디자인이 정한 값(Mocha base · Latte text)과 같다.

use crate::color::HexColor;
use crate::theme::ThemeColors;

/// 역직렬화 기본값. 알파 0 글자색은 쓸모가 없으므로 "값 없음" 표지로 쓴다.
pub(crate) fn unset_search_match_active_fg() -> HexColor {
    crate::hex!("#00000000")
}

fn luminance((r, g, b): (f32, f32, f32)) -> f32 {
    let lin = |v: f32| {
        let v = v / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn contrast(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn rgb(c: HexColor) -> (f32, f32, f32) {
    (f32::from(c.r()), f32::from(c.g()), f32::from(c.b()))
}

impl ThemeColors {
    /// 값이 있으면 그대로, 없으면 base 위에 합성한 active 채움에 대해 base·text 중 대비가 큰 색.
    pub(crate) fn resolved_search_match_active_fg(&self) -> HexColor {
        if self.search_match_active_fg.a() > 0 {
            return self.search_match_active_fg;
        }
        let top = self.search_match_active_bg;
        let a = f32::from(top.a()) / 255.0;
        let (tr, tg, tb) = rgb(top);
        let (br, bg, bb) = rgb(self.base);
        let fill = (
            tr * a + br * (1.0 - a),
            tg * a + bg * (1.0 - a),
            tb * a + bb * (1.0 - a),
        );
        if contrast(rgb(self.base), fill) >= contrast(rgb(self.text), fill) {
            self.base
        } else {
            self.text
        }
    }
}
