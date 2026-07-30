//! `clipboard_viewer` specimen — clipboard-viewer plugin 의 header/type-bar/body/
//! footer popup (egui-mesh popup 전사, Overlays, TODO51).
//!
//! 본체 렌더 경로: plugin `crates/tasty-plugin-clipboard-viewer/src/view.rs` 가
//! **egui-mesh popup**(ADR-0028 / B4)으로 popup 콘텐츠를 자기 프로세스에서 egui 로
//! 그린다 — rail(세로 타입 목록)은 폐기됐다. header(아이콘+타이틀+snapshot 뱃지+
//! close) → type-bar(1개면 아이콘+뱃지, 2개 이상이면 가로 세그먼트 스위치) →
//! body(well: border+radius+bg-app 스크롤) → footer(mime+Close) 4단 수직 스택.
//! host 는 셸(scrim/border)만 그리고 plugin mesh 를 content 영역에 합성한다. 갤러리는
//! plugin/host crate 에 의존할 수 없어 그 *구성* 을 Theme 토큰 painter mock 으로
//! 전사한다 — 픽셀 동일성 비목표, 토큰·구조 정합 목표.
//!
//! 6 상태를 나란히 노출:
//! - **data (text only)** — 정상 4단, type-bar 는 배지로 표시(타입 1개).
//! - **data (files, segmented)** — type-bar 가 Text/Files 2개 세그먼트로 표시되고
//!   body 는 아이콘+경로 한 줄씩(TODO52).
//! - **image** — Image 타입 body(아이콘 + 치수·크기 메타 + "인라인 미리보기 없음"
//!   안내, 실제 픽셀 렌더링 없음 — design 결정, TODO48).
//! - **empty** — 가용 타입 0개(아이콘 + 굵은 타이틀 + 옅은 부제 2줄).
//! - **read failed** — 클립보드 핸들 실패(danger 톤).
//! - **already open** — 단일 인스턴스 가드.
//!
//! `SEG_COMPACT_AT`(5) 이상의 압축 세그먼트는 이 TODO 시점에도 실 데이터가 3종
//! (Text/Files/Image)뿐이라 실제로 재현되지 않는다 — plugin `view.rs` 와 동일한
//! 한계이며 [[49/50]]이 타입을 늘리면 그때 specimen 도 압축 세그먼트 상태를 추가한다
//! (`spec::note` 참고).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{TagVariant, tag};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// popup 본문 치수(디자인 480×360 고정 — size_hint). Theme 에 대응 토큰이 없는
/// 화면 전용 고정값.
const POPUP_W: f32 = 480.0;
const POPUP_H: f32 = 360.0;

/// CenterState 아이콘 크기(design 고정값 28 — Theme 아이콘 글리프 토큰은 16 상한).
const CENTER_ICON_SIZE: f32 = 28.0;

/// body well 안 mono 미리보기 샘플 — 현재 클립보드 text 표현.
const PREVIEW: &[&str] = &[
    "cargo build -p tasty-gallery",
    "git switch wt-5/T8-code",
    "tasty read screen --surface 3",
];

/// files 상태 body 미리보기 샘플 — 파일 탐색기에서 복사한 경로 목록(TODO52).
const FILE_PREVIEW: &[&str] = &[
    "/home/user/Documents/report.pdf",
    "/home/user/Pictures/screenshot-2026-07-30.png",
    "/home/user/workspace/tasty/Cargo.toml",
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(
            ui,
            theme,
            "text only — header / type-bar(badge) / body / footer",
            |ui| data_popup(ui, theme),
        );
        spec::cluster(
            ui,
            theme,
            "files — type-bar(segmented Text/Files) / body(icon+path rows)",
            |ui| files_popup(ui, theme),
        );
        spec::cluster(
            ui,
            theme,
            "image — icon + meta + \"no inline preview\"",
            |ui| image_popup(ui, theme),
        );
        spec::cluster(ui, theme, "empty clipboard", |ui| {
            center_popup(
                ui,
                theme,
                icons::CLIPBOARD,
                "Clipboard is empty",
                "Copy some text, an image, or files and reopen to see a snapshot here.",
                false,
            );
        });
        spec::cluster(ui, theme, "read failed", |ui| {
            center_popup(
                ui,
                theme,
                icons::ALERT_TRIANGLE,
                "Couldn't read the clipboard",
                "The system clipboard handle could not be opened. Close another app that may be holding it and reopen.",
                true,
            );
        });
        spec::cluster(ui, theme, "already open", |ui| {
            center_popup(
                ui,
                theme,
                icons::LOCK,
                "Clipboard viewer is already open",
                "Only one snapshot window runs at a time — the existing one was brought to the front.",
                false,
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "480×360 popup · bg-panel"),
            ("header", "icon + title(14/600) + snapshot tag + close"),
            (
                "type-bar",
                "≤1: icon+tag(accent) · ≥2: segmented(border-default)",
            ),
            ("body", "well(border+radius+bg-app) · mono scroll"),
            ("footer", "mime(mono caption) + Close(secondary)"),
            (
                "states",
                "data(text) · data(files) · image · empty · read-failed · already-open",
            ),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("bg-app", "body well fill", theme.bg_app().to_egui()),
            TokenChip::new(
                "bg-sidebar",
                "type-bar row fill",
                theme.bg_sidebar().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "active segment / badge",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("separator", "row divider", theme.separator.to_egui()),
            TokenChip::new(
                "accent-danger",
                "read-failed tone",
                theme.accent_danger().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "TODO51/52/48 구조 전사 — 좌측 rail(세로 타입 목록)을 폐기하고 header/type-bar/\
         body/footer 4단 수직 스택으로 교체했다. 타입이 1개(Text)면 type-bar 를 배지 \
         하나로만, 2개 이상(Text/Files, TODO52 · Text/Image, TODO48)이면 가로 세그먼트로 \
         보여준다 — `SEG_COMPACT_AT`(5) 이상의 압축 세그먼트(비활성 세그먼트가 아이콘 \
         전용으로 축소)는 골격만 갖춰뒀고 [[49/50]]이 타입을 늘리면 실제로 재현된다(그때 \
         이 specimen 도 압축 세그먼트 상태를 추가한다). files body 는 아이콘+mono 경로 \
         한 줄씩, 긴 경로는 말줄임 처리한다(design ellipsis 전사). image body 는 실제 \
         픽셀을 렌더링하지 않고 아이콘+치수·크기 메타+안내 문구만 중앙 정렬로 보여준다 \
         (design 결정, TODO48). 헤더/푸터의 Close 버튼은 host 의 outside-click/Esc 와 \
         기능 중복이지만 디자인이 명시적으로 요구해 그대로 반영했다.",
    );
}

/// image body 메타 샘플 — design mock(`clipboard_viewer.html` `multi.types` image 항목)
/// 과 동일한 예시 수치. 실제 값은 arboard `ImageData::width/height` + `bytes.len()`
/// 근사(`crates/tasty-plugin-clipboard-viewer/src/clipboard.rs::format_bytes`).
const IMAGE_META: &str = "1920×1080 · 7.9 MB";

/// 정상 데이터 상태 — header + type-bar(배지) + body(well) + footer 4행.
fn data_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        type_bar_row(ui, theme);
        body_row(ui, theme);
        footer_row(ui, theme);
    });
}

/// files 상태 — header + type-bar(Text/Files 세그먼트) + body(경로 행) + footer 4행
/// (TODO52).
fn files_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        type_bar_segmented_row(ui, theme);
        files_body_row(ui, theme);
        footer_row_files(ui, theme);
    });
}

/// image 타입 상태 — header + type-bar(Image 뱃지+meta) + body(아이콘+메타+안내) +
/// footer 4행(실제 렌더링 없음, TODO48).
fn image_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        image_type_bar_row(ui, theme);
        image_body_row(ui, theme);
        image_footer_row(ui, theme);
    });
}

/// type-bar — 타입 2개(Text/Files) 가로 세그먼트, active=Files(accent 채움).
fn type_bar_segmented_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let content = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    let mut lui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    egui::Frame::new()
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::ZERO)
        .show(&mut lui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                seg(ui, theme, icons::TEXT_LEFT, "Text", false);
                seg(ui, theme, icons::FILE, "Files", true);
            });
        });

    hline(ui, theme, rect.bottom());
}

/// type-bar — Image 뱃지(좌) + meta 텍스트(우, design `t.meta` 슬롯).
fn image_type_bar_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let content = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    let mut lui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut lui,
        icons::IMAGE,
        theme.icon_glyph_size_sm.value(),
        theme.text_muted().to_egui(),
    );
    tag(&mut lui, theme, "Image", TagVariant::Accent, false);

    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    rui.label(
        egui::RichText::new(IMAGE_META)
            .monospace()
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );

    hline(ui, theme, rect.bottom());
}

/// 세그먼트 한 칸 — active 면 accent 채움 + on-accent 텍스트.
fn seg(ui: &mut egui::Ui, theme: &Theme, glyph: MockGlyph, label: &str, active: bool) {
    let h = theme.item_height_tab.value();
    let icon_sz = theme.icon_glyph_size_xs.value();
    let pad_x = theme.spacing_sm.value();
    let gap = theme.spacing_xs.value();
    let font = egui::FontId::proportional(theme.font_size_term_sm.value());
    let label_w = ui
        .fonts(|f| f.layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::PLACEHOLDER))
        .size()
        .x;
    let w = pad_x * 2.0 + icon_sz + gap + label_w;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    if active {
        ui.painter()
            .rect_filled(rect, 0.0, theme.accent_primary().to_egui());
    }
    let fg = if active {
        theme.text_on_accent()
    } else {
        theme.text_secondary()
    }
    .to_egui();
    let icon_center = egui::pos2(rect.left() + pad_x + icon_sz * 0.5, rect.center().y);
    let icon_rect = egui::Rect::from_center_size(icon_center, egui::vec2(icon_sz, icon_sz));
    glyph.image(icon_sz, fg).paint_at(ui, icon_rect);
    ui.painter().text(
        egui::pos2(icon_center.x + icon_sz * 0.5 + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        fg,
    );
}

/// body(files) — well 안에 아이콘 + mono 경로 한 줄씩(design ellipsis 전사).
fn files_body_row(ui: &mut egui::Ui, theme: &Theme) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    let icon_sz = theme.icon_glyph_size_sm.value();
    let gap = theme.spacing_sm.value();
    let tx = well.left() + theme.spacing_sm.value();
    let mut ty = well.top() + theme.spacing_sm.value();
    let line_h = icon_sz.max(theme.font_size_term_sm.value()) + theme.spacing_xs.value();
    for path in FILE_PREVIEW {
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(tx + icon_sz * 0.5, ty + line_h * 0.5),
            egui::vec2(icon_sz, icon_sz),
        );
        icons::FILE
            .image(icon_sz, theme.text_muted().to_egui())
            .paint_at(ui, icon_rect);
        p.text(
            egui::pos2(tx + icon_sz + gap, ty + line_h * 0.5),
            egui::Align2::LEFT_CENTER,
            path,
            egui::FontId::monospace(theme.font_size_term_sm.value()),
            theme.text_primary().to_egui(),
        );
        ty += line_h;
    }
}

/// body — well 안에 아이콘(30px 고정) + 메타 + "인라인 미리보기 없음" 안내를 상하좌우
/// 중앙 정렬(design jsx image 분기의 `cbWell` + `alignItems/justifyContent: center`).
fn image_body_row(ui: &mut egui::Ui, theme: &Theme) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    const IMAGE_BODY_ICON_SIZE: f32 = 30.0;
    let gap = theme.spacing_sm.value();
    let icon_h = IMAGE_BODY_ICON_SIZE;
    let meta_h = theme.font_size_caption.value();
    let sub_h = theme.font_size_caption.value();
    let block_h = icon_h + gap + meta_h + theme.spacing_xs.value() + sub_h;
    let mut y = well.center().y - block_h / 2.0;

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(well.center().x, y + icon_h / 2.0),
        egui::vec2(icon_h, icon_h),
    );
    icons::IMAGE
        .image(IMAGE_BODY_ICON_SIZE, theme.text_muted().to_egui())
        .paint_at(ui, icon_rect);
    y += icon_h + gap;

    ui.painter().text(
        egui::pos2(well.center().x, y),
        egui::Align2::CENTER_TOP,
        IMAGE_META,
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    y += meta_h + theme.spacing_xs.value();

    ui.painter().text(
        egui::pos2(well.center().x, y),
        egui::Align2::CENTER_TOP,
        "No inline image preview",
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_disabled().to_egui(),
    );
}

/// footer(files) — mime(`text/uri-list`) + 우측 Close(secondary).
fn footer_row_files(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    ui.painter().text(
        egui::pos2(rect.left() + pad_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "text/uri-list",
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );

    let btn_w = 64.0;
    let btn_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - pad_x - btn_w, rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    ui.painter().rect(
        btn_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        btn_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Close",
        egui::FontId::proportional(theme.font_size_term_sm.value()),
        theme.text_secondary().to_egui(),
    );
}

/// footer — mime(`image/rgba8`) + 우측 Close(secondary).
fn image_footer_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    ui.painter().text(
        egui::pos2(rect.left() + pad_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "image/rgba8",
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );

    let btn_w = 64.0;
    let btn_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - pad_x - btn_w, rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    ui.painter().rect(
        btn_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        btn_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Close",
        egui::FontId::proportional(theme.font_size_term_sm.value()),
        theme.text_secondary().to_egui(),
    );
}

/// header — 클립보드 아이콘 + "Clipboard" + snapshot 뱃지 + 우측 close.
fn header_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_md.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let content = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    let mut lui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut lui,
        icons::CLIPBOARD,
        theme.icon_glyph_size_md.value(),
        theme.text_muted().to_egui(),
    );
    lui.label(
        egui::RichText::new("Clipboard")
            .size(theme.font_size_max.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );
    tag(&mut lui, theme, "snapshot", TagVariant::Default, false);

    let close_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(close_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    kit::icon(
        &mut rui,
        icons::CLOSE,
        theme.icon_glyph_size_sm.value(),
        theme.text_secondary().to_egui(),
    );

    hline(ui, theme, rect.bottom());
}

/// type-bar — 타입이 1개(Text)뿐인 상태는 세그먼트가 아니라 아이콘+accent 뱃지.
fn type_bar_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let content = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    let mut lui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut lui,
        icons::TEXT_LEFT,
        theme.icon_glyph_size_sm.value(),
        theme.text_muted().to_egui(),
    );
    tag(&mut lui, theme, "Text", TagVariant::Accent, false);

    hline(ui, theme, rect.bottom());
}

/// body — well(border+radius+bg-app) 안에 mono 미리보기.
fn body_row(ui: &mut egui::Ui, theme: &Theme) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    let tx = well.left() + theme.spacing_sm.value();
    let mut ty = well.top() + theme.spacing_sm.value();
    let line_h = theme.font_size_term_sm.value() + theme.spacing_xs.value();
    for line in PREVIEW {
        p.text(
            egui::pos2(tx, ty),
            egui::Align2::LEFT_TOP,
            line,
            egui::FontId::monospace(theme.font_size_term_sm.value()),
            theme.text_primary().to_egui(),
        );
        ty += line_h;
    }
}

/// footer — mime(mono caption) + 우측 Close(secondary).
fn footer_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    ui.painter().text(
        egui::pos2(rect.left() + pad_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "text/plain",
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );

    let btn_w = 64.0;
    let btn_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - pad_x - btn_w, rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    ui.painter().rect(
        btn_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        btn_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Close",
        egui::FontId::proportional(theme.font_size_term_sm.value()),
        theme.text_secondary().to_egui(),
    );
}

/// CenterState — 아이콘(28px) + 굵은 타이틀 + 옅은 부제 2줄, popup 높이 절반 정도.
fn center_popup(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: MockGlyph,
    title: &str,
    sub: &str,
    danger: bool,
) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        let h = (POPUP_H / 2.0).round();
        let w = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(w, h),
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
            |ui| {
                ui.vertical_centered(|ui| {
                    let tint = if danger {
                        theme.accent_danger().to_egui().gamma_multiply(0.9)
                    } else {
                        theme.text_muted().to_egui().gamma_multiply(0.5)
                    };
                    kit::icon(ui, glyph, CENTER_ICON_SIZE, tint);
                    ui.add_space(theme.spacing_sm.value());
                    let title_color = if danger {
                        theme.accent_danger().to_egui()
                    } else {
                        theme.text_secondary().to_egui()
                    };
                    ui.label(
                        egui::RichText::new(title)
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(title_color),
                    );
                    ui.add_space(theme.spacing_xs.value());
                    ui.label(
                        egui::RichText::new(sub)
                            .size(theme.font_size_term_sm.value())
                            .color(theme.text_muted().to_egui()),
                    );
                });
            },
        );
    });
}

fn hline(ui: &mut egui::Ui, theme: &Theme, y: f32) {
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.x_range(),
        y - theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}
