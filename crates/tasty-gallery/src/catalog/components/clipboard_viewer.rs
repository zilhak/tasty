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
//! 4 상태를 나란히 노출:
//! - **data** — 정상 4단(오늘의 실 데이터는 Text 뿐이라 type-bar 는 배지로 표시).
//! - **empty** — 가용 타입 0개(아이콘 + 굵은 타이틀 + 옅은 부제 2줄).
//! - **read failed** — 클립보드 핸들 실패(danger 톤).
//! - **already open** — 단일 인스턴스 가드.
//!
//! 세그먼트(2개 이상 타입, `SEG_COMPACT_AT`=5 압축)는 이 TODO 시점엔 실 데이터가
//! Text 하나뿐이라 실제로 재현되지 않는다 — plugin `view.rs` 와 동일한 한계이며
//! [[48/49/50/52]]가 타입을 늘리면 그때 specimen 도 세그먼트 상태를 추가한다
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

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(
            ui,
            theme,
            "text only — header / type-bar(badge) / body / footer",
            |ui| data_popup(ui, theme),
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
            ("states", "data · empty · read-failed · already-open"),
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
        "TODO51 구조 전사 — 좌측 rail(세로 타입 목록)을 폐기하고 header/type-bar/body/\
         footer 4단 수직 스택으로 교체했다. 오늘의 유일한 실 타입(Text)은 type-bar 를 \
         배지 하나로만 보여준다 — 세그먼트(2개 이상 가로 버튼 그룹, 5개 이상이면 \
         비활성 세그먼트가 아이콘 전용으로 압축)는 골격만 갖춰뒀고 [[48/49/50/52]]가 \
         타입을 늘리면 실제로 재현된다(그때 이 specimen 도 세그먼트 상태를 추가한다). \
         헤더/푸터의 Close 버튼은 host 의 outside-click/Esc 와 기능 중복이지만 디자인이 \
         명시적으로 요구해 그대로 반영했다.",
    );
}

/// 정상 데이터 상태 — header + type-bar(배지) + body(well) + footer 4행.
fn data_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        type_bar_row(ui, theme);
        body_row(ui, theme);
        footer_row(ui, theme);
    });
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

/// type-bar — 오늘의 유일한 실 타입(Text)은 세그먼트가 아니라 아이콘+accent 뱃지.
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
