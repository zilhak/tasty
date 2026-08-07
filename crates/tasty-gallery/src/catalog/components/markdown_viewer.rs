//! `markdown_viewer` specimen — Markdown surface 의 host egui 패널 (Layouts).
//!
//! 본체 렌더 경로(`docs/plugins/markdown/screens/markdown.md` 참고): `crates/tasty-plugin-markdown` 이 **`egui_commonmark`
//! 라이브러리**로 마크다운을 그린다(hand-rolled 렌더러 은퇴). 색은 전부 `egui::Visuals` 에서
//! 읽으므로 plugin 이 Theme 시맨틱 토큰을 `Visuals`/text-style 에 주입해 디자인 토큰이 출력을
//! 몬다. toolbar·헤더 없이 surface 타일 전체를 본문이 채운다.
//!
//! **library-driven 주석 (이 specimen 은 라이브러리 출력의 근사):** 아래 카탈로그는 갤러리가
//! `egui_commonmark`/plugin 에 의존하지 않으므로 같은 토큰·계층을 **손으로 전사**한 것이다 —
//! 라이브러리 실제 출력과 픽셀 동일성은 비목표. 라이브러리가 확정한 디자인 예외(정본
//! `tokens/semantic.css:137-138,152`):
//! - **heading 사다리는 라이브러리가 보간**한다 — `Heading`(prose-h1 20 앵커)↔`Body`(13) 사이를
//!   H2..H6 이 자동 보간(per-H2 픽셀 지정 불가, prose-h2 토큰 제거됨). h2·h3 이 시각적으로
//!   겹치는 것은 이 보간의 결과다.
//! - **본문 leading override 불가**(line-height-prose 토큰 제거됨 — 라이브러리 소유).
//! - **표**는 `Frame::group`+`Grid::striped` 로 그려 grid border(md-table-border, 불투명)·zebra
//!   (md-table-row-bg-zebra)·cell fg(md-table-cell-fg)만 노출 — header 밴드/불투명 base fill/
//!   per-cell 8·4px 패딩은 라이브러리 Grid 로 도달 불가(heading 보간과 동류의 라이브러리 제약).
//! - inline bold 는 text-primary(strong) 승격으로 신호(egui 합성 weight 없음).
//!
//! **인라인 이미지** (`![alt](path)`) — `egui_commonmark` 의 `load-images` feature +
//! `render.rs` 의 base_dir 앵커로 상대경로 이미지를 실제로 로드해 그린다
//! (`docs/plugins/markdown/index.md` "인라인 이미지" 절). 이 specimen 은 파일 I/O 없이
//! placeholder rect 로 근사한다 — 아래 `image_block`.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{PathField, Spinner, checkbox};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 문서 카드 폭(전시 박스).
const DOC_W: f32 = 560.0;
/// 상태 타일 치수.
const TILE_W: f32 = 200.0;
const TILE_H: f32 = 132.0;

/// 주소창 바 폭 (필드/Go 높이는 공용 `PathField` 소유 — 디자인 40 바).
const ADDR_BAR_W: f32 = 360.0;

/// 최근 파일 후보(markdown 주소창 데모 — 플러그인 `recent.query` 응답의 근사).
const MD_RECENT: &[&str] = &[
    "/docs/readme.md",
    "/docs/guide.md",
    "/docs/architecture.md",
    "~/work/tasty/CHANGELOG.md",
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // 0. 상단 주소창 chrome (03) — 공용 [`PathField`] 라이브 소비(markdown 컨텍스트: file
    //    아이콘 + 최근파일 드롭다운). 비포커스=idle(경로 secondary), 클릭=editing(primary +
    //    후보 드롭다운 + 키내비/이동/원복). idle/editing×explorer/markdown 전 매트릭스는
    //    `prim_path_field` specimen 이 정적+라이브로 전시 — 여기선 markdown surface 컨텍스트만.
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        spec::cluster(
            ui,
            theme,
            "address bar — shared PathField (click to edit)",
            |ui| {
                address_bar(ui, theme);
            },
        );
    });

    // 1. 전체 element catalog 문서.
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.set_max_width(DOC_W);
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
            ("addr bar", "40px · bg-sidebar · shared PathField (03)"),
            (
                "addr field",
                "AutoComplete trigger + Go · idle secondary / edit primary",
            ),
            ("body", "13 · text-secondary · leading=library-owned"),
            ("h1", "Heading anchor prose-h1 20 · text-primary"),
            ("h2–h6", "library-interpolated 20→13 · strong"),
            ("code", "mono · surface-raised · egui_extras highlight"),
            ("link", "accent-primary · host-routed"),
            ("table", "grid border + zebra (header band n/a)"),
            ("states", "failed=accent-danger · empty=muted"),
        ],
        &[
            TokenChip::new("bg-panel", "surface", theme.bg_panel().to_egui()),
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
            TokenChip::new("accent-primary", "link", theme.accent_primary().to_egui()),
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
                "load failed",
                theme.accent_danger().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "A read-only Markdown surface — the plugin renders with egui_commonmark, injecting \
         Theme tokens into egui Visuals so the colors follow the design. The heading ladder \
         is interpolated by the library between the Heading anchor (prose-h1 20) and Body \
         (13); per-level pixel sizes and body leading are library-owned (a confirmed design \
         exception), and h2/h3 read alike. This specimen hand-transcribes the same tokens as \
         an approximation of that output. Below the document: the heading type-scale, and the \
         load-fail / empty / loading chrome that replaces a raw `Error:` body.",
    );
}

/// 대표 마크다운 문서 — 6단계 heading + inline runs + 리스트 3종 + table + nested
/// blockquote + code + hr.
fn document(ui: &mut egui::Ui, theme: &Theme) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
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
            ui.set_width(DOC_W - theme.spacing_lg.value() * 2.0);
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
            code_block(
                ui,
                theme,
                "fn main() {\n    println!(\"hi from tasty\");\n}",
            );

            heading(ui, theme, 3, "Image");
            image_block(ui, theme, "Referenced with a relative path — resolved against the md file's own directory, e.g. ![alt](./screenshot.png).");

            heading(ui, theme, 3, "Table");
            table(ui, theme);

            heading(ui, theme, 3, "Blockquote");
            blockquote(ui, theme);

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

/// H2 근사 크기 — egui_commonmark 은 헤딩 사다리를 `Heading` 앵커(`prose-h1`)와 `Body`
/// 사이에서 보간하며 per-H2 픽셀 토큰(`prose-h2`)은 은퇴했다. specimen 은 라이브러리의
/// 레벨-1 보간 계수(0.835, `egui_commonmark_backend`)를 미러해 H2 크기를 근사한다.
fn md_h2_size(theme: &Theme) -> f32 {
    let min = theme.font_size_body.value();
    min + (theme.font_size_prose_h1.value() - min) * 0.835
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

/// 인라인 이미지 — 라이브러리가 실제로 그리는 raster 를 손으로 근사한다(갤러리는 파일
/// I/O 를 하지 않는다). 실제 로드 경로: relative dest 는 plugin `render.rs` 가
/// `CommonMarkViewer::default_implicit_uri_scheme` 로 base_dir 를 앵커해 `file://` URI 를
/// 만들고, `egui_extras` file+image 로더(egui_commonmark 의 `load-images` feature)가 그
/// URI 를 읽어 텍스처로 올린다. alt 텍스트는 라이브러리 기본값(`show_alt_text_on_hover`)
/// 이 hover 툴팁으로 보여주지만, 정적 specimen 에서는 항상 보이는 캡션으로 대신 노출한다.
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

fn code_block(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width() - theme.spacing_sm.value() * 2.0);
            ui.label(
                egui::RichText::new(text)
                    .monospace()
                    .size(theme.font_size_body.value())
                    .color(theme.text_secondary().to_egui()),
            );
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
    // 좌측 강조 바 폭 — spec.rs accent_bar 좌측 바와 동일하게 focus_ring_width(2px) 토큰.
    let bar_w = theme.focus_ring_width.value();
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
                egui::vec2(TILE_W, TILE_H),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.add_space(theme.spacing_xl.value());
                    add(ui);
                },
            );
        });
}

/// 주소창 라이브 데모 상태(호출측 소유 — PathField 계약). 갤러리 재-draw 간 유지되도록
/// thread-local 로 보관한다.
struct AddrDemo {
    buf: String,
    editing: bool,
    active: Option<usize>,
}

thread_local! {
    static ADDR_DEMO: RefCell<Option<AddrDemo>> = const { RefCell::new(None) };
}

/// 상단 주소창 chrome (03) — 공용 [`PathField`] 라이브 소비(markdown 컨텍스트). 하드롤
/// 전사(구 `addr_field`/`addr_go`)를 폐기하고 본체 플러그인과 같은 위젯을 그대로 그린다:
/// file leading/row 아이콘 + arrow-right Go + 최근파일 후보 드롭다운. idle=경로 secondary,
/// 클릭=editing(primary + 드롭다운 + 키내비). 40px 바는 sidebar 프레임이 소유.
fn address_bar(ui: &mut egui::Ui, theme: &Theme) {
    // 아이콘 주입 — canonical FILE / ARROW_RIGHT 를 위젯이 넘긴 rect 에 그대로 그린다
    // (색·크기는 위젯이 상태별 토큰으로 호출).
    let file_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        icons::FILE.image(rect.height(), c).paint_at(ui, rect);
    };
    let go_icon = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        icons::ARROW_RIGHT
            .image(rect.height(), c)
            .paint_at(ui, rect);
    };

    egui::Frame::new()
        .fill(theme.bg_sidebar().to_egui())
        .inner_margin(egui::Margin::symmetric(theme.spacing_sm.value() as i8, 0))
        .show(ui, |ui| {
            ADDR_DEMO.with(|s| {
                let mut slot = s.borrow_mut();
                let st = slot.get_or_insert_with(|| AddrDemo {
                    buf: "/docs/readme.md".to_string(),
                    editing: false,
                    active: None,
                });
                PathField::new("md_viewer_addr")
                    .placeholder("Go to file…")
                    .empty_label("No recent files")
                    .width(ADDR_BAR_W - theme.spacing_sm.value() * 2.0)
                    .leading_icon(&file_icon)
                    .row_icon(&file_icon)
                    .go_icon(&go_icon)
                    .show(
                        ui,
                        theme,
                        &mut st.buf,
                        &mut st.editing,
                        &mut st.active,
                        MD_RECENT,
                        "/docs/readme.md",
                    );
            });
        });
}

/// 지정 size/color 라벨 텍스트.
fn rich(_theme: &Theme, text: &str, size: f32, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(color)
}
