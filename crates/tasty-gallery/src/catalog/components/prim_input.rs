//! 입력 필드의 아이콘·단위·글꼴·오류·비활성 상태 예제.
//! 편집 내용은 예제마다 별도 버퍼에 보관한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Input;

use super::glyph;
use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

thread_local! {
    // 예제 초깃값이 입력 중인 내용을 덮지 않도록 한 번만 초기화한다.
    static BUFS: RefCell<Option<[String; 6]>> = const { RefCell::new(None) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let xs = theme.field_width_xs.value();
    let md = theme.field_width_md.value();

    BUFS.with(|b| {
        let mut slot = b.borrow_mut();
        let bufs = slot.get_or_insert_with(|| {
            [
                String::new(),
                String::new(),
                "14".to_string(),
                "s_01HXK9".to_string(),
                "bad value".to_string(),
                String::new(),
            ]
        });
        stage(ui, theme, StageVariant::Column, |ui| {
            cluster(
                ui,
                theme,
                "default · icon · addon — click to focus",
                |ui| {
                    Input::new().placeholder("Workspace name").width(md).show(
                        ui,
                        theme,
                        &mut bufs[0],
                    );
                    Input::new()
                        .placeholder("Filter…")
                        .width(md)
                        .icon(&|ui, rect, c| {
                            glyph::SEARCH.image(rect.height(), c).paint_at(ui, rect)
                        })
                        .show(ui, theme, &mut bufs[1]);
                    Input::new()
                        .mono(true)
                        .addon("px")
                        .width(xs)
                        .show(ui, theme, &mut bufs[2]);
                },
            );
            cluster(ui, theme, "mono · invalid · disabled", |ui| {
                Input::new()
                    .mono(true)
                    .width(md)
                    .show(ui, theme, &mut bufs[3]);
                Input::new()
                    .invalid(true)
                    .width(md)
                    .show(ui, theme, &mut bufs[4]);
                Input::new()
                    .enabled(false)
                    .placeholder("Disabled")
                    .width(md)
                    .show(ui, theme, &mut bufs[5]);
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("height", "28 control-height"),
            ("border", "1px → focus 2px"),
            ("padding", "0 space-sm"),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "field fill",
                egui::Color32::from(theme.surface_raised()),
            ),
            TokenChip::new(
                "border-focus",
                "focus ring",
                egui::Color32::from(theme.border_focus()),
            ),
            TokenChip::new(
                "accent-danger",
                "invalid edge",
                egui::Color32::from(theme.accent_danger()),
            ),
            TokenChip::new(
                "text-placeholder",
                "placeholder",
                egui::Color32::from(theme.text_placeholder()),
            ),
        ],
    );
}
