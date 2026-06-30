//! `markdown_viewer` specimen — Markdown surface 의 host egui 패널 (Layouts).
//!
//! 본체 렌더 경로: `src/adapters/ui/surface/markdown.rs::draw_markdown` 가
//! `ScrollArea::vertical` 안에서 tasty 자체 렌더러(`surface/markdown/render.rs`)로
//! pulldown-cmark AST 를 그린다. toolbar·헤더 없이 surface 타일 전체를 본문이 채운다.
//!
//! 디자인의 핵심은 **14px UI cap 안의 prose element 계층**이다: heading 은 크기만이 아니라
//! `size → weight → color → case` 로 갈린다. h1 만 content 예외로 `prose-h1`(20)이고
//! 나머지는 ≤ `max`(14). egui 는 합성 weight 가 없어 700/600/500 단계는 **size+color+case**
//! 가 대신 나르며(h2·h3 은 같은 14/primary 라 시각적으로 겹친다 — egui 한계), inline bold 는
//! text-primary 승격으로 신호한다. 본문 줄간격은 `line-height-prose`(1.6).
//!
//! 갤러리는 본체 crate·pulldown-cmark 에 의존하지 않으므로 같은 토큰·계층으로 element
//! catalog(heading 6단계 / inline runs / bullet·ordered·task list / table / nested
//! blockquote / hr / code)와 상태(load-fail·empty·loading)를 전사한다 — 픽셀 동일성
//! 비목표, 토큰·구조 정합 목표.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Spinner, checkbox};

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 문서 카드 폭(전시 박스).
const DOC_W: f32 = 560.0;
/// 상태 타일 치수.
const TILE_W: f32 = 200.0;
const TILE_H: f32 = 132.0;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
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
            ("frame", "surface tile · bg-panel · no toolbar"),
            ("body", "13 · line-height-prose 1.6 · text-secondary"),
            ("h1", "prose-h1 20 · text-primary (cap-exempt)"),
            ("h2/h3", "14 · text-primary"),
            ("h4/h5/h6", "13 · secondary→muted · h6 UPPER"),
            ("code", "mono · surface-raised"),
            ("link", "accent-primary · underline"),
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
            TokenChip::new("separator", "table · hr", theme.separator.to_egui()),
            TokenChip::new(
                "border-strong",
                "blockquote bar",
                theme.border_strong().to_egui(),
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
        "A read-only Markdown surface — tasty parses with pulldown-cmark and paints every \
         element from Theme tokens so the six-level prose heading hierarchy (size → weight \
         → color → case) and the 1.6 line-height-prose body leading hold. egui has no \
         synthetic font weight, so the 700/600/500 steps are carried by size + color + \
         case (h2 and h3 share 14/primary and read alike — an egui limit). Below the \
         document: the heading type-scale, and the load-fail / empty / loading chrome that \
         replaces a raw `Error:` body.",
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
            theme.font_size_prose_h2.value(),
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

/// 1px divider 테이블 — header(bg-sidebar) + 2 rows.
fn table(ui: &mut egui::Ui, theme: &Theme) {
    let body = theme.font_size_body.value();
    let pad = theme.spacing_sm.value();
    egui::Frame::new()
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let row = |ui: &mut egui::Ui, cells: [&str; 3], header: bool| {
                let fill = if header {
                    Some(theme.bg_sidebar().to_egui())
                } else {
                    None
                };
                let mut f = egui::Frame::new().inner_margin(egui::Margin::symmetric(
                    pad as i8,
                    (theme.spacing_xs.value() * 1.5) as i8,
                ));
                if let Some(c) = fill {
                    f = f.fill(c);
                }
                let _ = header;
                f.show(ui, |ui| {
                    let color = theme.text_secondary().to_egui();
                    let render_cell = |ui: &mut egui::Ui, i: usize, cell: &str| {
                        let rt = if i == 2 {
                            egui::RichText::new(cell)
                                .monospace()
                                .size(body)
                                .color(color)
                        } else {
                            egui::RichText::new(cell).size(body).color(color)
                        };
                        ui.label(rt);
                    };
                    // 동적폭 + columns 방어 가드: 잔여폭이 컬럼 spacing 합 미만이면
                    // columns 내부의 (avail - total_spacing)/3 가 음수가 되어 panic
                    // (egui ui.rs set_min_width assert). 그 경우 세로 폴백으로 그린다.
                    let total_spacing = ui.spacing().item_spacing.x * 2.0;
                    if ui.available_width() > total_spacing + 1.0 {
                        ui.columns(3, |c| {
                            for (i, cell) in cells.iter().enumerate() {
                                render_cell(&mut c[i], i, cell);
                            }
                        });
                    } else {
                        ui.vertical(|ui| {
                            for (i, cell) in cells.iter().enumerate() {
                                render_cell(ui, i, cell);
                            }
                        });
                    }
                });
            };
            row(ui, ["Resource", "Kind", "Count"], true);
            table_divider(ui, theme);
            row(ui, ["surface", "viewer", "12"], false);
            table_divider(ui, theme);
            row(ui, ["popup", "overlay", "8"], false);
        });
}

fn table_divider(ui: &mut egui::Ui, theme: &Theme) {
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
        2 => (
            theme.font_size_prose_h2.value(),
            theme.text_primary().to_egui(),
            false,
        ),
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

/// 지정 size/color 라벨 텍스트.
fn rich(_theme: &Theme, text: &str, size: f32, color: egui::Color32) -> egui::RichText {
    egui::RichText::new(text).size(size).color(color)
}
