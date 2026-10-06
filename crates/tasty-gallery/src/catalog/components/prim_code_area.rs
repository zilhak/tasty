//! 줄 번호 거터가 있는 여러 줄 고정폭 입력(CodeArea) 예제. 시안 forms 카드의 오류 줄 표본과
//! 빈 입력·비활성 상태를 그린다. 편집 내용은 예제마다 별도 버퍼에 보관한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::CodeArea;

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

/// 시안 forms 카드의 CodeArea 칸 폭(`width: 420`). 공개 역할 토큰이 없어 갤러리 무대 치수로 둔다.
const CARD_WIDTH: LogicalPx = LogicalPx(420.0);

thread_local! {
    // 예제 초깃값이 입력 중인 내용을 덮지 않도록 한 번만 초기화한다.
    static BUFS: RefCell<Option<[String; 3]>> = const { RefCell::new(None) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let w = CARD_WIDTH.value();
    BUFS.with(|b| {
        let mut slot = b.borrow_mut();
        let bufs = slot.get_or_insert_with(|| {
            [
                "system.info\nnotification.send {\"title\": \"x\"".to_string(),
                String::new(),
                "system.info".to_string(),
            ]
        });
        stage(ui, theme, StageVariant::Column, |ui| {
            cluster(
                ui,
                theme,
                "errorLine 2 · minRows 3 — click to edit",
                |ui| {
                    ui.scope(|ui| {
                        ui.set_width(w);
                        CodeArea::new("gallery_codearea_error")
                            .min_rows(3)
                            .error_line(Some(2))
                            .show(ui, theme, &mut bufs[0]);
                    });
                },
            );
            cluster(ui, theme, "empty · disabled", |ui| {
                ui.scope(|ui| {
                    ui.set_width(w);
                    CodeArea::new("gallery_codearea_empty")
                        .placeholder("system.info")
                        .show(ui, theme, &mut bufs[1]);
                });
                ui.scope(|ui| {
                    ui.set_width(w);
                    CodeArea::new("gallery_codearea_disabled")
                        .min_rows(2)
                        .enabled(false)
                        .show(ui, theme, &mut bufs[2]);
                });
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            (
                "box",
                "same as Input — surface-raised · border-default · radius · focus edge + ring · invalid · disabled",
            ),
            ("text", "mono codearea-font-size · line-height-ui · no wrap"),
            (
                "padding",
                "codearea-padding-y · codearea-padding-x; gutter space-xs each side",
            ),
            (
                "gutter",
                "min codearea-gutter-width · bg codearea-gutter-bg · fg codearea-gutter-fg · right edge codearea-gutter-border · pinned on horizontal scroll",
            ),
            (
                "errorLine",
                "one line: gutter number codearea-error-fg + tint-fill-alpha danger band; implies invalid",
            ),
            (
                "height",
                "grows with lines to codearea-max-height, then scrolls",
            ),
            (
                "keys",
                "Enter newline · submit / cancel keys come from the caller (the app reads code_area_apply / code_area_cancel)",
            ),
        ],
        &[
            TokenChip::new(
                "codearea-gutter-bg",
                "→ bg-sidebar",
                theme.codearea_gutter_bg().to_egui(),
            ),
            TokenChip::new(
                "codearea-gutter-fg",
                "→ text-muted",
                theme.codearea_gutter_fg().to_egui(),
            ),
            TokenChip::new(
                "codearea-gutter-border",
                "→ separator",
                theme.codearea_gutter_border().to_egui_premultiplied(),
            ),
            TokenChip::new(
                "codearea-error-fg",
                "→ accent-danger",
                theme.codearea_error_fg().to_egui(),
            ),
            TokenChip::without_color("codearea-font-size", "→ font-size-caption"),
            TokenChip::without_color("codearea-gutter-width", "→ space-xl"),
            TokenChip::without_color("codearea-max-height", "→ size-200"),
        ],
    );
}
