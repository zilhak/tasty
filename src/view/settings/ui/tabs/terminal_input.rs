//! Terminal input uses the existing application-list editor and settings switch.
use crate::adapters::ui::icons;
use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Input, switch, vspace,
};

pub fn draw_terminal_input_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);
    ui.label(t("settings.terminal.input_description"));
    vspace(ui, th.spacing_md);

    if settings.terminal_input.rules.is_empty() {
        ui.label(egui::RichText::new(t("settings.terminal.input_empty")).color(th.text_muted()));
    }
    let mut remove = None;
    for (index, rule) in settings.terminal_input.rules.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&rule.app)
                        .monospace()
                        .color(th.text_primary()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(ControlSize::Sm)
                        .show(ui, &th, &|ui, rect, color| {
                            icons::CLOSE.image(rect.height(), color).paint_at(ui, rect);
                        })
                        .on_hover_text(t("settings.terminal.input_remove"))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                    switch(ui, &th, &mut rule.shift_enter_newline, None, true);
                    ui.label(t("settings.terminal.input_newline"));
                });
            });
        });
        vspace(ui, th.spacing_sm);
    }
    if let Some(index) = remove {
        settings.terminal_input.rules.remove(index);
    }

    vspace(ui, th.spacing_sm);
    let add_id = ui.id().with("terminal_input_add");
    let mut app = ui.data_mut(|data| data.get_temp::<String>(add_id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let normalized = tasty_settings::terminal_input::normalize_app(&app);
            let valid = !normalized.is_empty()
                && !normalized.chars().any(char::is_control)
                && !settings.terminal_input.rules.iter().any(|rule| {
                    tasty_settings::terminal_input::normalize_app(&rule.app) == normalized
                });
            let add = Button::new(t("settings.terminal.input_add"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .enabled(valid)
                .show(ui, &th)
                .clicked();
            let response = Input::new()
                .placeholder(t("settings.terminal.input_placeholder"))
                .mono(true)
                .show(ui, &th, &mut app);
            let submit =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if valid && (add || submit) {
                match settings.terminal_input.set_rule(&app, true) {
                    Ok(()) => app.clear(),
                    Err(error) => tracing::warn!("invalid terminal input rule: {error}"),
                }
            }
        });
    });
    ui.data_mut(|data| data.insert_temp(add_id, app));
    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(t("settings.terminal.input_notice"))
            .small()
            .color(th.text_muted()),
    );
}
