//! UI 입력란 캐럿 예제. 캐럿은 입력란과 창이 모두 포커스를 가질 때만 그려지므로 정지 화면에는
//! 나오지 않는다. 입력란을 눌러 깜박이지 않는 캐럿을 확인한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Input;

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

thread_local! {
    static BUF: RefCell<String> = RefCell::new("open settings".to_string());
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "click to focus — the caret stays on", |ui| {
            BUF.with(|b| {
                Input::new().width(theme.field_width_lg.value()).show(
                    ui,
                    theme,
                    &mut b.borrow_mut(),
                );
            });
        });
    });
    let caret = ui.visuals().text_cursor.stroke.color;
    meta(
        ui,
        theme,
        &[
            ("blink", "off · egui text_cursor.blink = false"),
            ("width", "1 · text-primary ink"),
            ("shown", "field focused AND window focused"),
            ("cost", "no idle repaint while a field is focused"),
        ],
        &[
            TokenChip::new("text-primary", "caret ink", caret),
            TokenChip::without_color("border-width", "caret width (1)"),
        ],
    );
}
