//! 탐색기 Properties 팝업 · 미리보기 패널 · Grid 썸네일 예제.
//! 시안 `gallery/explorer-ops.html`의 "Properties · preview panel · Grid thumbnails" 구역을 옮긴다.
//! 본체는 Properties 를 칸 범위 popup(`explorer_properties`)으로, 미리보기를 목록 오른쪽 패널로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ButtonVariant, ControlSize, IconButton, IconButtonVariant, Spinner};

use super::explorer_states::{StateCell, StateGlyph, Tone, state_block};
use super::explorer_view_cells::{Kind, grid_cell_parts};
use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};
use crate::catalog::widgets::dialog as kit;
use crate::i18n::{t, t_fmt, t_fmt2};

/// 상태 패널 예제의 높이(시안 `height: 230`). 전시 칸 치수다.
const PANEL_STAGE_H: LogicalPx = LogicalPx(230.0);

/// 시안 `YProps` 좌우 여백 `--tasty-size-14`. 대응 컴포넌트 토큰이 없다.
const PROPS_PAD_X: LogicalPx = LogicalPx(14.0);
/// 시안 `YField` 줄 높이 `--tasty-size-20`. 대응 컴포넌트 토큰이 없다.
const FIELD_LINE_H: LogicalPx = LogicalPx(20.0);
/// 시안 `YField` 최소 높이 `--tasty-size-24`. 대응 컴포넌트 토큰이 없다.
const FIELD_MIN_H: LogicalPx = LogicalPx(24.0);
/// 시안 `YField` 위 여백 `paddingTop: 2`. 대응 컴포넌트 토큰이 없다.
const FIELD_TOP: LogicalPx = LogicalPx(2.0);
/// 시안 `YPreview` 머리 높이 `--tasty-size-40`. 대응 컴포넌트 토큰이 없다.
const PREVIEW_HEAD_H: LogicalPx = LogicalPx(40.0);

/// 위 상수는 Theme 값과 달리 배율을 타지 않았으므로 같은 식에 쓰기 전에 UI 배율을 곱한다.
fn zoomed(theme: &Theme, px: LogicalPx) -> f32 {
    (px.value() * theme.ui_zoom).round()
}

/// Properties 필드 한 줄의 값.
struct Field {
    /// 라벨의 번역 키. 본체와 같은 문구를 쓴다.
    label: &'static str,
    value: String,
    mono: bool,
    copy: bool,
    counting: bool,
}

fn field(label: &'static str, value: &str) -> Field {
    Field {
        label,
        value: value.to_owned(),
        mono: false,
        copy: false,
        counting: false,
    }
}

fn mono(label: &'static str, value: &str, copy: bool) -> Field {
    Field {
        label,
        value: value.to_owned(),
        mono: true,
        copy,
        counting: false,
    }
}

/// 시안 `YField`: 라벨(96, caption muted) · 값(body 또는 mono caption, 어디서나 줄바꿈) · 선택 Copy.
fn field_row(ui: &mut egui::Ui, theme: &Theme, f: &Field) {
    let label_w = theme.explorer_props_label_width().value();
    let gap = theme.spacing_sm.value();
    let line_h = zoomed(theme, FIELD_LINE_H);
    let min_h = zoomed(theme, FIELD_MIN_H);
    let top = zoomed(theme, FIELD_TOP);
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        egui::vec2(width, min_h),
        egui::Layout::left_to_right(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            ui.set_min_height(min_h);
            ui.vertical(|ui| {
                ui.add_space(top);
                ui.set_width(label_w);
                ui.allocate_ui_with_layout(
                    egui::vec2(label_w, line_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(t(f.label))
                                    .size(theme.font_size_caption.value())
                                    .color(theme.text_muted().to_egui()),
                            )
                            .truncate(),
                        )
                    },
                );
            });
            ui.vertical(|ui| {
                ui.add_space(top);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
                    if f.counting {
                        let s = theme.icon_glyph_size_sm.value();
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(s, line_h), egui::Sense::hover());
                        let mut slot =
                            ui.new_child(egui::UiBuilder::new().max_rect(
                                egui::Rect::from_center_size(r.center(), egui::vec2(s, s)),
                            ));
                        Spinner::new().size(s).show(&mut slot, theme);
                    }
                    let copy_w = if f.copy {
                        ControlSize::Sm.height(theme) + theme.spacing_xs.value()
                    } else {
                        0.0
                    };
                    let font = if f.mono {
                        egui::FontId::monospace(theme.font_size_caption.value())
                    } else {
                        egui::FontId::proportional(theme.font_size_body.value())
                    };
                    let text_w = (ui.available_width() - copy_w).max(0.0);
                    let mut job = egui::text::LayoutJob::simple(
                        f.value.clone(),
                        font,
                        theme.text_secondary().to_egui(),
                        text_w,
                    );
                    job.wrap.break_anywhere = true;
                    let galley = ui.fonts(|fonts| fonts.layout_job(job));
                    // 첫 줄을 줄 높이 가운데에 두고, 줄을 바꾼 값은 글자 줄 높이만큼만 늘린다.
                    let first_h = galley.rows.first().map_or(line_h, |r| r.height());
                    let text_h = (galley.rect.height() + line_h - first_h).max(line_h);
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(text_w, text_h), egui::Sense::hover());
                    let y = r.top() + (line_h - first_h) / 2.0;
                    ui.painter().galley(
                        egui::pos2(r.left(), y),
                        galley,
                        theme.text_secondary().to_egui(),
                    );
                    if f.copy {
                        icon_button(ui, theme, icons::COPY);
                    }
                });
            });
        },
    );
}

fn icon_button(ui: &mut egui::Ui, theme: &Theme, g: MockGlyph) -> egui::Response {
    IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(ui, theme, &|ui, rect, c| {
            g.image(rect.height(), c).paint_at(ui, rect)
        })
}

/// 시안 `YProps`: 360 카드 · 머리(글리프 + 이름 14 semibold 말줄임 + 닫기) · 필드 · 선택 안내 문구.
fn props_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: MockGlyph,
    name: &str,
    fields: &[Field],
    note_text: Option<&str>,
) {
    let pad_x = zoomed(theme, PROPS_PAD_X);
    let width = theme.explorer_props_width();
    kit::frame_card(ui, theme, width, kit::panel_fill(theme), |ui| {
        kit::region(
            ui,
            egui::Margin {
                left: pad_x as i8,
                right: pad_x as i8,
                top: theme.spacing_md.value() as i8,
                bottom: theme.spacing_sm.value() as i8,
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    let g = theme.icon_glyph_size_md.value();
                    kit::icon(
                        ui,
                        glyph,
                        theme.icon_glyph_size_md,
                        theme.text_muted().to_egui(),
                    );
                    let close_w = ControlSize::Sm.height(theme);
                    let name_w =
                        (ui.available_width() - close_w - theme.spacing_sm.value()).max(0.0);
                    let (name_rect, _) =
                        ui.allocate_exact_size(egui::vec2(name_w, g), egui::Sense::hover());
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        name.to_owned(),
                        egui::FontId::proportional(theme.font_size_max.value()),
                        theme.text_primary().to_egui(),
                    );
                    job.wrap = egui::text::TextWrapping::truncate_at_width(name_w);
                    let galley = ui.fonts(|f| f.layout_job(job));
                    let y = name_rect.center().y - galley.rect.height() / 2.0;
                    ui.painter().galley(
                        egui::pos2(name_rect.left(), y),
                        galley,
                        theme.text_primary().to_egui(),
                    );
                    icon_button(ui, theme, icons::CLOSE);
                });
            },
        );
        kit::region(
            ui,
            egui::Margin {
                left: pad_x as i8,
                right: pad_x as i8,
                top: 0,
                bottom: theme.spacing_md.value() as i8,
            },
            |ui| {
                for f in fields {
                    field_row(ui, theme, f);
                }
                if let Some(text) = note_text {
                    ui.add_space(theme.spacing_sm.value());
                    ui.label(
                        egui::RichText::new(text)
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                }
            },
        );
    });
}

pub fn draw_properties(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        cluster(ui, theme, "local file", |ui| {
            props_card(
                ui,
                theme,
                icons::IMAGE,
                "diagram.png",
                &[
                    field("explorer.properties.kind", "PNG"),
                    field("explorer.properties.size", "488 KB (499,712 bytes)"),
                    mono("explorer.properties.modified", "2026-06-26 18:05", false),
                    mono("explorer.properties.created", "2026-06-26 17:58", false),
                    mono("explorer.properties.location", "~/Downloads", true),
                    mono(
                        "explorer.properties.permissions",
                        "rw-r--r-- · read-only: no",
                        false,
                    ),
                ],
                None,
            )
        });
        cluster(ui, theme, "folder — size still counting", |ui| {
            props_card(
                ui,
                theme,
                icons::FOLDER,
                "mockup-exports",
                &[
                    field("explorer.properties.kind", t("explorer.type.folder")),
                    Field {
                        counting: true,
                        ..field("explorer.properties.size", "Counting… 1,204 items · 1.1 GB")
                    },
                    mono("explorer.properties.modified", "2026-06-20 14:30", false),
                    mono("explorer.properties.location", "~/Downloads", true),
                ],
                None,
            )
        });
        cluster(ui, theme, "symlink", |ui| {
            props_card(
                ui,
                theme,
                icons::FILE,
                "current",
                &[
                    field("explorer.properties.kind", t("explorer.properties.symlink")),
                    mono(
                        "explorer.properties.link_target",
                        "~/work/tasty/target/release/tasty",
                        true,
                    ),
                    mono("explorer.properties.location", "~/bin", true),
                ],
                None,
            )
        });
        cluster(ui, theme, "several items", |ui| {
            props_card(
                ui,
                theme,
                icons::LAYERS,
                &t_fmt("explorer.properties.items", "3"),
                &[
                    field("explorer.properties.kinds", "2 files, 1 folder"),
                    field("explorer.properties.total_size", "66.4 MB"),
                    mono("explorer.properties.location", "~/Downloads", true),
                ],
                None,
            )
        });
        cluster(ui, theme, "remote item", |ui| {
            props_card(
                ui,
                theme,
                icons::FILE,
                "build-0412.log",
                &[
                    field("explorer.properties.kind", t("explorer.type.file")),
                    field("explorer.properties.size", "2.1 MB"),
                    mono("explorer.properties.modified", "2026-10-09 08:12", false),
                    mono("explorer.properties.location", "build-eu:~/logs", true),
                ],
                Some(t("explorer.properties.remote_note")),
            )
        });
    });

    meta(
        ui,
        theme,
        &[
            (
                "open",
                "context menu Properties (every shape) · keybinding action explorer.properties",
            ),
            (
                "popup",
                "explorer-props-width 360 · scoped to the cell · shadow-modal · Esc / ×",
            ),
            ("title", "glyph + name · 14 · ellipsis"),
            (
                "fields",
                "label explorer-props-label-width 96 · caption muted · value body 13 (mono caption for paths / dates / mode) · wrap anywhere",
            ),
            (
                "local file",
                "Kind · Size (with bytes) · Modified · Created · Location · Permissions",
            ),
            (
                "folder",
                "Size counts in the background (Spinner) · item count",
            ),
            ("symlink", "Link target + Copy"),
            (
                "several",
                "“{n} items” · Kinds · Total size · Location (common parent)",
            ),
            ("remote", "Kind · Size · Modified · Location · muted note"),
            ("selection change", "popup keeps its item"),
        ],
        &[
            TokenChip::without_color("explorer-props-width", "→ size-360"),
            TokenChip::without_color("explorer-props-label-width", "→ size-96"),
            TokenChip::without_color("shadow-modal", "popup"),
            TokenChip::new(
                "spinner-indicator",
                "counting",
                theme.spinner_indicator().to_egui(),
            ),
        ],
    );
    note(
        ui,
        theme,
        "The symlink card uses the file glyph: the design's link glyph is not in the shared icon set yet. \
         The title is regular weight because egui UI registers no semibold face.",
    );
}

/// 미리보기 패널 예제의 종류.
#[derive(Clone, Copy)]
enum PreviewKind {
    Text,
    Image,
    None,
    Loading,
    Large,
    Pixels,
    Several,
    Error,
}

const PREVIEW_TEXT: &str = "# Notes\n\n- split floor 180\n- favorites pin 240\n- drag: move same disk\n\n## Open\n- thumbnails in Grid\n- preview panel back";

/// 시안 `YPreview`: 288 폭 · 40 머리(이름 body · "종류 · 크기" caption muted) · bg-sidebar 바탕 본문.
fn preview_panel(ui: &mut egui::Ui, theme: &Theme, kind: PreviewKind, height: f32) {
    let (name, facts) = match kind {
        PreviewKind::Text | PreviewKind::Loading => ("notes.md", "Markdown · 12 KB"),
        PreviewKind::Image => ("diagram.png", "PNG · 1280 × 720 · 488 KB"),
        PreviewKind::None => ("archive.zip", "Archive · 64 MB"),
        PreviewKind::Large => ("server.log", "Log · 38 MB"),
        PreviewKind::Pixels => ("scan-poster.tif", "TIFF · 61.0 MB"),
        PreviewKind::Several => ("3 items", "2 files, 1 folder"),
        PreviewKind::Error => ("private.key", "File · 3 KB"),
    };
    let w = theme.explorer_preview_width().value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, height), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let line = egui::Stroke::new(
        theme.border_width.value(),
        theme.separator.to_egui_premultiplied(),
    );
    p.rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let head_h = zoomed(theme, PREVIEW_HEAD_H);
    let head = egui::Rect::from_min_size(rect.min, egui::vec2(w, head_h));
    let pad = theme.spacing_sm.value();
    let inner_w = (w - pad * 2.0).max(0.0);
    let name_g = ui.painter().layout(
        name.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_primary().to_egui(),
        inner_w,
    );
    let facts_g = ui.painter().layout_no_wrap(
        facts.to_owned(),
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    let block_h = name_g.rect.height() + facts_g.rect.height();
    let top = head.center().y - block_h / 2.0;
    let name_h = name_g.rect.height();
    p.galley(
        egui::pos2(head.left() + pad, top),
        name_g,
        theme.text_primary().to_egui(),
    );
    p.galley(
        egui::pos2(head.left() + pad, top + name_h),
        facts_g,
        theme.text_muted().to_egui(),
    );
    p.hline(head.x_range(), head.bottom(), line);
    let body = egui::Rect::from_min_max(egui::pos2(rect.left(), head.bottom()), rect.max);
    p.rect_filled(body, 0.0, theme.bg_sidebar().to_egui());
    match kind {
        PreviewKind::Text => {
            let g = ui.painter().layout_no_wrap(
                PREVIEW_TEXT.to_owned(),
                egui::FontId::monospace(theme.font_size_caption.value()),
                theme.text_secondary().to_egui(),
            );
            p.galley(
                body.min + egui::vec2(pad, pad),
                g,
                theme.text_secondary().to_egui(),
            );
        }
        PreviewKind::Image => {
            let inner = body.shrink(theme.spacing_md.value());
            let img_h = inner.width() * 9.0 / 16.0;
            let img =
                egui::Rect::from_center_size(inner.center(), egui::vec2(inner.width(), img_h));
            p.rect_filled(
                img,
                theme.corner_radius_sm.value(),
                theme.surface_raised().to_egui(),
            );
            p.rect_stroke(
                img,
                theme.corner_radius_sm.value(),
                line,
                egui::StrokeKind::Inside,
            );
            p.text(
                img.center(),
                egui::Align2::CENTER_CENTER,
                "image · fit",
                egui::FontId::monospace(theme.font_size_micro.value()),
                theme.text_muted().to_egui(),
            );
        }
        _ => {
            let pixels_sub = t_fmt2("explorer.preview.too_large_pixels_sub", "16384", "256 MiB");
            let several = t("explorer.preview.multi").replace("{n}", "3");
            let cell = match kind {
                PreviewKind::Loading => StateCell {
                    glyph: StateGlyph::Spinner,
                    tone: Tone::Neutral,
                    title: t("explorer.preview.loading"),
                    sub: None,
                    reason: None,
                    actions: &[],
                },
                PreviewKind::Large => StateCell {
                    glyph: StateGlyph::Icon(icons::FILE),
                    tone: Tone::Neutral,
                    title: t("explorer.preview.too_large"),
                    sub: Some("Over 1 MB."),
                    reason: None,
                    actions: &[],
                },
                PreviewKind::Pixels => StateCell {
                    glyph: StateGlyph::Icon(icons::FILE),
                    tone: Tone::Neutral,
                    title: t("explorer.preview.too_large"),
                    sub: Some(&pixels_sub),
                    reason: Some("20000 × 14000 px"),
                    actions: &[],
                },
                PreviewKind::Several => StateCell {
                    glyph: StateGlyph::Icon(icons::LAYERS),
                    tone: Tone::Neutral,
                    title: &several,
                    sub: Some(t("explorer.preview.multi_sub")),
                    reason: None,
                    actions: &[],
                },
                PreviewKind::Error => StateCell {
                    glyph: StateGlyph::Icon(icons::ALERT_TRIANGLE),
                    tone: Tone::Error,
                    title: t("explorer.preview.unreadable"),
                    sub: None,
                    reason: Some("Permission denied (os error 13)"),
                    actions: &[],
                },
                _ => StateCell {
                    glyph: StateGlyph::Icon(icons::FILE),
                    tone: Tone::Neutral,
                    title: t("explorer.preview.unsupported"),
                    sub: None,
                    reason: None,
                    actions: &[] as &[(&str, ButtonVariant)],
                },
            };
            state_block(ui, theme, body.shrink(pad), &cell);
        }
    }
    ui.painter().vline(rect.left(), rect.y_range(), line);
}

pub fn draw_preview(ui: &mut egui::Ui, theme: &Theme) {
    let h = PANEL_STAGE_H.value();
    stage(ui, theme, StageVariant::Wrap, |ui| {
        for (label, kind) in [
            ("text", PreviewKind::Text),
            ("image", PreviewKind::Image),
            ("not supported", PreviewKind::None),
            ("loading", PreviewKind::Loading),
            ("too large", PreviewKind::Large),
            ("over the pixel limit", PreviewKind::Pixels),
            ("several selected", PreviewKind::Several),
            ("unreadable", PreviewKind::Error),
        ] {
            cluster(ui, theme, label, |ui| preview_panel(ui, theme, kind, h));
        }
    });

    meta(
        ui,
        theme,
        &[
            (
                "toggle",
                "toolbar view group · columns glyph · keybinding action explorer.toggle_preview",
            ),
            (
                "panel",
                "right · explorer-preview-width 288 · splitter 200 … 460 · 1px separator · remembered per explorer",
            ),
            (
                "header",
                "40 · name (body, ellipsis) · kind · size (caption muted)",
            ),
            (
                "text",
                "first 64 KB · mono caption · on bg-sidebar · no wrap",
            ),
            ("image", "fit, never upscaled · on bg-sidebar"),
            ("other / folder", "“No preview for this file type”"),
            (
                "loading",
                "Spinner · “Loading preview…” — the old preview is cleared first",
            ),
            ("too large", "> 1 MB (app limit) · “Too large to preview”"),
            (
                "pixel limit",
                "same screen · “Over {px} px on a side, or needs more than {mem} to decode.” · reason = real size · MiB",
            ),
            (
                "several",
                "layers glyph · “{n} items selected” · “Select one file to preview it.” · none = “Select a file”",
            ),
            ("unreadable", "error tone · OS reason (mono)"),
            (
                "narrow cell",
                "panel hides itself below 200 + list min; toggle stays on",
            ),
        ],
        &[
            TokenChip::without_color("explorer-preview-width", "→ size-288"),
            TokenChip::without_color("explorer-preview-min-width", "→ size-200"),
            TokenChip::without_color("explorer-preview-max-width", "→ size-460"),
            TokenChip::new("bg-sidebar", "preview bed", theme.bg_sidebar().to_egui()),
        ],
    );
}

pub fn draw_thumbnails(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Wrap, |ui| {
        egui::Frame::new()
            .fill(theme.bg_panel().to_egui())
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
                    for (kind, name, selected) in [
                        (Kind::Folder, "mockup-exports", false),
                        (Kind::Thumb, "diagram.png", true),
                        (Kind::Thumb, "screenshot-2026-10-09.png", false),
                        (Kind::Image, "IMG_2031.heic", false),
                        (Kind::File, "notes.md", false),
                    ] {
                        grid_cell_parts(ui, theme, kind, name, selected, false, false);
                    }
                });
            });
    });

    meta(
        ui,
        theme,
        &[
            ("scope", "Grid only · local only"),
            ("slot", "explorer-grid-thumb-size 40 · every cell"),
            ("thumbnail", "fit 40 × 40 · 1px separator · radius-sm"),
            ("loading / over limit", "image glyph 16 · accent-info"),
            ("cell", "80 wide (unchanged) · +24 tall"),
        ],
        &[
            TokenChip::without_color("explorer-grid-thumb-size", "→ size-40"),
            TokenChip::new(
                "accent-info",
                "image glyph fallback",
                theme.accent_info().to_egui(),
            ),
            TokenChip::new(
                "separator",
                "thumb edge",
                theme.separator.to_egui_premultiplied(),
            ),
        ],
    );
    note(
        ui,
        theme,
        "“img” stands in for the decoded picture. The real grid cell reserves the 40 slot for every entry; \
         the thumbnail cache is keyed by path + mtime.",
    );
}
