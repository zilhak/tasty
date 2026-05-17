//! Settings UI 의 `FileHandler` 탭 (Phase D MD3 의 일부).
//!
//! Phase E ME4 단계에서는 **Extension Mapping** sub-tab 만 구현. 다른 sub-tab
//! (Detectors / Handlers / Recent picks) 은 MD3 본 작업에서 추가될 예정이며 현재는
//! 안내 문구만 표시.
//!
//! 사용자가 같은 확장자를 광고하는 여러 detector 의 우선순위 (`extension_priority` 표)
//! 를 직접 조정할 수 있게 한다. draft 는 `SettingsUiState.extension_priority_draft` 에
//! 보관, Save 클릭 시 registry 에 commit + `~/.tasty/file-handlers.toml` 에 atomic write.

use std::collections::BTreeMap;

use crate::file_format::{DetectorId, DetectorInfo, FileFormatRegistry};
use crate::i18n::t;

/// FileHandler 탭의 sub-tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileHandlerSubTab {
    Detectors,
    Handlers,
    ExtensionMapping,
    RecentPicks,
}

pub(crate) fn draw_file_handler_tab(
    ui: &mut egui::Ui,
    sub_tab: &mut FileHandlerSubTab,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    new_ext_input: &mut String,
    file_format: &FileFormatRegistry,
) {
    ui.horizontal(|ui| {
        for (tab, label_key) in [
            (
                FileHandlerSubTab::Detectors,
                "settings.file_handler.sub.detectors",
            ),
            (
                FileHandlerSubTab::Handlers,
                "settings.file_handler.sub.handlers",
            ),
            (
                FileHandlerSubTab::ExtensionMapping,
                "settings.file_handler.sub.extension_mapping",
            ),
            (
                FileHandlerSubTab::RecentPicks,
                "settings.file_handler.sub.recent_picks",
            ),
        ] {
            let selected = *sub_tab == tab;
            if ui.selectable_label(selected, t(label_key)).clicked() {
                *sub_tab = tab;
            }
        }
    });
    ui.separator();

    match *sub_tab {
        FileHandlerSubTab::ExtensionMapping => {
            draw_extension_mapping(ui, draft, new_ext_input, file_format)
        }
        FileHandlerSubTab::Detectors
        | FileHandlerSubTab::Handlers
        | FileHandlerSubTab::RecentPicks => {
            ui.add_space(8.0);
            ui.label(t("settings.file_handler.coming_soon"));
        }
    }
}

/// Extension Mapping sub-tab. 광고된 모든 확장자 + draft 에 있는 확장자를 리스트로 표시,
/// 각 확장자 옆에 후보 detector 들을 ↑↓ 버튼으로 재정렬 가능.
fn draw_extension_mapping(
    ui: &mut egui::Ui,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    new_ext_input: &mut String,
    file_format: &FileFormatRegistry,
) {
    // 초기 진입 시 registry 의 현재 priority 표를 draft 로 복사.
    if draft.is_none() {
        let mut map = BTreeMap::new();
        for ext in file_format.extension_priority_keys() {
            if let Some(order) = file_format.extension_priority_order(&ext) {
                map.insert(ext, order);
            }
        }
        *draft = Some(map);
    }
    let draft_map = draft.as_mut().expect("draft initialized above");

    ui.add_space(4.0);
    ui.label(t("settings.file_handler.extension_mapping.description"));
    ui.add_space(8.0);

    // 표시할 확장자 = (draft 의 확장자) ∪ (모든 광고 확장자 중 1개 이상 candidate 가 있는 것).
    // candidate 가 2개 이상인 경우만 priority 의미가 있으므로 실제 노출은 후자 위주.
    let all_exts = file_format.all_advertised_extensions();
    let mut visible: std::collections::BTreeSet<String> =
        draft_map.keys().cloned().collect();
    for ext in &all_exts {
        let candidates = file_format.detectors_for_extension(ext);
        if candidates.len() >= 2 {
            visible.insert(ext.clone());
        }
    }

    if visible.is_empty() {
        ui.label(t("settings.file_handler.extension_mapping.no_conflicts"));
    } else {
        for ext in &visible {
            draw_extension_row(ui, ext, draft_map, file_format);
            ui.add_space(4.0);
        }
    }

    ui.add_space(12.0);
    ui.separator();
    ui.add_space(4.0);

    // 새 확장자 수동 추가 (자동완성 dropdown 대신 단순 textbox + suggestion 라벨).
    ui.horizontal(|ui| {
        ui.label(t("settings.file_handler.extension_mapping.add_label"));
        ui.add(
            egui::TextEdit::singleline(new_ext_input)
                .hint_text(".md")
                .desired_width(120.0),
        );
        let normalized = new_ext_input
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let enabled = !normalized.is_empty()
            && !file_format.detectors_for_extension(&normalized).is_empty();
        if ui
            .add_enabled(enabled, egui::Button::new(t("button.add")))
            .clicked()
        {
            let order = file_format.detectors_for_extension(&normalized);
            if !order.is_empty() {
                draft_map.entry(normalized.clone()).or_insert(order);
            }
            new_ext_input.clear();
        }
    });
}

/// 한 확장자 행. 좌측에 확장자 라벨, 우측에 후보 detector 들의 ↑↓ 컨트롤.
fn draw_extension_row(
    ui: &mut egui::Ui,
    ext: &str,
    draft_map: &mut BTreeMap<String, Vec<DetectorId>>,
    file_format: &FileFormatRegistry,
) {
    let candidates = file_format.detectors_for_extension(ext);
    if candidates.is_empty() {
        // 광고 detector 가 없는 확장자 — draft 에만 남아있는 (예: plugin 제거 후) 경우.
        // 사용자가 "지우기" 할 수 있게 행은 표시하되 회색으로.
        egui::Frame::new()
            .inner_margin(egui::vec2(8.0, 6.0))
            .fill(ui.visuals().faint_bg_color)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!(".{}", ext));
                    ui.label(t("settings.file_handler.extension_mapping.unregistered"));
                    if ui
                        .button(t("settings.file_handler.extension_mapping.clear"))
                        .clicked()
                    {
                        draft_map.insert(ext.to_string(), Vec::new());
                    }
                });
            });
        return;
    }

    // draft 에 entry 가 있으면 그것을 표시 순서로, 없으면 registry 의 install_order 순.
    // draft 의 detector 중 candidates 에 없는 것 (plugin 제거 등) 은 회색 표시.
    let order: Vec<DetectorId> = if let Some(d) = draft_map.get(ext) {
        let mut result: Vec<DetectorId> = d.clone();
        for c in &candidates {
            if !result.contains(c) {
                result.push(c.clone());
            }
        }
        result
    } else {
        candidates.clone()
    };

    egui::Frame::new()
        .inner_margin(egui::vec2(8.0, 6.0))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!(".{}", ext));
                ui.add_space(8.0);
                if draft_map.contains_key(ext) {
                    if ui
                        .small_button(t("settings.file_handler.extension_mapping.reset"))
                        .on_hover_text(t(
                            "settings.file_handler.extension_mapping.reset_tooltip",
                        ))
                        .clicked()
                    {
                        // reset = 표에서 entry 제거 (default install_order 순서로 복귀).
                        // 빈 Vec 으로 marker — Save 시 clear 호출됨.
                        draft_map.insert(ext.to_string(), Vec::new());
                    }
                }
            });
            ui.add_space(4.0);
            let len = order.len();
            let mut move_up: Option<usize> = None;
            let mut move_down: Option<usize> = None;
            for (i, id) in order.iter().enumerate() {
                let in_candidates = candidates.iter().any(|c| c == id);
                ui.horizontal(|ui| {
                    let up_enabled = i > 0 && in_candidates;
                    let down_enabled = i + 1 < len && in_candidates;
                    if ui
                        .add_enabled(up_enabled, egui::Button::new("▲"))
                        .clicked()
                    {
                        move_up = Some(i);
                    }
                    if ui
                        .add_enabled(down_enabled, egui::Button::new("▼"))
                        .clicked()
                    {
                        move_down = Some(i);
                    }
                    if in_candidates {
                        ui.label(id.as_str());
                        if !file_format.is_enabled(id) {
                            ui.weak(format!(
                                "({})",
                                t("settings.file_handler.extension_mapping.disabled")
                            ));
                        }
                    } else {
                        ui.weak(format!(
                            "{} ({})",
                            id.as_str(),
                            t("settings.file_handler.extension_mapping.unregistered")
                        ));
                    }
                });
            }
            if let Some(i) = move_up {
                let mut new_order = order.clone();
                new_order.swap(i - 1, i);
                draft_map.insert(ext.to_string(), new_order);
            } else if let Some(i) = move_down {
                let mut new_order = order.clone();
                new_order.swap(i, i + 1);
                draft_map.insert(ext.to_string(), new_order);
            }
        });
}

