//! 전체화면 무대(fullscreen stage) — 창 전체를 독점하는 독립 표면 + 그 진입 버튼.
//!
//! 무대는 popup 을 확대한 것이 **아니다.** 같은 형상으로 구성된 별개 콘텐츠를 창
//! 전체에 올린 것이고, 뒤의 popup 은 열린 채로 남는다(본체
//! `docs/design/systems/fullscreen-stage.md`). 여기 두 specimen 이 그 한 쌍을 보여준다:
//!
//! - [`draw`] — 무대 셸: scrim + 제목 + **셸이 공통 제공하는 종료 버튼** + 콘텐츠 프레임.
//! - [`draw_titlebar`] — popup 타이틀바: 무대를 선언한 popup 에만 X 왼쪽에 전체화면
//!   버튼이 붙고, 선언하지 않은 popup 은 이전 그대로다.
//!
//! 두 종료/전체화면 어포던스는 **본체와 같은 렌더 방식**으로 그린다 — 렌더 방식이
//! 갈리면 specimen 이 본체를 대신 검증하지 못한다(구조 전사 원칙,
//! `docs/design/systems/design-parity-notes.md`).
//!
//! - 무대 셸의 종료 버튼: 본체 `fullscreen.rs::draw_exit_button` 과 같은
//!   `IconButton`(Ghost/Md) + canonical `close` SVG. 셸에는 `Ui` 가 있다.
//! - popup 타이틀바 버튼: 본체가 `ctx.layer_painter` 하나로 타이틀바를 그려 그 구간엔
//!   `Ui` 가 없으므로 양쪽 모두 painter 직선이다(형상은 canonical `close`/`fit` 과 동일).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant};

use super::glyph;
use crate::catalog::popup_frame::{self, TitleButtons};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 무대 셸 데모 크기 — 창 전체를 축소한 비율 무대.
const STAGE_W: LogicalPx = LogicalPx(640.0);
const STAGE_H: LogicalPx = LogicalPx(300.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(STAGE_W.value(), STAGE_H.value()),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(rect);

        // 창 전체 scrim — 마커 오버레이와 같은 토큰.
        painter.rect_filled(rect, 0.0, theme.scrim().to_egui());

        // 셸 chrome ① 제목 (상단 중앙).
        let pad = theme.spacing_xl.value();
        painter.text(
            egui::pos2(rect.center().x, rect.top() + pad),
            egui::Align2::CENTER_TOP,
            "Notifications",
            egui::FontId::proportional(theme.font_size_heading.value()),
            theme.text_primary().to_egui(),
        );

        // 셸 chrome ② 종료 버튼 (상단 우측). **콘텐츠가 아니라 셸이 그린다** — 무대
        // 프레임에는 CSD 타이틀바가 없어 이 버튼이 유일한 마우스 탈출 수단이다.
        // 본체(`fullscreen.rs::draw_exit_button`)와 같은 위젯·같은 글리프를 쓴다:
        // ghost `IconButton`(md) + canonical `close`. hover 오버레이도 위젯이 낸다.
        let side = ControlSize::Md.height(theme);
        let exit = egui::Rect::from_min_size(
            egui::pos2(rect.right() - pad - side, rect.top() + pad),
            egui::Vec2::splat(side),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(exit), |ui| {
            IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .size(ControlSize::Md)
                .show(ui, theme, &|ui, rect, c| {
                    glyph::CLOSE.image(rect.height(), c).paint_at(ui, rect)
                });
        });

        // 콘텐츠 — 무대가 콘텐츠에 주는 rect(제목 띠 아래, 바깥 여백만큼 안쪽).
        let content = egui::Rect::from_min_max(
            egui::pos2(
                rect.left() + pad,
                rect.top() + pad + theme.font_size_heading.value() + theme.spacing_lg.value(),
            ),
            egui::pos2(rect.right() - pad, rect.bottom() - pad),
        );
        painter.rect_filled(
            content,
            theme.corner_radius.value(),
            theme.surface_raised().to_egui(),
        );
        painter.rect_stroke(
            content,
            theme.corner_radius.value(),
            egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
            egui::StrokeKind::Inside,
        );
        // 콘텐츠는 popup 과 **같은 형상 함수**가 채운다 — 타이틀바는 다시 그리지
        // 않는다(제목·닫기는 셸 chrome 의 몫이라 겹친다).
        let row_h = theme.item_height_interactive.value();
        let mut y = content.top() + theme.spacing_xs.value();
        for i in 0..3 {
            let row = egui::Rect::from_min_size(
                egui::pos2(content.left() + theme.spacing_xs.value(), y),
                egui::vec2(content.width() - theme.spacing_xs.value() * 2.0, row_h),
            );
            painter.rect_filled(
                row,
                theme.corner_radius_sm.value(),
                theme.surface_hover().to_egui(),
            );
            painter.text(
                egui::pos2(row.left() + theme.spacing_sm.value(), row.center().y),
                egui::Align2::LEFT_CENTER,
                format!("Build {} finished", i + 1),
                egui::FontId::proportional(theme.font_size_body.value()),
                theme.text_primary().to_egui(),
            );
            y += row_h + theme.spacing_xs.value();
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("shell", "scrim + centered title + exit button"),
            ("exit", "shell-owned ghost IconButton (md) · close glyph"),
            ("content", "below the title band, inset by space-xl"),
            ("frame", "surface-raised + 1px border-strong"),
            (
                "title bar",
                "not repeated — the shell already titles the stage",
            ),
        ],
        &[
            TokenChip::new("scrim", "backdrop", theme.scrim().to_egui()),
            TokenChip::new(
                "surface-raised",
                "content frame",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "border-strong",
                "frame border",
                theme.border_strong().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The stage is not an enlarged popup — it is a separate instance of the same shape, and \
         the original popup stays open behind it. The stage frame drops the window's own title \
         bar, so the shell always draws the exit button: a stage with no way out would trap the \
         window.",
    );
}

/// popup 타이틀바 — 전체화면 버튼이 붙은 것과 붙지 않은 것.
pub fn draw_titlebar(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for (title, buttons, label) in [
            (
                "Notifications",
                TitleButtons::FULLSCREEN_AND_CLOSE,
                "declares a stage → fullscreen + close",
            ),
            (
                "Rename workspace",
                TitleButtons::CLOSE,
                "no stage → close only (unchanged)",
            ),
        ] {
            ui.label(
                egui::RichText::new(label)
                    .size(theme.font_size_micro.value())
                    .color(theme.text_muted().to_egui()),
            );
            host_title_bar(ui, theme, title, buttons);
            ui.add_space(theme.spacing_md.value());
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("bar", "28px · bg-sidebar · 1px border-strong hairline"),
            ("title", "centered, elided against the button cluster"),
            ("buttons", "20px square · 4px gap · 4px from the right edge"),
            ("hover", "overlay-hover fill + text-primary glyph"),
            ("priority", "buttons sit above the drag handle they overlap"),
        ],
        &[
            TokenChip::new("bg-sidebar", "title bar", theme.bg_sidebar().into()),
            TokenChip::new("text-muted", "glyph rest", theme.text_muted().to_egui()),
            TokenChip::new(
                "overlay-hover",
                "button hover",
                theme.hover_overlay.to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Only a popup that declares a stage gets the fullscreen button; every other title bar is \
         byte-identical to before, because the close button keeps its own position and the title's \
         available width is measured from whichever button sits leftmost.",
    );
}

/// 본체 `PopupManager` 가 그리는 타이틀바 구조 그대로 — 배경 + hairline + 가운데
/// 제목 + 우측 버튼군. 갤러리 공통 헬퍼 [`popup_frame::draw_title_buttons`] 가 버튼을
/// 그린다(본체와 같은 rect 산술).
fn host_title_bar(ui: &mut egui::Ui, theme: &Theme, title: &str, buttons: TitleButtons) {
    let width = 320.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, popup_frame::TITLE_BAR_HEIGHT.value()),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let cr = theme.corner_radius.value() as u8;
    painter.rect_filled(
        rect,
        egui::CornerRadius {
            nw: cr,
            ne: cr,
            sw: 0,
            se: 0,
        },
        theme.bg_sidebar(),
    );
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y),
            egui::pos2(rect.max.x, rect.max.y),
        ],
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
    );
    let buttons_left = popup_frame::draw_title_buttons(&painter, theme, rect, buttons);
    let pad = theme.spacing_sm.value();
    let avail = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + pad, rect.min.y),
        egui::pos2((buttons_left - pad).max(rect.min.x + pad), rect.max.y),
    );
    painter.with_clip_rect(avail).text(
        avail.center(),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_primary().to_egui(),
    );
}
