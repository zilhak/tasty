//! Select · Checkbox · Switch primitive specimen — 디자인 gallery 대조.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{checkbox, select, switch};

thread_local! {
    static STATE: RefCell<FormState> = const {
        RefCell::new(FormState { sel: 0, check_a: true, check_b: false, switch_a: true, switch_b: false })
    };
}

struct FormState {
    sel: usize,
    check_a: bool,
    check_b: bool,
    switch_a: bool,
    switch_b: bool,
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(egui::Color32::from(theme.subtext0)),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
    STATE.with(|s| {
        let mut st = s.borrow_mut();

        caption(ui, theme, "Select — 토큰 트리거 + 드롭다운");
        let opts = ["Mocha (dark)", "Latte (light)", "Auto"];
        select(ui, theme, "gallery_theme", &mut st.sel, &opts, 200.0, true);

        ui.add_space(10.0);
        caption(ui, theme, "Checkbox — checked · unchecked · disabled");
        ui.horizontal(|ui| {
            checkbox(ui, theme, &mut st.check_a, "Restore layout", true);
            checkbox(ui, theme, &mut st.check_b, "Confirm on close", true);
            let mut off = false;
            checkbox(ui, theme, &mut off, "Disabled", false);
        });

        ui.add_space(10.0);
        caption(ui, theme, "Switch — on · off · disabled");
        ui.horizontal(|ui| {
            switch(ui, theme, &mut st.switch_a, Some("Reduced motion"), true);
            switch(ui, theme, &mut st.switch_b, Some("High contrast"), true);
            let mut off = false;
            switch(ui, theme, &mut off, Some("Disabled"), false);
        });
    });
}
