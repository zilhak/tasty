use crate::i18n::t;
use crate::settings::Settings;

use super::{draw_accessibility_tab, draw_performance_tab};

/// Misc / 기타 탭: 좌측 서브탭 메뉴 + 우측 콘텐츠.
///
/// 외관/단축키 탭과 동일한 `tasty_ui_widgets::two_depth_layout` 패턴.
/// tastyrc 서브탭만 Windows 전용이며, 접근성/성능은 OS 무관 노출된다.
pub fn draw_misc_tab(
    ui: &mut egui::Ui,
    sub_tab: &mut crate::settings_ui::MiscSubTab,
    settings: &mut Settings,
    // 비-Windows 빌드에서는 Tastyrc 서브탭이 매뉴에 없으므로 draft 도 안 쓰인다.
    #[cfg_attr(not(windows), allow(unused_variables))] bashrc_user_draft: &mut Option<String>,
) {
    use crate::settings_ui::MiscSubTab;
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    let sub_tabs: Vec<(MiscSubTab, String)> = vec![
        #[cfg(windows)]
        (
            MiscSubTab::Tastyrc,
            t("settings.misc.subtab.tastyrc").to_string(),
        ),
        (
            MiscSubTab::Accessibility,
            t("settings.misc.subtab.accessibility").to_string(),
        ),
        (
            MiscSubTab::Performance,
            t("settings.misc.subtab.performance").to_string(),
        ),
    ];

    // 비-Windows 빌드에서 misc_sub_tab 이 Tastyrc 로 남아 있는 경우 (예: 설정 직렬화에서
    // 복원되거나 cfg 분기 누락) 첫 노출 항목으로 fallback — 우측이 빈 화면이 되지 않도록.
    #[cfg(not(windows))]
    if *sub_tab == MiscSubTab::Tastyrc {
        *sub_tab = MiscSubTab::Accessibility;
    }

    let current = *sub_tab;
    let mut selected_new: Option<MiscSubTab> = None;
    tasty_ui_widgets::two_depth_layout(
        ui,
        &th,
        available_height,
        |ui| {
            for (tab, label) in &sub_tabs {
                let selected = current == *tab;
                if ui.selectable_label(selected, label.as_str()).clicked() {
                    selected_new = Some(*tab);
                }
            }
        },
        |ui| match current {
            #[cfg(windows)]
            MiscSubTab::Tastyrc => draw_tastyrc_subtab(ui, bashrc_user_draft),
            #[cfg(not(windows))]
            // 도달 불가: sub_tabs 에 push 되지 않고 위에서 Accessibility 로 fallback 됨.
            MiscSubTab::Tastyrc => {}
            MiscSubTab::Accessibility => draw_accessibility_tab(ui, settings),
            MiscSubTab::Performance => draw_performance_tab(ui, settings),
        },
    );
    if let Some(new) = selected_new {
        *sub_tab = new;
    }
}

/// tastyrc 서브탭: Tasty 모드에서 적용되는 bashrc 사용자 영역 편집.
#[cfg(windows)]
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
