//! 마크다운 플러그인의 HTML 화면을 egui로 근사한 예제. 실제 WebView는 실행하지 않는다.
//! 제목 크기·표·구문 강조·알림은 Theme 값을 사용하되 브라우저와의 픽셀 일치는 보장하지 않는다.
//! 주소창과 목차는 정적으로 그리며 이미지는 파일을 읽지 않고 대체 영역을 표시한다.

mod document;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::Spinner;

use document::{document, md_h2_size};

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
            "table of contents — collapsible, in-document",
            |ui| {
                toc_chrome(ui, theme);
            },
        );
    });

    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.set_max_width(DOC_W.value());
        document(ui, theme);
    });

    spec::stage(ui, theme, StageVariant::Column, |ui| {
        type_scale(ui, theme);
    });

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
            (
                "addr bar",
                // 높이는 렌더러 CSS의 --md-addr-bar-h를 기준으로 관리한다.
                "--md-addr-bar-h sticky top · bg-sidebar · in-document HTML chrome",
            ),
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
        "The plugin renders sanitized HTML in a native WebView and applies Theme values through CSS. It interpolates heading sizes between prose-h1 and body, generates heading IDs and a collapsible table of contents, and uses a bundled highlight.js for fenced code. GFM alerts use localized labels and distinct colors. This gallery reproduces those elements with egui and fixed example data; it does not run the browser, highlighting script or navigation.",
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

/// 문서 안 HTML 주소창을 정적으로 그린다. 경로 편집은 지원하지 않는다.
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
                    .inner_margin(egui::Margin::symmetric(
                        theme.spacing_sm.value() as i8,
                        theme.spacing_xs.value() as i8,
                    ))
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

/// 문서 목차를 항상 펼친 상태로 그린다. 클릭 이동·접기 동작은 구현하지 않는다.
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
