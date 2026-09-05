//! `markdown_viewer` specimen — Markdown surface 문서 디자인의 egui 근사 (Layouts).
//!
//! 본체 렌더 경로(`docs/plugins/markdown/screens/markdown.md`, [ADR-0065](../../../../../docs/adr/0065-markdown-webview-render-channel.md)
//! 참고): `crates/tasty-plugin-markdown` 은 Stage B 부터 **native OS webview**(host 가 만드는
//! overlay — WebKitGTK/WKWebView/WebView2)에 sanitize 된 HTML 문서를 올려 그린다. plugin 이
//! `pulldown-cmark` 로 HTML 을 생성하고, Theme 에서 캡처한 색·크기·간격을 CSS custom property
//! 로 주입한 `<style>` 을 문서에 인라인한다(`render.rs::theme_css`) — egui 렌더 채널이 아니므로
//! host 가 pixel 단위로 관여하지 않는다.
//!
//! **CSS-driven 주석 (이 specimen 은 CSS 출력의 근사):** 갤러리는 실제 webview 를 띄우지
//! 않으므로(egui 카탈로그), 같은 토큰·계층을 **손으로 전사**한 것이다 — 실제 브라우저 렌더와
//! 픽셀 동일성은 비목표. CSS 채널이므로 egui_commonmark 시절과 달리 아래는 더 이상 "라이브러리
//! 제약"이 아니라 **plugin 이 스스로 결정한 디자인**이다:
//! - **heading 사다리**: `render.rs::heading_sizes_px` 가 `font-size-prose-h1`(h1)↔
//!   `font-size-body`(h6) 사이를 5단계 선형보간한다 — CSS 라 per-level 픽셀을 자유롭게 override
//!   할 수 있지만, 현재는 이 선형보간을 디자인으로 채택했다(egui_commonmark 시절의 시각적
//!   사다리를 유지). h2 근사 계수(0.835)는 이 선형보간의 근사치.
//! - **본문 leading** 은 CSS `line-height` 로 완전히 제어 가능(라이브러리 제약 없음).
//! - **표**는 실제 `<table>` — header 밴드(`md-table-header-bg`/`-fg`)·zebra·8/4px 셀 패딩
//!   전부 CSS 로 직접 달성한다(egui `Grid::striped` 우회가 더 이상 필요 없음). 이 specimen 은
//!   egui `Frame`+수동 grid 로 같은 시각 결과를 손으로 흉내낸다.
//! - inline bold 는 CSS `font-weight` 로 실제 굵기 차이가 난다(specimen 은 egui 에 합성
//!   weight 가 없어 text-primary 승격으로만 신호).
//!
//! **주소창**: 더 이상 공유 `PathField` egui 위젯이 아니다 — HTML 문서 자체에 내장된
//! `<input>`+`<button>`(`render.rs::addr_bar_html`/`nav_script`)이라 host egui 컴포넌트가
//! 아니게 됐다. 이 specimen 은 그 HTML chrome 의 정적 근사만 그린다(라이브 PathField 소비 아님).
//!
//! **인라인 이미지** (`![alt](path)`) — 이제 `<base href="file:///…">` 로 상대경로를 앵커한
//! 평범한 `<img>` 태그다(`render.rs::file_dir_uri`). 이 specimen 은 파일 I/O 없이 placeholder
//! rect 로 근사한다 — 아래 `image_block`.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Spinner, checkbox};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 문서 카드 폭(전시 박스).
const DOC_W: LogicalPx = LogicalPx(560.0);
/// 상태 타일 치수.
const TILE_W: LogicalPx = LogicalPx(200.0);
const TILE_H: LogicalPx = LogicalPx(132.0);

/// 주소창 바 폭(HTML chrome 정적 근사 — `render.rs::addr_bar_html` 의 디자인 폭).
const ADDR_BAR_W: LogicalPx = LogicalPx(360.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // 0. 상단 주소창 chrome — 더 이상 host egui 위젯(PathField)이 아니라 문서 HTML 에 내장된
    //    `<input>`+`<button>`(`render.rs::addr_bar_html`) 이다. host 컴포넌트가 아니게 됐으므로
    //    라이브 편집 상태(idle/editing)를 소비하지 않고, 그 HTML chrome 의 정적 근사만 그린다.
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        spec::cluster(
            ui,
            theme,
            "address bar — HTML chrome (in-document)",
            |ui| {
                address_bar(ui, theme);
            },
        );
        spec::cluster(
            ui,
            theme,
            "table of contents — collapsible, in-document (todo42)",
            |ui| {
                toc_chrome(ui, theme);
            },
        );
    });

    // 1. 전체 element catalog 문서.
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.set_max_width(DOC_W.value());
        document(ui, theme);
    });

    // 2. heading 계층 type-scale 시트 (h1–h6 + p + small).
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        type_scale(ui, theme);
    });

    // 3. 상태 chrome (load-fail / empty / loading).
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "load failed", |ui| {
            tile(ui, theme, |ui| {
                ui.label(rich(
                    theme,
                    "Failed to load",
                    theme.font_size_max.value(),
                    theme.accent_danger().to_egui(),
                ));
                ui.label(
                    egui::RichText::new("notes.md: No such file")
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        });
        spec::cluster(ui, theme, "empty file", |ui| {
            tile(ui, theme, |ui| {
                ui.label(rich(
                    theme,
                    "This file is empty",
                    theme.font_size_body.value(),
                    theme.text_muted().to_egui(),
                ));
            });
        });
        spec::cluster(ui, theme, "loading", |ui| {
            tile(ui, theme, |ui| {
                Spinner::new()
                    .size(theme.spinner_size.value())
                    .show(ui, theme);
                ui.add_space(theme.spacing_sm.value());
                ui.label(rich(
                    theme,
                    "Loading…",
                    theme.font_size_body.value(),
                    theme.text_muted().to_egui(),
                ));
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("addr bar", "40px · bg-sidebar · in-document HTML chrome"),
            (
                "addr field",
                "in-document <input>+<button> · nav-fragment scheme",
            ),
            (
                "body",
                "13 · text-secondary · CSS line-height (full control)",
            ),
            ("h1", "Heading anchor prose-h1 20 · text-primary"),
            ("h2–h6", "CSS-interpolated 20→13 · strong"),
            (
                "heading id",
                "GitHub-compatible auto slug — no explicit {#id} syntax",
            ),
            (
                "toc",
                "collapsible <nav> · surface-raised · indent = space-sm × level",
            ),
            ("code", "mono · surface-raised · language class preserved"),
            (
                "syntax highlighting",
                "client-side highlight.js (offline vendor) · hljs-* token colors from Theme hues",
            ),
            ("link", "accent-primary · nav-fragment intercepted"),
            ("table", "real <table> — header band + zebra + padding"),
            ("states", "failed=accent-danger · empty=muted"),
            (
                "alerts",
                "5× GFM `[!NOTE]`.. — accent 12% bg + border + icon/label header",
            ),
        ],
        &[
            TokenChip::new("bg-panel", "surface", theme.bg_panel().to_egui()),
            TokenChip::new("bg-app", "document bg (crust)", theme.bg_app().to_egui()),
            TokenChip::new(
                "text-primary",
                "h1–h3 · bold",
                theme.text_primary().to_egui(),
            ),
            TokenChip::new(
                "text-secondary",
                "body · h4",
                theme.text_secondary().to_egui(),
            ),
            TokenChip::new(
                "text-muted",
                "h5/h6 · caption",
                theme.text_muted().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "link · note alert",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "code bg",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new("separator", "hr", theme.separator.to_egui()),
            TokenChip::new(
                "border-strong",
                "blockquote · table grid",
                theme.border_strong().to_egui(),
            ),
            TokenChip::new(
                "md-table-header-bg",
                "table header",
                theme.md_table_header_bg().to_egui(),
            ),
            TokenChip::new(
                "md-table-zebra",
                "table even row",
                theme.md_table_row_bg_zebra().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "load failed · caution alert",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "accent-success",
                "tip alert",
                theme.accent_success().to_egui(),
            ),
            TokenChip::new(
                "accent-warning",
                "warning alert",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "accent-agent",
                "important alert",
                theme.accent_agent().to_egui(),
            ),
            TokenChip::new("mauve", "hljs-keyword", theme.mauve.to_egui()),
            TokenChip::new("blue", "hljs-title/function", theme.blue.to_egui()),
            TokenChip::new("green", "hljs-string", theme.green.to_egui()),
            TokenChip::new("red", "hljs-built_in", theme.red.to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "A read-only Markdown surface — the plugin renders sanitized HTML in a native OS \
         webview (Stage B, ADR-0065), injecting Theme tokens as CSS custom properties so the \
         colors and type scale follow the design. The heading ladder is a 5-step linear \
         interpolation between the Heading anchor (prose-h1 20) and Body (13), fully \
         controlled by the plugin's own CSS generator (no library constraint left) — h2/h3 \
         still read alike by design choice, not limitation. GFM alert blockquotes \
         (`> [!NOTE]`/`TIP`/`IMPORTANT`/`WARNING`/`CAUTION`) each get their own accent \
         color, icon, and localized header label baked into the document at generation time — \
         CSS alone can't branch on the UI language, so `render.rs` resolves the label via its \
         own `Translator` and carries it across the sanitize boundary as a `data-label` \
         attribute the CSS then echoes back with `content: attr(data-label)`. This specimen \
         hand-transcribes the same tokens as an approximation of that CSS output (the gallery \
         does not embed a live webview). Every heading also gets a GitHub-compatible auto slug \
         `id` (`render.rs::collect_headings`/`Slugger` — lowercase, Unicode-aware, deduped with \
         `-1`/`-2` suffixes, no explicit `{#id}` syntax) and a document-top collapsible `<nav>` \
         TOC is generated from them (`render.rs::toc_nav_html`) — clicking an entry is a plain \
         same-document anchor jump, reusing the existing anchor-only pass-through rather than \
         the `#tasty-nav:` interception scheme. Fenced code blocks get client-side syntax \
         highlighting from a vendored, offline highlight.js bundle (`render.rs::highlight_script` \
         — inserted only when the document actually has a code block, same conditional pattern as \
         the mermaid bundle), with `hljs-*` token colors mapped to this plugin's own Catppuccin- \
         style `Theme` hues (`render.rs::hljs_css`) instead of a fixed vendored theme, so \
         highlighted code follows the active tasty theme. This specimen's code block hand-tokenizes \
         the same sample the way highlight.js's `rust` grammar would, to approximate that CSS \
         output. Below the document: the heading type-scale, and the load-fail / empty / loading \
         chrome that replaces a raw `Error:` body.",
    );
}

/// 대표 마크다운 문서 — 6단계 heading + inline runs + 리스트 3종 + table + nested
/// blockquote + code + hr.
fn document(ui: &mut egui::Ui, theme: &Theme) {
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
            // bullet · nested bullet · ordered · task.
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

            // h4/h5/h6 — 작은 계층(secondary → muted → UPPER) 노출.
            heading(ui, theme, 4, "Subsection (h4)");
            heading(ui, theme, 5, "Minor note (h5)");
            heading(ui, theme, 6, "Label (h6)");

            // horizontal rule.
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
fn md_h2_size(theme: &Theme) -> f32 {
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
        // checkbox 는 라벨까지 그려 주므로 라벨을 직접 넘긴다.
        checkbox(ui, theme, &mut done, text, false);
    });
}

/// 인라인 이미지 — webview 가 실제로 그리는 raster 를 손으로 근사한다(갤러리는 파일
/// I/O 도, live webview 도 갖지 않는다). 실제 로드 경로: 문서 `<head>` 의
/// `<base href="file:///…/">`(`render.rs::file_dir_uri`)가 상대경로 dest 를 앵커하고,
/// 나머지는 평범한 `<img src>` 로 브라우저 엔진이 직접 로드한다(host 관여 없음). alt 텍스트는
/// 표준 `<img alt>` 로 스크린리더/로드실패 fallback 에 쓰이지만, 정적 specimen 에서는 항상
/// 보이는 캡션으로 대신 노출한다.
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

/// One highlight.js token scope, hand-mapped to the same `Theme` hue `render.rs::hljs_css` uses
/// (this specimen mirrors that mapping by hand — it doesn't consume it live, the gallery has no
/// webview to run highlight.js in).
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

/// `fn main() { format!("hi from tasty"); }` tokenized the way highlight.js's `rust` grammar
/// would split it, each token tagged with the `hljs-*` scope `render.rs::hljs_css` colors it by:
/// `fn` = keyword (mauve), `main` = title/function (blue), `format!` = built_in (red),
/// the string literal = string (green), everything else = plain (text-secondary, unchanged).
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

/// Code block — each line rendered as colored `CodeToken` runs, approximating highlight.js's
/// `<span class="hljs-*">` output (`render.rs::highlight_script`) painted through
/// `render.rs::hljs_css`'s Theme-derived token colors, rather than the single flat-color mono
/// block this specimen used before syntax highlighting existed.
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

    // 셀 렌더 — index 2(Count)는 mono + 우측정렬 숫자열(디자인 §2-3 num).
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

    // 헤더 신호는 색+배경(text-primary), 본문은 text-secondary.
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
            // 헤더 밴드 (상단 2모서리 라운드).
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
            // 첫 본문행 = base(줄무늬 없음).
            egui::Frame::new()
                .inner_margin(margin)
                .show(ui, |ui| row(ui, ["surface", "viewer", "12"], false));
            table_divider(ui, theme);
            // 짝수 본문행(2행째) = zebra(mantle) + 마지막이므로 하단 2모서리 라운드.
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

    // 세로 컬럼 격자선 — egui columns 는 세로선을 그리지 않으므로 표 전체 높이에 걸쳐
    // 컬럼 경계마다 수동 draw. 마지막 열 오른쪽 선은 외곽과 겹치므로 생략(i in 1..cols).
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
    // 좌측 강조 바 폭 — spec.rs accent_bar 좌측 바와 동일하게 tab_indicator_width(2px) 토큰.
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

/// GitHub 스타일 alert blockquote(`> [!NOTE]` 등) 5종 — `render.rs::ALERT_KINDS`(icon/accent/
/// label 매핑) 과 `render.rs::alert_css`(배경 12% tint 유도)의 손 근사. 실제 CSS 출력은
/// 좌측 accent 바만 쓰지만(다른 blockquote 와 동일 `border-left`), 이 specimen 은 egui
/// `Frame` 의 표준 paint-order 이점(배경이 자식 콘텐츠보다 먼저 그려짐이 보장됨)을 살리려
/// 4변 보더로 근사한다 — 픽셀 동일성은 애초에 이 파일의 비목표(모듈 doc 참고).
/// (glyph, accent accessor, label, body) — one [`alerts`] row.
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

/// heading type-scale 시트 — h1–h6 + p + small, 좌측 mono 태그.
fn type_scale(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
    let scale_row = |ui: &mut egui::Ui, tag: &str, draw: &dyn Fn(&mut egui::Ui)| {
        ui.horizontal(|ui| {
            ui.add_space(theme.spacing_xs.value());
            let (r, _) = ui.allocate_exact_size(
                egui::vec2(theme.spacing_xl.value(), theme.font_size_body.value()),
                egui::Sense::hover(),
            );
            ui.painter().text(
                r.left_center(),
                egui::Align2::LEFT_CENTER,
                tag,
                egui::FontId::monospace(theme.font_size_micro.value()),
                theme.text_muted().to_egui(),
            );
            ui.add_space(theme.spacing_md.value());
            draw(ui);
        });
    };
    for lvl in 1..=6u8 {
        scale_row(ui, &format!("h{lvl}"), &move |ui| {
            heading_sample(ui, theme, lvl)
        });
    }
    scale_row(ui, "p", &|ui| {
        ui.label(rich(
            theme,
            "Body — 13px, line-height 1.6, secondary.",
            theme.font_size_body.value(),
            theme.text_secondary().to_egui(),
        ));
    });
    scale_row(ui, "small", &|ui| {
        ui.label(rich(
            theme,
            "Caption — body × 0.85, muted.",
            theme.font_size_caption.value(),
            theme.text_muted().to_egui(),
        ));
    });
}

/// type-scale 한 줄용 heading 샘플(상단 마진 없이).
fn heading_sample(ui: &mut egui::Ui, theme: &Theme, level: u8) {
    let body = theme.font_size_body.value();
    let (size, color, upper) = match level {
        1 => (
            theme.font_size_prose_h1.value(),
            theme.text_primary().to_egui(),
            false,
        ),
        2 => (md_h2_size(theme), theme.text_primary().to_egui(), false),
        3 => (
            theme.font_size_max.value(),
            theme.text_primary().to_egui(),
            false,
        ),
        4 => (body, theme.text_secondary().to_egui(), false),
        5 => (body, theme.text_muted().to_egui(), false),
        _ => (body, theme.text_muted().to_egui(), true),
    };
    let text = if upper {
        "The quick brown fox".to_uppercase()
    } else {
        "The quick brown fox".to_string()
    };
    ui.label(egui::RichText::new(text).size(size).color(color));
}

/// 상태 타일 — 고정 W×H 테두리 박스, 콘텐츠 세로 가운데.
fn tile(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(TILE_W.value(), TILE_H.value()),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.add_space(theme.spacing_xl.value());
                    add(ui);
                },
            );
        });
}

/// 상단 주소창 chrome — Stage B 부터 host egui 위젯이 아니라 문서 HTML 에 내장된
/// `<input>`+`<button>`(`render.rs::addr_bar_html`)이라, 더 이상 공유 `PathField` 를
/// 라이브 소비하지 않는다(그 위젯 자체를 markdown 이 안 쓴다). 이 specimen 은 그 HTML
/// chrome 을 정적으로 근사한다 — file 아이콘 + 경로 텍스트 + Go 아이콘, idle 상태 하나만.
fn address_bar(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::new()
        .fill(theme.bg_sidebar().to_egui())
        .inner_margin(egui::Margin::symmetric(theme.spacing_sm.value() as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ADDR_BAR_W.value());
            ui.horizontal_centered(|ui| {
                egui::Frame::new()
                    .fill(theme.surface_raised().to_egui())
                    .stroke(egui::Stroke::new(
                        theme.border_width.value(),
                        theme.border_default().to_egui(),
                    ))
                    .corner_radius(theme.corner_radius.value())
                    .inner_margin(egui::Margin::symmetric(theme.spacing_sm.value() as i8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let sz = theme.font_size_caption.value();
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                            icons::FILE
                                .image(sz, theme.text_muted().to_egui())
                                .paint_at(ui, rect);
                            ui.add_space(theme.spacing_xs.value());
                            ui.label(rich(
                                theme,
                                "/docs/readme.md",
                                sz,
                                theme.text_secondary().to_egui(),
                            ));
                        });
                    });
                ui.add_space(theme.spacing_xs.value());
                let sz = theme.font_size_caption.value();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(sz, sz), egui::Sense::hover());
                icons::ARROW_RIGHT
                    .image(sz, theme.text_muted().to_egui())
                    .paint_at(ui, rect);
            });
        });
}

/// TOC chrome (`render.rs::toc_nav_html`) — collapsible `<nav>` auto-inserted between the address
/// bar and the document body, from headings auto-slugged in `render.rs::collect_headings`(no
/// explicit `{#id}` syntax — module design decision: auto slugs only). Like `address_bar` this
/// is an in-document HTML element, not a host egui widget, so this specimen is a static
/// always-expanded approximation (no live collapse/click-scroll state in the gallery) — indent
/// per level mirrors `theme_css`'s `.tasty-toc-l<N>` rules (`--md-space-sm` × depth). The item
/// list below mirrors [`document`]'s own headings.
fn toc_chrome(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width((DOC_W - theme.spacing_lg.scaled(2.0)).value());
            ui.horizontal(|ui| {
                ui.label(rich(
                    theme,
                    "\u{25be}",
                    theme.font_size_body.value(),
                    theme.text_primary().to_egui(),
                ));
                ui.add_space(theme.spacing_xs.value());
                ui.label(
                    rich(
                        theme,
                        "Table of contents",
                        theme.font_size_body.value(),
                        theme.text_primary().to_egui(),
                    )
                    .strong(),
                );
            });
            ui.add_space(theme.spacing_xs.value());
            for (level, label) in [
                (1u8, "Markdown surface"),
                (2, "Headings & emphasis"),
                (3, "Lists"),
                (3, "Code block"),
                (3, "Image"),
                (3, "Table"),
                (3, "Blockquote"),
                (3, "Alerts (GFM)"),
                (4, "Subsection (h4)"),
            ] {
                toc_row(ui, theme, level, label);
            }
        });
}

/// One TOC entry — indent grows by `--md-space-sm` per level below h1 (`theme_css`'s
/// `.tasty-toc-l<N>` ladder), link-colored label (an in-document `<a href="#slug">`).
fn toc_row(ui: &mut egui::Ui, theme: &Theme, level: u8, label: &str) {
    ui.horizontal(|ui| {
        ui.add_space(level.saturating_sub(1) as f32 * theme.spacing_sm.value());
        ui.label(rich(
            theme,
            label,
            theme.font_size_body.value(),
            theme.accent_primary().to_egui(),
        ));
    });
}

/// 지정 size/color 라벨 텍스트.
fn rich(_theme: &Theme, text: &str, size: f32, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(color)
}
