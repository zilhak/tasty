//! Settings UI 의 `FileHandler` 탭 (Phase D MD3 + Phase E ME4).
//!
//! 4 개의 sub-tab:
//! - **Detectors** — 등록된 모든 detector 의 read-only 목록 (origin, rule 종류, disabled 표시).
//!   추가/편집 modal 은 MD3 후속 단계로 보류.
//! - **Handlers** — 등록된 handler 목록 (priority, detector, action, owner, disabled).
//!   read-only. 편집 modal 은 후속.
//! - **Extension Mapping** — 같은 확장자를 광고하는 여러 detector 의 우선순위 표 편집.
//!   ↑/↓ 버튼 + 새 확장자 추가. Save 시 `~/.tasty/file-handlers.toml` atomic write.
//! - **Recent picks** — picker 가 기록한 LRU 목록 + Forget 액션. Forget 은 즉시 디스크 저장.

use std::collections::BTreeMap;

use crate::file_format::{DetectorId, DetectorInfo, DetectorRuleKind, FileFormatRegistry, RuleOrigin};
use crate::file_handler::{FileHandlerRegistry, HandlerAction, HandlerOwner};
use crate::file_handler_recent::RecentPicks;
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
    recent_picks: &mut Option<RecentPicks>,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
    recent_picks_path: Option<&std::path::Path>,
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
        FileHandlerSubTab::Detectors => draw_detectors(ui, file_format),
        FileHandlerSubTab::Handlers => draw_handlers(ui, file_handler),
        FileHandlerSubTab::RecentPicks => {
            draw_recent_picks(ui, recent_picks, recent_picks_path)
        }
    }
}

/// Detectors sub-tab — 등록된 모든 detector 의 read-only 목록.
fn draw_detectors(ui: &mut egui::Ui, file_format: &FileFormatRegistry) {
    ui.add_space(4.0);
    ui.label(t("settings.file_handler.detectors.description"));
    ui.add_space(8.0);

    let ids = file_format.list_detectors();
    if ids.is_empty() {
        ui.label(t("settings.file_handler.detectors.empty"));
        return;
    }

    egui::Grid::new("file_handler_detectors_grid")
        .num_columns(4)
        .striped(true)
        .spacing(egui::vec2(12.0, 4.0))
        .show(ui, |ui| {
            ui.strong(t("settings.file_handler.detectors.col_id"));
            ui.strong(t("settings.file_handler.detectors.col_origin"));
            ui.strong(t("settings.file_handler.detectors.col_rules"));
            ui.strong(t("settings.file_handler.detectors.col_status"));
            ui.end_row();

            for id in &ids {
                let Some(det) = file_format.detector(id) else {
                    continue;
                };
                ui.label(id.as_str());
                ui.label(detector_origins_summary(&det.rules));
                ui.label(rule_kinds_summary(&det.rules));
                if det.disabled {
                    ui.weak(t("settings.file_handler.common.disabled"));
                } else {
                    ui.label(t("settings.file_handler.common.enabled"));
                }
                ui.end_row();
            }
        });
}

/// Handlers sub-tab — 등록된 모든 handler 의 read-only 목록.
fn draw_handlers(ui: &mut egui::Ui, file_handler: &FileHandlerRegistry) {
    ui.add_space(4.0);
    ui.label(t("settings.file_handler.handlers.description"));
    ui.add_space(8.0);

    let ids = file_handler.list_handlers();
    if ids.is_empty() {
        ui.label(t("settings.file_handler.handlers.empty"));
        return;
    }

    // priority 오름차순 정렬 (낮을수록 우선).
    let mut rows: Vec<crate::file_handler::FileHandler> = ids
        .iter()
        .filter_map(|id| file_handler.handler(id))
        .collect();
    rows.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.id.as_str().cmp(b.id.as_str()))
    });

    egui::Grid::new("file_handler_handlers_grid")
        .num_columns(6)
        .striped(true)
        .spacing(egui::vec2(12.0, 4.0))
        .show(ui, |ui| {
            ui.strong(t("settings.file_handler.handlers.col_priority"));
            ui.strong(t("settings.file_handler.handlers.col_id"));
            ui.strong(t("settings.file_handler.handlers.col_owner"));
            ui.strong(t("settings.file_handler.handlers.col_detector"));
            ui.strong(t("settings.file_handler.handlers.col_action"));
            ui.strong(t("settings.file_handler.handlers.col_status"));
            ui.end_row();

            for h in &rows {
                ui.label(h.priority.to_string());
                ui.label(h.id.as_str());
                ui.label(handler_owner_label(&h.owner));
                ui.label(h.detector.as_str());
                ui.label(handler_action_summary(&h.action));
                if h.disabled {
                    ui.weak(t("settings.file_handler.common.disabled"));
                } else {
                    ui.label(t("settings.file_handler.common.enabled"));
                }
                ui.end_row();
            }
        });
}

/// Recent picks sub-tab — `~/.tasty/file-handler-recent.json` 의 LRU 목록 + Forget.
fn draw_recent_picks(
    ui: &mut egui::Ui,
    recent_picks: &mut Option<RecentPicks>,
    path: Option<&std::path::Path>,
) {
    // 첫 진입 시 디스크에서 로드. 경로가 없으면 (홈 없음) 빈 인스턴스로 시작.
    if recent_picks.is_none() {
        let picks = match path {
            Some(p) => RecentPicks::load(p),
            None => RecentPicks::default(),
        };
        *recent_picks = Some(picks);
    }
    let picks = recent_picks.as_mut().expect("loaded above");

    ui.add_space(4.0);
    ui.label(t("settings.file_handler.recent_picks.description"));
    ui.add_space(8.0);

    if picks.list().is_empty() {
        ui.label(t("settings.file_handler.recent_picks.empty"));
        return;
    }

    let entries: Vec<(crate::file_handler::HandlerId, i64)> = picks
        .list()
        .iter()
        .map(|e| (e.handler_id.clone(), e.last_used_at))
        .collect();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut to_forget: Option<crate::file_handler::HandlerId> = None;
    for (id, last_used) in &entries {
        ui.horizontal(|ui| {
            ui.label(id.as_str());
            ui.weak(format!("({})", format_relative_time(now_secs - last_used)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(t("settings.file_handler.recent_picks.forget"))
                    .clicked()
                {
                    to_forget = Some(id.clone());
                }
            });
        });
    }
    if let Some(id) = to_forget {
        if picks.forget(&id) {
            if let Some(p) = path {
                if let Err(e) = picks.save_atomic(p) {
                    tracing::warn!(
                        path = %p.display(),
                        error = %e,
                        "recent_picks: forget save failed",
                    );
                }
            }
        }
    }
}

fn detector_origins_summary(rules: &[crate::file_format::DetectorRule]) -> String {
    let mut origins: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for r in rules {
        let label = match &r.origin {
            RuleOrigin::HostDefault => "host".to_string(),
            RuleOrigin::Plugin(id) => format!("plugin:{}", id),
            RuleOrigin::User => "user".to_string(),
        };
        origins.insert(label);
    }
    if origins.is_empty() {
        "—".into()
    } else {
        origins.into_iter().collect::<Vec<_>>().join(", ")
    }
}

fn rule_kinds_summary(rules: &[crate::file_format::DetectorRule]) -> String {
    let mut kinds: Vec<&str> = rules
        .iter()
        .map(|r| match &r.kind {
            DetectorRuleKind::Extension { .. } => "ext",
            DetectorRuleKind::PathGlob { .. } => "glob",
            DetectorRuleKind::Mime { .. } => "mime",
            DetectorRuleKind::Magic { .. } => "magic",
            DetectorRuleKind::IsDirectory => "dir",
            DetectorRuleKind::Lua { .. } => "lua",
            DetectorRuleKind::StructureCheck { .. } => "structure",
            DetectorRuleKind::Unknown { .. } => "?",
        })
        .collect();
    kinds.sort();
    kinds.dedup();
    if kinds.is_empty() {
        "—".into()
    } else {
        kinds.join(", ")
    }
}

fn handler_owner_label(owner: &HandlerOwner) -> String {
    match owner {
        HandlerOwner::Host => "host".into(),
        HandlerOwner::Plugin(id) => format!("plugin:{}", id),
        HandlerOwner::User => "user".into(),
    }
}

fn handler_action_summary(action: &HandlerAction) -> String {
    match action {
        HandlerAction::OpenSurface { surface_kind, .. } => format!("surface:{}", surface_kind),
        HandlerAction::Ipc { method, .. } => format!("ipc:{}", method),
        HandlerAction::System => "system".into(),
    }
}

/// 상대 시간을 짧게 표시. UI 라벨이라 정밀도 < 가독성.
fn format_relative_time(secs_ago: i64) -> String {
    if secs_ago < 60 {
        return t("settings.file_handler.recent_picks.just_now").to_string();
    }
    if secs_ago < 3600 {
        return crate::i18n::t_fmt(
            "settings.file_handler.recent_picks.minutes_ago",
            &(secs_ago / 60).to_string(),
        );
    }
    if secs_ago < 86400 {
        return crate::i18n::t_fmt(
            "settings.file_handler.recent_picks.hours_ago",
            &(secs_ago / 3600).to_string(),
        );
    }
    crate::i18n::t_fmt(
        "settings.file_handler.recent_picks.days_ago",
        &(secs_ago / 86400).to_string(),
    )
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

