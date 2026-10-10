//! 줄 길이를 고르게 맞추는 줄바꿈(시안 CSS `text-wrap: balance`).
//! 최대 폭에서 나온 줄 수를 유지하는 가장 좁은 폭으로 다시 배치한다. 그 폭이 가장 긴 줄을
//! 최소로 만들므로 마지막 줄에 한두 글자만 남는 꼬리("ありま / す。")가 생기지 않는다.

use std::sync::Arc;

/// 폭을 반으로 나눠 찾는 횟수. 상태 화면 폭(수백 px)에서 1px 아래까지 좁힌다.
const BISECT_STEPS: usize = 12;

/// `text` 를 `max_w` 안에서 가운데 정렬로 배치하되, 여러 줄이면 줄 길이가 고르게 되는 폭을 쓴다.
/// 한 줄에 들어가면 그대로 둔다. 같은 입력은 같은 폭들로 배치하므로 egui 배치 캐시를 그대로 탄다.
pub(crate) fn balanced_galley(
    ctx: &egui::Context,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_w: f32,
) -> Arc<egui::Galley> {
    let lay = |w: f32| {
        let mut job = egui::text::LayoutJob::simple(text.to_owned(), font.clone(), color, w);
        job.halign = egui::Align::Center;
        ctx.fonts(|f| f.layout_job(job))
    };
    let full = lay(max_w);
    let rows = full.rows.len();
    if rows <= 1 {
        return full;
    }
    // 폭이 좁을수록 줄 수는 줄지 않는다. hi 는 언제나 줄 수가 그대로인 폭이다.
    let (mut lo, mut hi) = (0.0_f32, max_w);
    for _ in 0..BISECT_STEPS {
        let mid = (lo + hi) * 0.5;
        if lay(mid).rows.len() <= rows {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    lay(hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        // 첫 프레임은 폰트 준비 전이라 한 번 돌려 둔다.
        let _output = ctx.run(egui::RawInput::default(), |_| {});
        ctx
    }

    /// 상태 화면 보조 줄과 같은 caption 크기.
    fn font() -> egui::FontId {
        let theme = tasty_type_appearance::theme::Theme::with_colors_and_zoom(
            tasty_themes::mocha_fallback_colors(),
            false,
            1.0,
        );
        egui::FontId::proportional(theme.font_size_caption.value())
    }

    fn row_texts(g: &egui::Galley) -> Vec<String> {
        g.rows
            .iter()
            .map(|r| r.glyphs.iter().map(|gl| gl.chr).collect::<String>())
            .collect()
    }

    fn widest(g: &egui::Galley) -> f32 {
        g.rows.iter().map(|r| r.rect.width()).fold(0.0, f32::max)
    }

    fn greedy(ctx: &egui::Context, text: &str, w: f32) -> Arc<egui::Galley> {
        let job = egui::text::LayoutJob::simple(text.to_owned(), font(), egui::Color32::WHITE, w);
        ctx.fonts(|f| f.layout_job(job))
    }

    /// 한 줄에 들어가는 문장은 그대로 한 줄이다.
    #[test]
    fn a_line_that_fits_stays_one_line() {
        let ctx = ctx();
        let g = balanced_galley(&ctx, "No image loaded", font(), egui::Color32::WHITE, 300.0);
        assert_eq!(g.rows.len(), 1);
    }

    /// 줄 수는 그대로이고 가장 긴 줄은 앞에서부터 채운 배치보다 길지 않으며, 마지막 줄에
    /// 한 낱말만 남는 꼬리가 사라진다.
    #[test]
    fn the_same_number_of_lines_with_no_short_tail() {
        let ctx = ctx();
        let text = "Over 16384\u{a0}px on a side, or needs more than 512\u{a0}MiB to decode.";
        for w in [120.0, 160.0, 200.0, 260.0] {
            let plain = greedy(&ctx, text, w);
            let g = balanced_galley(&ctx, text, font(), egui::Color32::WHITE, w);
            assert_eq!(g.rows.len(), plain.rows.len(), "width {w}");
            assert!(widest(&g) <= widest(&plain) + 0.01, "width {w}");
            assert!(widest(&g) <= w + 0.01, "width {w}");
            if g.rows.len() > 1 {
                let last = g.rows.last().map_or(0.0, |r| r.rect.width());
                assert!(last >= widest(&g) * 0.5, "width {w}: {:?}", row_texts(&g));
            }
        }
    }

    /// 앞에서부터 채우면 한 낱말이 남는 문장도 고른 두 줄이 된다.
    #[test]
    fn a_one_word_tail_moves_up_to_even_lines() {
        let ctx = ctx();
        let text = "Image is too large to open";
        let one = greedy(&ctx, text, 1000.0).rows[0].rect.width();
        let w = one - 1.0;
        let plain = greedy(&ctx, text, w);
        assert_eq!(row_texts(&plain).last().map(|s| s.trim()), Some("open"));
        let g = balanced_galley(&ctx, text, font(), egui::Color32::WHITE, w);
        assert_eq!(g.rows.len(), 2);
        let texts = row_texts(&g);
        assert_ne!(texts[1].trim(), "open", "{texts:?}");
    }

    /// 숫자와 단위 사이의 NBSP 에서는 줄을 바꾸지 않는다.
    #[test]
    fn a_number_stays_with_its_unit() {
        let ctx = ctx();
        let text = "Over 16384\u{a0}px on a side, or needs more than 512\u{a0}MiB to decode.";
        for w in (60..=260).step_by(4) {
            let g = balanced_galley(&ctx, text, font(), egui::Color32::WHITE, w as f32);
            for row in row_texts(&g) {
                let row = row.trim_end();
                assert!(
                    !row.ends_with("16384") && !row.ends_with("512"),
                    "width {w}: {:?}",
                    row_texts(&g)
                );
            }
        }
    }
}
