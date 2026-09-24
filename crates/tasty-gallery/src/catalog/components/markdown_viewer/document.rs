//! 마크다운의 제목·본문·목록·이미지·코드·표·인용·알림 예제.
//! WebView나 플러그인 렌더러를 실행하지 않고 egui로 근사한다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::checkbox;

use crate::catalog::icons;

use super::{DOC_W, rich};

pub(super) fn document(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::new()
        .fill(theme.bg_app().to_egui()) // crust — the webview render path's only background (no focus signal)
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_lg.value() as i8,
            theme.spacing_md.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width((DOC_W - theme.spacing_lg.scaled(2.0)).value());
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();

            heading(ui, theme, 1, "Markdown surface");
            body_inline(
                ui,
                theme,
                &[
                    Run::text("A read-only viewer that reloads on file change. Inline runs: "),
                    Run::strong("bold"),
                    Run::text(", "),
                    Run::italic("italic"),
                    Run::text(", "),
                    Run::strike("strikethrough"),
                    Run::text(", a "),
                    Run::link("link"),
                    Run::text(", and "),
                    Run::code("inline code"),
                    Run::text("."),
                ],
            );

            heading(ui, theme, 2, "Headings & emphasis");
            body_inline(
                ui,
                theme,
                &[Run::text(
                    "Body uses font-size-body at the 14px cap, line-height 1.6.",
                )],
            );
            heading(ui, theme, 3, "Lists");
            bullet_row(
                ui,
                theme,
                0,
                "•",
                "Bullet item with wrapped text for rhythm.",
            );
            bullet_row(ui, theme, 1, "◦", "Nested bullet");
            bullet_row(ui, theme, 0, "1.", "Ordered item");
            task_row(ui, theme, true, "Task done");
            task_row(ui, theme, false, "Task to do");

            heading(ui, theme, 3, "Code block");
            code_block(ui, theme, &rust_snippet_tokens());

            heading(ui, theme, 3, "Image");
            image_block(ui, theme, "Referenced with a relative path — resolved against the md file's own directory, e.g. ![alt](./screenshot.png).");

            heading(ui, theme, 3, "Table");
            table(ui, theme);

            heading(ui, theme, 3, "Blockquote");
            blockquote(ui, theme);

            heading(ui, theme, 3, "Alerts (GFM)");
            alerts(ui, theme);

            heading(ui, theme, 4, "Subsection (h4)");
            heading(ui, theme, 5, "Minor note (h5)");
            heading(ui, theme, 6, "Label (h6)");

            ui.add_space(theme.spacing_sm.value());
            hr(ui, theme);
            ui.add_space(theme.spacing_xs.value());
            ui.label(rich(
                theme,
                "Horizontal rule above · trailing space below.",
                theme.font_size_caption.value(),
                theme.text_muted().to_egui(),
            ));
        });
}

/// H2 크기 — `render.rs::heading_sizes_px` 가 `font-size-prose-h1`(h1)↔`font-size-body`(h6)
/// 사이를 5단계로 선형보간한다(h1..h6, 5개 구간). h2 는 h1 에서 1구간 내려온 지점이므로
/// 계수는 4/5 = 0.8 — specimen 은 이 계수를 그대로 미러해 H2 크기를 근사한다.
pub(super) fn md_h2_size(theme: &Theme) -> f32 {
    let min = theme.font_size_body.value();
    min + (theme.font_size_prose_h1.value() - min) * 0.8
}

/// heading 한 줄 — level 별 size/color/case (디자인 MD_H 전사).
fn heading(ui: &mut egui::Ui, theme: &Theme, level: u8, text: &str) {
    let body = theme.font_size_body.value();
    let (size, color, upper, top) = match level {
        1 => (
            theme.font_size_prose_h1.value(),
            theme.text_primary().to_egui(),
            false,
            0.0,
        ),
        2 => (
            md_h2_size(theme),
            theme.text_primary().to_egui(),
            false,
            theme.spacing_md.value(),
        ),
        3 => (
            theme.font_size_max.value(),
            theme.text_primary().to_egui(),
            false,
            theme.spacing_sm.value(),
        ),
        4 => (
            body,
            theme.text_secondary().to_egui(),
            false,
            theme.spacing_sm.value(),
        ),
        5 => (
            body,
            theme.text_muted().to_egui(),
            false,
            theme.spacing_xs.value(),
        ),
        _ => (
            body,
            theme.text_muted().to_egui(),
            true,
            theme.spacing_xs.value(),
        ),
    };
    ui.add_space(top);
    let label = if upper {
        text.to_uppercase()
    } else {
        text.to_string()
    };
    ui.label(egui::RichText::new(label).size(size).color(color));
}

/// 한 inline run.
struct Run {
    text: String,
    kind: RunKind,
}
enum RunKind {
    Text,
    Strong,
    Italic,
    Strike,
    Code,
    Link,
}
impl Run {
    fn text(s: &str) -> Self {
        Self {
            text: s.into(),
            kind: RunKind::Text,
        }
    }
    fn strong(s: &str) -> Self {
        Self {
            text: s.into(),
            kind: RunKind::Strong,
        }
    }
    fn italic(s: &str) -> Self {
        Self {
            text: s.into(),
            kind: RunKind::Italic,
        }
    }
    fn strike(s: &str) -> Self {
        Self {
            text: s.into(),
            kind: RunKind::Strike,
        }
    }
    fn code(s: &str) -> Self {
        Self {
            text: s.into(),
            kind: RunKind::Code,
        }
    }
    fn link(s: &str) -> Self {
        Self {
            text: s.into(),
            kind: RunKind::Link,
        }
    }
}

/// 본문 문단 — inline run 들을 wrap 행에 배치(공백은 run 텍스트에 포함).
fn body_inline(ui: &mut egui::Ui, theme: &Theme, runs: &[Run]) {
    let body = theme.font_size_body.value();
    let secondary = theme.text_secondary().to_egui();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, body * 0.6);
        for r in runs {
            let rt = match r.kind {
                RunKind::Text => egui::RichText::new(&r.text).size(body).color(secondary),
                RunKind::Strong => egui::RichText::new(&r.text)
                    .size(body)
                    .color(theme.text_primary().to_egui()),
                RunKind::Italic => egui::RichText::new(&r.text)
                    .size(body)
                    .color(secondary)
                    .italics(),
                RunKind::Strike => egui::RichText::new(&r.text)
                    .size(body)
                    .color(theme.text_muted().to_egui())
                    .strikethrough(),
                RunKind::Code => egui::RichText::new(&r.text)
                    .monospace()
                    .size(body)
                    .color(theme.text_primary().to_egui())
                    .background_color(theme.surface_raised().to_egui()),
                RunKind::Link => egui::RichText::new(&r.text)
                    .size(body)
                    .color(theme.accent_primary().to_egui())
                    .underline(),
            };
            ui.label(rt);
        }
    });
}

/// bullet/ordered 리스트 한 행 — marker(muted) + 본문(secondary).
fn bullet_row(ui: &mut egui::Ui, theme: &Theme, depth: usize, marker: &str, text: &str) {
    let body = theme.font_size_body.value();
    ui.horizontal(|ui| {
        ui.add_space(theme.spacing_lg.value() + depth as f32 * theme.spacing_lg.value());
        ui.label(rich(theme, marker, body, theme.text_muted().to_egui()));
        ui.add_space(theme.spacing_xs.value());
        ui.label(rich(theme, text, body, theme.text_secondary().to_egui()));
    });
}

/// task 리스트 한 행 — 16px checkbox + 본문.
fn task_row(ui: &mut egui::Ui, theme: &Theme, mut done: bool, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(theme.spacing_lg.value());
        checkbox(ui, theme, &mut done, text, false);
    });
}

/// 이미지 파일을 읽지 않고 대체 영역과 alt 설명을 그린다.
fn image_block(ui: &mut egui::Ui, theme: &Theme, alt: &str) {
    let (w, h) = (200.0, 120.0);
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "image",
                egui::FontId::monospace(theme.font_size_caption.value()),
                theme.text_muted().to_egui(),
            );
        });
    ui.add_space(theme.spacing_xs.value());
    ui.label(rich(
        theme,
        alt,
        theme.font_size_caption.value(),
        theme.text_muted().to_egui(),
    ));
}

/// 플러그인 hljs_css와 같은 색을 쓰는 구문 종류. 실제 highlight.js는 실행하지 않는다.
#[derive(Clone, Copy)]
enum TokenKind {
    Plain,
    Keyword,
    Title,
    Builtin,
    String,
}

/// One token's text + [`TokenKind`] — a code-block line is a `Vec<CodeToken>`.
struct CodeToken(&'static str, TokenKind);

/// 미리 나눈 코드 토큰 예제. 실제 구문 분석은 하지 않는다.
fn rust_snippet_tokens() -> [Vec<CodeToken>; 3] {
    [
        vec![
            CodeToken("fn ", TokenKind::Keyword),
            CodeToken("main", TokenKind::Title),
            CodeToken("() {", TokenKind::Plain),
        ],
        vec![
            CodeToken("    ", TokenKind::Plain),
            CodeToken("format!", TokenKind::Builtin),
            CodeToken("(", TokenKind::Plain),
            CodeToken("\"hi from tasty\"", TokenKind::String),
            CodeToken(");", TokenKind::Plain),
        ],
        vec![CodeToken("}", TokenKind::Plain)],
    ]
}

/// 토큰별 색을 적용해 구문 강조 결과를 근사한다.
fn code_block(ui: &mut egui::Ui, theme: &Theme, lines: &[Vec<CodeToken>]) {
    let body = theme.font_size_body.value();
    let token_color = |kind: TokenKind| match kind {
        TokenKind::Plain => theme.text_secondary().to_egui(),
        TokenKind::Keyword => theme.mauve.to_egui(),
        TokenKind::Title => theme.blue.to_egui(),
        TokenKind::Builtin => theme.red.to_egui(),
        TokenKind::String => theme.green.to_egui(),
    };
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - theme.spacing_sm.value() * 2.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for line in lines {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for CodeToken(text, kind) in line {
                        ui.label(
                            egui::RichText::new(*text)
                                .monospace()
                                .size(body)
                                .color(token_color(*kind)),
                        );
                    }
                });
            }
        });
}

/// grid + zebra 테이블 (md-table 토큰) — header(surface-raised) 밴드 + base/zebra 본문
/// + 외곽·가로·세로 격자선(border-strong). 값 사다리 mantle<base<surface0<surface1.
fn table(ui: &mut egui::Ui, theme: &Theme) {
    let body = theme.font_size_body.value();
    let pad_x = theme.md_table_cell_padding_x().value();
    let pad_y = theme.md_table_cell_padding_y().value();
    // 세로 격자선 양쪽에 셀 패딩(pad_x)이 대칭으로 오도록 컬럼 간격을 2*pad_x 로.
    let col_gap = pad_x * 2.0;
    let border_w = theme.border_width.value();
    let border = theme.md_table_border().to_egui();
    let r = theme.corner_radius.value() as u8;
    let margin = egui::Margin::symmetric(pad_x as i8, pad_y as i8);
    let cols = 3usize;

    let cell = |ui: &mut egui::Ui, i: usize, text: &str, color: egui::Color32| {
        let lay = if i == 2 {
            egui::Layout::right_to_left(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Min)
        };
        ui.with_layout(lay, |ui| {
            let rt = if i == 2 {
                egui::RichText::new(text)
                    .monospace()
                    .size(body)
                    .color(color)
            } else {
                egui::RichText::new(text).size(body).color(color)
            };
            ui.label(rt);
        });
    };

    let row = |ui: &mut egui::Ui, cells: [&str; 3], header: bool| {
        let color = if header {
            theme.md_table_header_fg().to_egui()
        } else {
            theme.md_table_cell_fg().to_egui()
        };
        ui.spacing_mut().item_spacing.x = col_gap;
        // 동적폭 방어: 잔여폭이 컬럼 간격 합 미만이면 columns 내부 폭이 음수가 되어 panic.
        if ui.available_width() > col_gap * (cols as f32 - 1.0) + 1.0 {
            ui.columns(cols, |c| {
                for (i, t) in cells.iter().enumerate() {
                    cell(&mut c[i], i, t, color);
                }
            });
        } else {
            ui.vertical(|ui| {
                for (i, t) in cells.iter().enumerate() {
                    cell(ui, i, t, color);
                }
            });
        }
    };

    let out = egui::Frame::new()
        .fill(theme.md_table_row_bg().to_egui()) // 불투명 base 채움 → 배경 무관 패널로 읽힘.
        .stroke(egui::Stroke::new(border_w, border))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new()
                .fill(theme.md_table_header_bg().to_egui())
                .corner_radius(egui::CornerRadius {
                    nw: r,
                    ne: r,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(margin)
                .show(ui, |ui| row(ui, ["Resource", "Kind", "Count"], true));
            table_divider(ui, theme);
            egui::Frame::new()
                .inner_margin(margin)
                .show(ui, |ui| row(ui, ["surface", "viewer", "12"], false));
            table_divider(ui, theme);
            egui::Frame::new()
                .fill(theme.md_table_row_bg_zebra().to_egui())
                .corner_radius(egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: r,
                    se: r,
                })
                .inner_margin(margin)
                .show(ui, |ui| row(ui, ["popup", "overlay", "8"], false));
        });

    // egui columns가 세로선을 그리지 않으므로 열 경계만 직접 그린다.
    let content = out.response.rect;
    let inner_w = content.width() - col_gap;
    if inner_w > col_gap * (cols as f32 - 1.0) {
        let col_w = (inner_w - col_gap * (cols as f32 - 1.0)) / cols as f32;
        let inner_left = content.left() + pad_x;
        let painter = ui.painter();
        for i in 1..cols {
            let x = inner_left + i as f32 * col_w + (2 * i - 1) as f32 * pad_x;
            painter.vline(x, content.y_range(), egui::Stroke::new(border_w, border));
        }
    }
}

fn table_divider(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.md_table_border().to_egui(),
        ),
    );
}

/// nested blockquote — left bar(border-strong) + muted 본문, 1단계 중첩.
fn blockquote(ui: &mut egui::Ui, theme: &Theme) {
    let body = theme.font_size_body.value();
    let bar_w = theme.tab_indicator_width.value();
    let gap = theme.spacing_md.value();
    quote_block(ui, theme, bar_w, gap, |ui| {
        ui.label(rich(
            theme,
            "Quoted text reads one tone down (muted) with a left bar.",
            body,
            theme.text_muted().to_egui(),
        ));
        ui.add_space(theme.spacing_xs.value());
        quote_block(ui, theme, bar_w, gap, |ui| {
            ui.label(rich(
                theme,
                "Nested quote, one level deeper.",
                body,
                theme.text_muted().to_egui(),
            ));
        });
    });
}

/// left bar + 들여쓴 content. content 를 자식 ui 로 측정한 뒤 바를 그 높이만큼 칠한다.
fn quote_block(
    ui: &mut egui::Ui,
    theme: &Theme,
    bar_w: f32,
    gap: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let top = ui.cursor().min.y;
    let left = ui.min_rect().left();
    let avail = ui.available_width();
    let content_x = left + bar_w + gap;
    let content_w = (avail - bar_w - gap).max(1.0);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                egui::pos2(content_x, top),
                egui::vec2(content_w, f32::INFINITY),
            ))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    add(&mut child);
    let bottom = child.min_rect().bottom();
    ui.allocate_rect(
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + avail, bottom)),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + bar_w, bottom)),
        0.0,
        theme.border_strong().to_egui(),
    );
}

/// GFM 알림의 아이콘·색·라벨·본문. 브라우저의 왼쪽 선은 egui Frame의 네 면 보더로 근사한다.
type AlertSpec = (
    icons::MockGlyph,
    fn(&Theme) -> tasty_type_appearance::color::HexColor,
    &'static str,
    &'static str,
);

fn alerts(ui: &mut egui::Ui, theme: &Theme) {
    let items: [AlertSpec; 5] = [
        (
            icons::ALERT_CIRCLE,
            Theme::accent_primary,
            "Note",
            "Highlights information users should take into account, even when skimming.",
        ),
        (
            icons::STAR_FILL,
            Theme::accent_success,
            "Tip",
            "Optional information to help a user be more successful.",
        ),
        (
            icons::BELL,
            Theme::accent_agent,
            "Important",
            "Crucial information necessary for users to succeed.",
        ),
        (
            icons::ALERT_TRIANGLE,
            Theme::accent_warning,
            "Warning",
            "Critical content demanding immediate user attention due to possible risks.",
        ),
        (
            icons::CLOSE,
            Theme::accent_danger,
            "Caution",
            "Negative potential consequences of an action.",
        ),
    ];
    for (icon, accent, label, body) in items {
        alert_box(ui, theme, icon, accent(theme), label, body);
        ui.add_space(theme.spacing_xs.value());
    }
}

/// accent 12% 배경(`render.rs::alert_css`의 `BG_ALPHA = 31` 과 동일 비율) + accent 보더 +
/// 아이콘(`tasty_icons`, accent tint)+굵은 label 헤더 + muted 본문.
fn alert_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: icons::MockGlyph,
    color: tasty_type_appearance::color::HexColor,
    label: &str,
    body: &str,
) {
    // callout 배경 — accent 저알파. 대응 토큰 없음.
    const CALLOUT_BG_ALPHA: u8 = 31;
    egui::Frame::new()
        .fill(color.with_alpha(CALLOUT_BG_ALPHA).to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            color.to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let sz = theme.font_size_body.value();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                icon.image(sz, color.to_egui()).paint_at(ui, rect);
                ui.label(
                    rich(theme, label, theme.font_size_body.value(), color.to_egui()).strong(),
                );
            });
            ui.add_space(theme.spacing_xs.value() * 0.5);
            ui.label(rich(
                theme,
                body,
                theme.font_size_body.value(),
                theme.text_secondary().to_egui(),
            ));
        });
}

fn hr(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}
