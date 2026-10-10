//! UI 문장 안의 CLI 조각(명령·옵션·인자)을 code run 으로 그린다.
//!
//! UI 글꼴은 하이픈 두 개를 대시 하나처럼 이어 그려 `--webhook-port` 가 `–webhook-port` 로 읽힌다.
//! 번역 문자열은 CLI 조각을 백틱으로 감싸고, 이 위젯이 그 구간을 문장과 같은 크기의 mono 글자,
//! `ui-code-bg` 채움, 좌우 `ui-code-padding-x`, 모서리 `ui-code-radius` 로 그린다. 세로 여백은 없어
//! 줄 높이가 바뀌지 않는다. 조각 안의 공백은 줄을 나누지 않는 공백으로 바꿔 문장이 조각 바깥에서만
//! 줄을 바꾼다. 조각 하나가 줄 폭보다 길면 egui 가 조각 안에서도 자른다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// 줄을 나누지 않는 공백. egui 는 이 글자에서 줄을 바꾸지 않는다.
const NO_BREAK_SPACE: char = '\u{A0}';

/// code run 의 토큰 값.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiCodeTokens {
    /// `ui-code-bg`.
    pub bg: egui::Color32,
    /// `ui-code-fg`.
    pub fg: egui::Color32,
    /// `ui-code-padding-x`.
    pub padding_x: LogicalPx,
    /// `ui-code-radius`.
    pub radius: LogicalPx,
}

impl UiCodeTokens {
    // TODO(ui-code): batch 11 토큰 접근자가 들어오면 theme.ui_code_*() 로 바꾼다. 지금은 같은 값의 의미 토큰이다.
    pub fn of(theme: &Theme) -> Self {
        Self {
            bg: theme.surface_raised().to_egui(),
            fg: theme.text_primary().to_egui(),
            padding_x: theme.spacing_xs,
            radius: theme.corner_radius_sm,
        }
    }
}

/// 문장의 한 구간.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiCopySpan<'a> {
    /// 일반 문장.
    Text(&'a str),
    /// 백틱 안의 CLI 조각. 백틱은 뺀다.
    Code(&'a str),
}

/// 백틱으로 감싼 구간을 code run 으로 나눈다. 닫히지 않은 백틱과 빈 구간(``)은 글자 그대로 둔다.
pub fn split_ui_copy(text: &str) -> Vec<UiCopySpan<'_>> {
    let mut out = Vec::new();
    let mut plain_start = 0;
    let mut cursor = 0;
    while let Some(open) = text[cursor..].find('`').map(|i| cursor + i) {
        let Some(close) = text[open + 1..].find('`').map(|i| open + 1 + i) else {
            break;
        };
        if close == open + 1 {
            cursor = close + 1;
            continue;
        }
        if plain_start < open {
            out.push(UiCopySpan::Text(&text[plain_start..open]));
        }
        out.push(UiCopySpan::Code(&text[open + 1..close]));
        plain_start = close + 1;
        cursor = plain_start;
    }
    if plain_start < text.len() {
        out.push(UiCopySpan::Text(&text[plain_start..]));
    }
    out
}

/// 문장 하나의 글자 배치. code run 의 좌우 여백은 앞뒤 구간의 `leading_space` 로 둔다.
pub fn ui_copy_job(
    theme: &Theme,
    text: &str,
    size: LogicalPx,
    color: egui::Color32,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let pad = UiCodeTokens::of(theme).padding_x.value();
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let text_format = egui::TextFormat {
        font_id: egui::FontId::proportional(size.value()),
        color,
        ..Default::default()
    };
    let code_format = egui::TextFormat {
        font_id: egui::FontId::monospace(size.value()),
        color: UiCodeTokens::of(theme).fg,
        valign: egui::Align::Center,
        ..Default::default()
    };
    let mut after_code = false;
    for span in split_ui_copy(text) {
        match span {
            UiCopySpan::Text(s) => {
                job.append(s, if after_code { pad } else { 0.0 }, text_format.clone());
                after_code = false;
            }
            UiCopySpan::Code(s) => {
                let unbroken: String = s
                    .chars()
                    .map(|c| if c == ' ' { NO_BREAK_SPACE } else { c })
                    .collect();
                job.append(
                    &unbroken,
                    if after_code { pad * 2.0 } else { pad },
                    code_format.clone(),
                );
                after_code = true;
            }
        }
    }
    job
}

/// code run 이 차지하는 채움 사각형(galley 기준). 한 run 이 여러 줄에 걸치면 줄마다 하나다.
pub fn ui_code_rects(theme: &Theme, galley: &egui::Galley) -> Vec<egui::Rect> {
    let pad = UiCodeTokens::of(theme).padding_x.value();
    let is_code = |section: u32| {
        galley
            .job
            .sections
            .get(section as usize)
            .is_some_and(|s| s.format.font_id.family == egui::FontFamily::Monospace)
    };
    let mut rects = Vec::new();
    for row in &galley.rows {
        let mut run: Option<(u32, f32, f32)> = None;
        for glyph in &row.glyphs {
            let code = is_code(glyph.section_index);
            match (&mut run, code) {
                (Some((section, _, max_x)), true) if *section == glyph.section_index => {
                    *max_x = glyph.max_x();
                }
                _ => {
                    if let Some((_, min_x, max_x)) = run.take() {
                        rects.push(code_rect(row.rect, min_x, max_x, pad));
                    }
                    if code {
                        run = Some((glyph.section_index, glyph.pos.x, glyph.max_x()));
                    }
                }
            }
        }
        if let Some((_, min_x, max_x)) = run {
            rects.push(code_rect(row.rect, min_x, max_x, pad));
        }
    }
    rects
}

fn code_rect(row: egui::Rect, min_x: f32, max_x: f32, pad: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(min_x - pad, row.min.y),
        egui::pos2(max_x + pad, row.max.y),
    )
}

/// 채움까지 포함한 문장 크기. 문장 끝의 run 은 오른쪽 여백이 글자 배치 밖에 있어 따로 더한다.
pub fn ui_copy_size(theme: &Theme, galley: &egui::Galley) -> egui::Vec2 {
    ui_code_rects(theme, galley)
        .iter()
        .fold(galley.rect, |acc, r| acc.union(*r))
        .size()
}

/// `pos` 에 code run 채움을 칠하고 그 위에 문장을 그린다. 글자 색은 글자 배치가 정한다.
pub fn paint_ui_copy(
    painter: &egui::Painter,
    theme: &Theme,
    pos: egui::Pos2,
    galley: std::sync::Arc<egui::Galley>,
) {
    let tokens = UiCodeTokens::of(theme);
    for r in ui_code_rects(theme, &galley) {
        painter.rect_filled(r.translate(pos.to_vec2()), tokens.radius.value(), tokens.bg);
    }
    painter.galley(pos, galley, tokens.fg);
}

/// 문장을 남은 폭에 맞춰 줄바꿈해 그린다. 백틱 구간은 code run 이다.
pub fn ui_copy(
    ui: &mut egui::Ui,
    theme: &Theme,
    text: &str,
    size: LogicalPx,
    color: egui::Color32,
) -> egui::Response {
    let job = ui_copy_job(theme, text, size, color, ui.available_width());
    let galley = ui.fonts(|f| f.layout_job(job));
    let (rect, response) =
        ui.allocate_exact_size(ui_copy_size(theme, &galley), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_ui_copy(ui.painter(), theme, rect.min, galley);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backticks_mark_code_runs() {
        assert_eq!(
            split_ui_copy("Start with `--webhook-port 7421` or quit."),
            vec![
                UiCopySpan::Text("Start with "),
                UiCopySpan::Code("--webhook-port 7421"),
                UiCopySpan::Text(" or quit."),
            ]
        );
        assert_eq!(
            split_ui_copy("`tasty webhook list`"),
            vec![UiCopySpan::Code("tasty webhook list")]
        );
    }

    #[test]
    fn unclosed_or_empty_backticks_stay_literal() {
        assert_eq!(split_ui_copy("a `tick"), vec![UiCopySpan::Text("a `tick")]);
        assert_eq!(split_ui_copy("a `` b"), vec![UiCopySpan::Text("a `` b")]);
    }

    /// 한 프레임 안에서 폭마다 글자 배치를 한다.
    fn with_layouts(
        theme: &Theme,
        text: &str,
        wraps: &[f32],
        mut f: impl FnMut(f32, &egui::Galley),
    ) {
        let ctx = egui::Context::default();
        // 출력은 쓰지 않는다. 측정은 `f` 안에서 끝난다.
        drop(ctx.run(Default::default(), |ctx| {
            for &wrap in wraps {
                let job = ui_copy_job(
                    theme,
                    text,
                    theme.font_size_body,
                    egui::Color32::WHITE,
                    wrap,
                );
                let galley = ctx.fonts(|fonts| fonts.layout_job(job));
                f(wrap, &galley);
            }
        }));
    }

    #[test]
    fn a_run_is_filled_with_side_padding_and_the_row_height() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let pad = UiCodeTokens::of(&theme).padding_x.value();
        with_layouts(
            &theme,
            "Use `--webhook-port` now.",
            &[f32::INFINITY],
            |_, galley| {
                let rects = ui_code_rects(&theme, galley);
                assert_eq!(rects.len(), 1);
                let row = &galley.rows[0];
                let code: Vec<_> = row.glyphs.iter().filter(|g| g.section_index == 1).collect();
                let first = code.first().expect("code glyphs");
                let last = code.last().expect("code glyphs");
                assert_eq!(rects[0].min.x, first.pos.x - pad);
                assert_eq!(rects[0].max.x, last.max_x() + pad);
                assert_eq!(rects[0].height(), row.rect.height());
                // 앞뒤 문장 글자는 채움 밖에 있다. egui 가 글자 커서를 픽셀에 맞춰 반올림하므로 반 픽셀을 허용한다.
                let before = row
                    .glyphs
                    .iter()
                    .rfind(|g| g.section_index == 0)
                    .expect("text");
                let after = row
                    .glyphs
                    .iter()
                    .find(|g| g.section_index == 2)
                    .expect("text");
                assert!(before.max_x() <= rects[0].min.x + 0.5);
                assert!(after.pos.x >= rects[0].max.x - 0.5);
            },
        );
    }

    #[test]
    fn a_narrow_line_wraps_around_the_run_not_inside_it() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let text = "Start Tasty with `--webhook-port 7421` or close the other app.";
        // run 하나는 들어가지만 문장은 줄을 바꾸는 폭을 모두 훑는다. 어느 폭에서든 run 은 한 줄이다.
        let mut run_w = 0.0;
        with_layouts(&theme, text, &[f32::INFINITY], |_, galley| {
            run_w = ui_code_rects(&theme, galley)[0].width();
        });
        let wraps: Vec<f32> = (1..120).map(|i| run_w.ceil() + 2.0 * i as f32).collect();
        let mut wrapped = 0;
        with_layouts(&theme, text, &wraps, |wrap, galley| {
            if galley.rows.len() > 1 {
                wrapped += 1;
            }
            let rows_with_code = galley
                .rows
                .iter()
                .filter(|r| r.glyphs.iter().any(|g| g.section_index == 1))
                .count();
            assert_eq!(rows_with_code, 1, "the run broke at wrap width {wrap}");
        });
        assert!(wrapped > 0, "no width made the sentence wrap");
    }

    #[test]
    fn multibyte_text_around_a_run_survives() {
        assert_eq!(
            split_ui_copy("`--webhook-port`로 지정했다"),
            vec![
                UiCopySpan::Code("--webhook-port"),
                UiCopySpan::Text("로 지정했다"),
            ]
        );
    }
}
