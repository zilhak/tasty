//! 안내 모달 셸의 본문과 버튼 행. 본체 `info_modal.rs`와 갤러리 specimen이 함께 호출한다.
//!
//! 셸 규칙은 큐의 모든 메시지에 같다. 본문은 문단 사이를 `info-modal-para-gap`만큼(이어진 번호
//! 항목 사이는 `space-xs`만큼) 띄우고 넘치면 스크롤한다. 버튼 행은 스크롤하지 않고 오른쪽 정렬이며, 닫기 버튼(마지막 항목)이
//! 가장 오른쪽에 온다. 아래로 가려진 내용이 있을 때만 버튼 행 위에 1px 경계를 긋는다.
//! 제목은 팝업 타이틀바가 그리므로 여기에는 없다.
//!
//! 강조는 메시지가 직접 표시한 구간에만 준다(현재 macOS 권한 안내 하나). 표기는
//! [`parse_emphasis`]를 따르며, 번호 목록 항목과 보조 문단도 같은 표기로 고른다. egui는 굵기를 고를 수 없어 도입부·경로는 색으로만 구분된다.
//! 명령은 UI 문장의 code run 과 같은 모양이다(문장 크기 mono, `ui-code-*` 채움·여백·반경).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::button::{Button, ButtonVariant};
use crate::control::ControlSize;
use crate::ui_code::{UiCodeTokens, ui_code_font, ui_code_rects, unbroken};

/// 본문 강조 구간의 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoModalSpanKind {
    /// 강조 없는 문장.
    Plain,
    /// 문단 도입부(`**…**`). 디자인은 text-primary · semibold.
    Lead,
    /// 설정 경로(`*…*`). 디자인은 text-primary · medium.
    Path,
    /// 셸 명령(`` `…` ``). 문단과 같은 크기의 code run 이다.
    Command,
}

/// 문단의 종류. 강조 표기를 해석할 때만 `Text` 밖의 종류가 나온다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InfoModalParagraphKind {
    /// 일반 문단. 본문 크기 · text-secondary.
    Text,
    /// 번호 목록 항목(`1. …`). 값은 번호 표지("1."). 본문 크기 · text-primary, 표지 뒤
    /// `space-xl` 들여쓰기에 줄을 맞추고, 이어진 항목 사이는 `space-xs` 만 띄운다.
    Step(String),
    /// 보조 문단(`> …`). caption 크기 · text-muted.
    Aside,
}

/// 본문 문단 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoModalParagraph {
    pub kind: InfoModalParagraphKind,
    pub spans: Vec<InfoModalSpan>,
}

/// 문단 하나를 이루는 구간.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoModalSpan {
    pub kind: InfoModalSpanKind,
    pub text: String,
}

/// 버튼 하나. 왼쪽부터 순서대로 주며 마지막 항목이 닫기 버튼이다.
#[derive(Clone, Copy, Debug)]
pub struct InfoModalButton<'a> {
    pub label: &'a str,
    pub variant: ButtonVariant,
}

/// 셸 본문과 버튼 행의 입력.
pub struct InfoModalView<'a> {
    /// 스크롤 상태를 구분하는 id. 한 화면에 여러 셸을 그리는 갤러리는 각각 달라야 한다.
    pub id_salt: egui::Id,
    /// 빈 줄로 문단을 나눈 본문.
    pub body: &'a str,
    /// true면 본문의 강조 표기를 해석한다. false면 표기 문자도 그대로 보인다.
    pub emphasis: bool,
    pub buttons: &'a [InfoModalButton<'a>],
    /// 스크롤 위치를 0(맨 위)..1(맨 끝) 비율로 고정한다. 갤러리 전용이며 본체는 `None`이다.
    pub scroll_to: Option<f32>,
}

/// 그린 결과.
pub struct InfoModalOutput {
    /// 눌린 버튼의 `buttons` 인덱스.
    pub clicked: Option<usize>,
    /// 스크롤 영역 안 콘텐츠 높이(위아래 여백 포함). 셸 높이 계산에 쓴다.
    pub body_content_height: LogicalPx,
    /// 아래로 가려진 내용이 있어 버튼 행 위 경계를 그렸는지.
    pub hidden_below: bool,
}

/// 버튼 행 높이. 버튼(Md)과 위아래 `space-md` 여백.
pub fn footer_height(theme: &Theme) -> LogicalPx {
    LogicalPx(ControlSize::Md.height(theme)) + theme.spacing_md * 2.0
}

/// 셸 전체 높이. 타이틀바·본문 콘텐츠·버튼 행을 더해 `info-modal-min-height`..`max-height`로 자른다.
pub fn shell_height(
    theme: &Theme,
    title_bar_height: LogicalPx,
    body_content_height: LogicalPx,
) -> LogicalPx {
    (title_bar_height + body_content_height + footer_height(theme))
        .clamp(theme.info_modal_min_height(), theme.info_modal_max_height())
}

/// 본문을 문단과 강조 구간으로 나눈다.
///
/// 문단은 빈 줄로 나눈다. `emphasis`가 true면 `**도입부**`, `*경로*`, `` `명령` `` 표기를
/// 해석하며, 닫히지 않은 표기는 글자 그대로 둔다. 같은 경우 문단 첫머리의 `1. `(숫자와 점,
/// 공백)은 번호 목록 항목, `> `는 보조 문단이 된다.
pub fn parse_emphasis(body: &str, emphasis: bool) -> Vec<InfoModalParagraph> {
    body.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            if !emphasis {
                return InfoModalParagraph {
                    kind: InfoModalParagraphKind::Text,
                    spans: vec![InfoModalSpan {
                        kind: InfoModalSpanKind::Plain,
                        text: p.to_string(),
                    }],
                };
            }
            let (kind, rest) = paragraph_kind(p);
            InfoModalParagraph {
                kind,
                spans: parse_paragraph(rest),
            }
        })
        .collect()
}

/// 문단 첫머리 표기로 종류를 고르고, 표기를 뺀 나머지를 돌려준다.
fn paragraph_kind(p: &str) -> (InfoModalParagraphKind, &str) {
    if let Some(rest) = p.strip_prefix("> ") {
        return (InfoModalParagraphKind::Aside, rest);
    }
    let digits = p.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = p[digits..].strip_prefix(". ")
    {
        return (
            InfoModalParagraphKind::Step(format!("{}.", &p[..digits])),
            rest,
        );
    }
    (InfoModalParagraphKind::Text, p)
}

fn parse_paragraph(p: &str) -> Vec<InfoModalSpan> {
    let mut out: Vec<InfoModalSpan> = Vec::new();
    let mut plain = String::new();
    let mut rest = p;
    while !rest.is_empty() {
        let marker = [
            ("**", InfoModalSpanKind::Lead),
            ("`", InfoModalSpanKind::Command),
            ("*", InfoModalSpanKind::Path),
        ]
        .into_iter()
        .find(|(m, _)| rest.starts_with(m));
        if let Some((m, kind)) = marker
            && let Some(end) = rest[m.len()..].find(m)
            && end > 0
        {
            if !plain.is_empty() {
                out.push(InfoModalSpan {
                    kind: InfoModalSpanKind::Plain,
                    text: std::mem::take(&mut plain),
                });
            }
            out.push(InfoModalSpan {
                kind,
                text: rest[m.len()..m.len() + end].to_string(),
            });
            rest = &rest[m.len() * 2 + end..];
            continue;
        }
        let ch = rest.chars().next().unwrap_or_default();
        plain.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    if !plain.is_empty() {
        out.push(InfoModalSpan {
            kind: InfoModalSpanKind::Plain,
            text: plain,
        });
    }
    out
}

/// 문단 종류별 글자 크기와 일반 구간 색.
fn paragraph_style(theme: &Theme, kind: &InfoModalParagraphKind) -> (f32, egui::Color32) {
    match kind {
        InfoModalParagraphKind::Text => (
            theme.font_size_body.value(),
            theme.text_secondary().to_egui(),
        ),
        InfoModalParagraphKind::Step(_) => {
            (theme.font_size_body.value(), theme.text_primary().to_egui())
        }
        InfoModalParagraphKind::Aside => (
            theme.font_size_caption.value(),
            theme.text_muted().to_egui(),
        ),
    }
}

fn paragraph_job(
    theme: &Theme,
    kind: &InfoModalParagraphKind,
    spans: &[InfoModalSpan],
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let (size, plain) = paragraph_style(theme, kind);
    let line_height = Some(size * theme.line_height_ui);
    let code = UiCodeTokens::of(theme);
    let pad = code.padding_x.value();
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    // 명령의 좌우 여백은 code run 처럼 명령 자신과 다음 구간의 앞 간격으로 둔다.
    let mut after_code = false;
    for span in spans {
        let leading = if after_code { pad } else { 0.0 };
        let text_format = |color| egui::TextFormat {
            font_id: egui::FontId::proportional(size),
            color,
            line_height,
            ..Default::default()
        };
        match span.kind {
            InfoModalSpanKind::Plain => job.append(&span.text, leading, text_format(plain)),
            InfoModalSpanKind::Lead | InfoModalSpanKind::Path => job.append(
                &span.text,
                leading,
                text_format(theme.text_primary().to_egui()),
            ),
            InfoModalSpanKind::Command => job.append(
                &unbroken(&span.text),
                leading + pad,
                egui::TextFormat {
                    font_id: egui::FontId::new(size, ui_code_font()),
                    color: code.fg,
                    line_height,
                    valign: egui::Align::Center,
                    ..Default::default()
                },
            ),
        }
        after_code = span.kind == InfoModalSpanKind::Command;
    }
    job
}

/// 문단을 `pos` 에 그린다. 명령 채움을 먼저 칠한다.
fn paint_paragraph(
    painter: &egui::Painter,
    theme: &Theme,
    pos: egui::Pos2,
    galley: std::sync::Arc<egui::Galley>,
) {
    let code = UiCodeTokens::of(theme);
    for r in ui_code_rects(theme, &galley) {
        painter.rect_filled(r.translate(pos.to_vec2()), code.radius.value(), code.bg);
    }
    painter.galley(pos, galley, egui::Color32::PLACEHOLDER);
}

/// 넘겨받은 `ui`의 남은 영역 전체에 본문과 버튼 행을 그린다.
pub fn info_modal(ui: &mut egui::Ui, theme: &Theme, view: &InfoModalView<'_>) -> InfoModalOutput {
    let full = ui.available_rect_before_wrap();
    let footer_h = footer_height(theme).value();
    let footer_rect = egui::Rect::from_min_max(
        egui::pos2(full.min.x, (full.max.y - footer_h).max(full.min.y)),
        full.max,
    );
    let body_rect = egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, footer_rect.min.y));
    let pad_x = theme.spacing_lg.value();
    let pad_y = theme.spacing_md.value();
    let para_gap = theme.info_modal_para_gap().value();
    let paragraphs = parse_emphasis(view.body, view.emphasis);

    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body_rect));
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(view.id_salt)
        .max_height(body_rect.height())
        .auto_shrink([false, false])
        .drag_to_scroll(false);
    if let Some(fraction) = view.scroll_to {
        let max_key = view.id_salt.with("scroll_max");
        let max = body_ui
            .ctx()
            .data(|d| d.get_temp::<f32>(max_key))
            .unwrap_or(0.0);
        scroll = scroll.vertical_scroll_offset((max * fraction.clamp(0.0, 1.0)).round());
    }
    let scrolled = scroll.show(&mut body_ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(pad_y);
        let wrap = (ui.available_width() - pad_x * 2.0).max(0.0);
        let indent = theme.spacing_xl.value();
        for (i, para) in paragraphs.iter().enumerate() {
            if i > 0 {
                let both_steps = matches!(para.kind, InfoModalParagraphKind::Step(_))
                    && matches!(paragraphs[i - 1].kind, InfoModalParagraphKind::Step(_));
                ui.add_space(if both_steps {
                    theme.spacing_xs.value()
                } else {
                    para_gap
                });
            }
            // 번호 항목은 표지를 줄 머리에 두고 글은 들여쓰기에 맞춰 감싼다.
            let text_x = match para.kind {
                InfoModalParagraphKind::Step(_) => pad_x + indent,
                _ => pad_x,
            };
            let galley = ui.fonts(|f| {
                f.layout_job(paragraph_job(
                    theme,
                    &para.kind,
                    &para.spans,
                    (wrap - (text_x - pad_x)).max(0.0),
                ))
            });
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), galley.size().y),
                egui::Sense::hover(),
            );
            if let InfoModalParagraphKind::Step(marker) = &para.kind {
                let (size, color) = paragraph_style(theme, &para.kind);
                let mut job = egui::text::LayoutJob::default();
                job.append(
                    marker,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(size),
                        color,
                        line_height: Some(size * theme.line_height_ui),
                        ..Default::default()
                    },
                );
                let marker_galley = ui.fonts(|f| f.layout_job(job));
                ui.painter().galley(
                    egui::pos2(rect.min.x + pad_x, rect.min.y),
                    marker_galley,
                    egui::Color32::PLACEHOLDER,
                );
            }
            paint_paragraph(
                ui.painter(),
                theme,
                egui::pos2(rect.min.x + text_x, rect.min.y),
                galley,
            );
        }
        ui.add_space(pad_y);
    });
    let content_h = scrolled.content_size.y;
    let visible_h = scrolled.inner_rect.height();
    if view.scroll_to.is_some() {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                view.id_salt.with("scroll_max"),
                (content_h - visible_h).max(0.0),
            )
        });
    }
    // 반올림 오차로 경계가 깜박이지 않도록 반 픽셀 여유를 둔다.
    let hidden_below = scrolled.state.offset.y + visible_h < content_h - 0.5;
    if hidden_below {
        ui.painter().hline(
            footer_rect.x_range(),
            footer_rect.min.y,
            egui::Stroke::new(
                theme.border_width.value(),
                theme.info_modal_scroll_edge().to_egui(),
            ),
        );
    }

    let mut clicked = None;
    let inner = footer_rect.shrink2(egui::vec2(pad_x, pad_y));
    let mut footer_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    footer_ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    // 오른쪽부터 배치하므로 마지막 버튼(닫기)을 먼저 그린다.
    for (i, button) in view.buttons.iter().enumerate().rev() {
        if Button::new(button.label)
            .variant(button.variant)
            .size(ControlSize::Md)
            .show(&mut footer_ui, theme)
            .clicked()
        {
            clicked = Some(i);
        }
    }

    ui.advance_cursor_after_rect(full);
    InfoModalOutput {
        clicked,
        body_content_height: LogicalPx(content_h),
        hidden_below,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(p: &InfoModalParagraph) -> Vec<(InfoModalSpanKind, &str)> {
        p.spans.iter().map(|s| (s.kind, s.text.as_str())).collect()
    }

    #[test]
    fn paragraphs_split_on_blank_lines() {
        let p = parse_emphasis("one\n\ntwo\nstill two\n\n\n\nthree", false);
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].spans[0].text, "two\nstill two");
    }

    #[test]
    fn plain_messages_keep_marker_characters() {
        let p = parse_emphasis("a *b* `c`", false);
        assert_eq!(kinds(&p[0]), vec![(InfoModalSpanKind::Plain, "a *b* `c`")]);
    }

    #[test]
    fn emphasis_markers_become_spans() {
        let p = parse_emphasis("**Lead:** go to *A > B* and run `x --y`.", true);
        assert_eq!(
            kinds(&p[0]),
            vec![
                (InfoModalSpanKind::Lead, "Lead:"),
                (InfoModalSpanKind::Plain, " go to "),
                (InfoModalSpanKind::Path, "A > B"),
                (InfoModalSpanKind::Plain, " and run "),
                (InfoModalSpanKind::Command, "x --y"),
                (InfoModalSpanKind::Plain, "."),
            ]
        );
    }

    #[test]
    fn unclosed_markers_stay_literal() {
        let p = parse_emphasis("5 * 3 and a `tick", true);
        assert_eq!(
            kinds(&p[0]),
            vec![(InfoModalSpanKind::Plain, "5 * 3 and a `tick")]
        );
    }

    #[test]
    fn multibyte_text_survives_parsing() {
        let p = parse_emphasis("**전체 디스크 접근 권한:** *설정 > 일반*", true);
        assert_eq!(
            kinds(&p[0]),
            vec![
                (InfoModalSpanKind::Lead, "전체 디스크 접근 권한:"),
                (InfoModalSpanKind::Plain, " "),
                (InfoModalSpanKind::Path, "설정 > 일반"),
            ]
        );
    }

    #[test]
    fn step_and_aside_markers_pick_the_paragraph_kind() {
        let p = parse_emphasis(
            "1. remove it\n\n12. add `x`\n\n> signed *aside*\n\n1.5 stays text",
            true,
        );
        assert_eq!(p[0].kind, InfoModalParagraphKind::Step("1.".into()));
        assert_eq!(kinds(&p[0]), vec![(InfoModalSpanKind::Plain, "remove it")]);
        assert_eq!(p[1].kind, InfoModalParagraphKind::Step("12.".into()));
        assert_eq!(p[2].kind, InfoModalParagraphKind::Aside);
        assert_eq!(
            kinds(&p[2]),
            vec![
                (InfoModalSpanKind::Plain, "signed "),
                (InfoModalSpanKind::Path, "aside"),
            ]
        );
        assert_eq!(p[3].kind, InfoModalParagraphKind::Text);
    }

    /// 명령은 문단 크기의 code run 이다. caption 크기 칩이 아니다.
    #[test]
    fn commands_are_code_runs_at_the_paragraph_size() {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let code = UiCodeTokens::of(&theme);
        let pad = code.padding_x.value();
        for para in parse_emphasis(
            "Reset with `tccutil reset`, then sign it.\n\n> Signed `x --create`.",
            true,
        ) {
            let (size, _) = paragraph_style(&theme, &para.kind);
            let job = paragraph_job(&theme, &para.kind, &para.spans, f32::INFINITY);
            let command = job
                .sections
                .iter()
                .find(|s| s.format.font_id.family == ui_code_font())
                .expect("command section");
            assert_eq!(command.format.font_id.size, size);
            assert_eq!(command.format.color, code.fg);
            assert_eq!(command.format.background, egui::Color32::TRANSPARENT);
            assert_eq!(command.leading_space, pad);
            let ctx = egui::Context::default();
            drop(ctx.run(Default::default(), |ctx| {
                let galley = ctx.fonts(|f| f.layout_job(job.clone()));
                let rects = ui_code_rects(&theme, &galley);
                assert_eq!(rects.len(), 1);
                let first = galley.rows[0]
                    .glyphs
                    .iter()
                    .find(|g| g.section_index == 1)
                    .expect("command glyph");
                assert_eq!(rects[0].min.x, first.pos.x - pad);
            }));
        }
    }

    #[test]
    fn plain_messages_keep_paragraph_markers() {
        let p = parse_emphasis("1. not a list\n\n> not an aside", false);
        assert!(p.iter().all(|p| p.kind == InfoModalParagraphKind::Text));
        assert_eq!(p[0].spans[0].text, "1. not a list");
    }
}
