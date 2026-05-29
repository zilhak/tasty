use crate::i18n::t;

/// Misc / 기타 탭: 좌측 서브탭 메뉴 + 우측 콘텐츠.
pub fn draw_misc_tab(
    ui: &mut egui::Ui,
    sub_tab: &mut crate::settings_ui::MiscSubTab,
    bashrc_user_draft: &mut Option<String>,
) {
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    ui.horizontal_top(|ui| {
        egui::Frame::new()
            .fill(th.crust.into())
            .stroke(egui::Stroke::new(1.0, th.surface0))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 6))
            .show(ui, |ui| {
                ui.set_width(100.0);
                ui.set_min_height(available_height);

                ui.vertical(|ui| {
                    let sub_tabs = [(
                        crate::settings_ui::MiscSubTab::Tastyrc,
                        t("settings.misc.subtab.tastyrc"),
                    )];

                    for (tab, label) in &sub_tabs {
                        let selected = *sub_tab == *tab;
                        if ui.selectable_label(selected, *label).clicked() {
                            *sub_tab = *tab;
                        }
                    }
                });
            });

        ui.add_space(8.0);

        ui.vertical(|ui| match *sub_tab {
            crate::settings_ui::MiscSubTab::Tastyrc => draw_tastyrc_subtab(ui, bashrc_user_draft),
        });
    });
}

/// tastyrc 서브탭: Tasty 모드에서 적용되는 bashrc 사용자 영역 편집.
fn draw_tastyrc_subtab(ui: &mut egui::Ui, bashrc_user_draft: &mut Option<String>) {
    let th = crate::theme::theme();

    ui.heading(t("settings.misc.bashrc.heading"));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.misc.bashrc.description"))
            .small()
            .color(th.subtext0),
    );
    ui.add_space(8.0);

    // draft는 mod.rs 진입부에서 lazy 로드되므로 이 시점엔 항상 Some.
    let draft = bashrc_user_draft.get_or_insert_with(crate::settings::general::load_user_bashrc);

    ui.horizontal(|ui| {
        if ui.button(t("settings.misc.bashrc.reset_button")).clicked() {
            *draft = crate::settings::general::INITIAL_USER_BASHRC.to_string();
        }
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(draft)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(20)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
        });
}
