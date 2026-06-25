use std::collections::HashMap;

use crate::i18n::t;
use crate::settings::Settings;
use tasty_host_plugin::SettingsPageEntry;
use tasty_plugin_manifest::SettingsCategory;

use super::appearance::{draw_plugin_settings_page, find_plugin_settings_entry};

/// Plugin 탭. `[[contributes.settings_pages]]` 의 `category = "plugin"` 항목을
/// 좌측 sub-tab 으로 합성한다. 등록된 page 가 0 개면 좌측 비고 우측에 안내
/// 메시지만 표시.
pub fn draw_plugin_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    sub_tab: &mut Option<crate::settings_ui::PluginSubTab>,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
    settings_pages: &[SettingsPageEntry],
    l2_filter: &mut String,
) {
    use crate::settings_ui::PluginSubTab;
    let th = crate::theme::theme();
    ui.add_space(8.0);

    let available_height = ui.available_height() - 8.0 - 14.0;

    let sub_tabs: Vec<(PluginSubTab, String)> = settings_pages
        .iter()
        .filter(|e| e.page.category == SettingsCategory::Plugin)
        .map(|entry| {
            (
                PluginSubTab::Plugin {
                    plugin_id: entry.plugin_id.clone(),
                    page_id: entry.page.id.clone(),
                },
                t(&entry.page.title_key).to_string(),
            )
        })
        .collect();

    // 현재 활성 sub_tab 이 가리키는 page 가 사라졌다면 (plugin disable 등)
    // None 으로 리셋해 우측이 안내 메시지로 fallback 되도록 한다.
    if let Some(PluginSubTab::Plugin {
        plugin_id: active_plugin,
        page_id: active_page,
    }) = sub_tab.as_ref()
        && !sub_tabs.iter().any(|(tab, _)| {
            matches!(
                tab,
                PluginSubTab::Plugin { plugin_id, page_id }
                    if plugin_id == active_plugin && page_id == active_page
            )
        })
    {
        *sub_tab = None;
    }

    let current = sub_tab.clone();
    let mut selected_new: Option<PluginSubTab> = None;
    let filter_lc = l2_filter.to_lowercase();
    tasty_ui_widgets::two_depth_layout_filtered(
        ui,
        &th,
        available_height,
        l2_filter,
        t("settings.filter.plugins"),
        |ui| {
            let mut any = false;
            for (tab, label) in &sub_tabs {
                if !filter_lc.is_empty() && !label.to_lowercase().contains(&filter_lc) {
                    continue;
                }
                any = true;
                let selected = current.as_ref() == Some(tab);
                if ui.selectable_label(selected, label.as_str()).clicked() {
                    selected_new = Some(tab.clone());
                }
            }
            if !any {
                ui.label(egui::RichText::new(t("settings.filter.no_matches")).color(th.subtext0));
            }
        },
        |ui| match current.as_ref() {
            Some(PluginSubTab::Plugin { plugin_id, page_id }) => {
                if let Some(entry) = find_plugin_settings_entry(settings_pages, plugin_id, page_id)
                {
                    draw_plugin_settings_page(
                        ui,
                        settings,
                        font_families,
                        font_filter,
                        preview_font_loaded,
                        plugin_id,
                        &entry.page,
                    );
                } else {
                    ui.label(t("settings.plugin.empty"));
                }
            }
            None => {
                ui.label(t("settings.plugin.empty"));
            }
        },
    );
    if let Some(new) = selected_new {
        *sub_tab = Some(new);
    }
}

#[cfg(test)]
mod tests {
    use crate::settings_ui::{PluginSubTab, SettingsUiState};

    #[test]
    fn plugin_sub_tab_default_is_none() {
        let s = SettingsUiState::new();
        assert!(s.plugin_sub_tab.is_none());
    }

    #[test]
    fn plugin_sub_tab_variant_uses_composite_key() {
        // 동일 page_id 라도 plugin_id 가 다르면 다른 variant 로 식별.
        let a = PluginSubTab::Plugin {
            plugin_id: "alpha".into(),
            page_id: "main".into(),
        };
        let b = PluginSubTab::Plugin {
            plugin_id: "beta".into(),
            page_id: "main".into(),
        };
        assert_ne!(a, b);
    }
}
