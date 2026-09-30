use crate::runtime::file_catalog::FormatCatalog as FileFormatRegistry;
use std::collections::BTreeMap;

use crate::file::format::{DetectorId, DetectorInfo};
use crate::i18n::t;

use super::draw_intro_block;
use crate::adapters::ui::icons;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Input, hspace, tag_disabled,
    vspace,
};

/// 등록된 확장자와 편집 중인 확장자를 표시하고 detector 우선순위를 조정한다.
pub(super) fn draw_extension_mapping(
    ui: &mut egui::Ui,
    draft: &mut Option<BTreeMap<String, Vec<DetectorId>>>,
    new_ext_input: &mut String,
    file_format: &FileFormatRegistry,
) {
    let th = crate::theme::theme();
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

    draw_intro_block(
        ui,
        "settings.file_handler.extension_mapping.description",
        &[
            "settings.file_handler.extension_mapping.bullet_when",
            "settings.file_handler.extension_mapping.bullet_visibility",
            "settings.file_handler.extension_mapping.bullet_actions",
            "settings.file_handler.extension_mapping.bullet_unregistered",
        ],
    );

    // 새 확장자 수동 추가 — Input + Add(Button secondary sm). 입력이 비었거나 그 확장자를
    // 지원하는 detector가 없으면 Add는 disabled다.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let normalized = new_ext_input
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let enabled =
            !normalized.is_empty() && !file_format.detectors_for_extension(&normalized).is_empty();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let add = Button::new(t("button.add"))
                .variant(ButtonVariant::Secondary)
                .size(ControlSize::Sm)
                .enabled(enabled)
                .show(ui, &th);
            let placeholder = t("settings.file_handler.extension_mapping.add_placeholder");
            let input = Input::new()
                .mono(true)
                .placeholder(placeholder)
                .width(ui.available_width())
                .show(ui, &th, new_ext_input);
            let submitted = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enabled && (add.clicked() || submitted) {
                let order = file_format.detectors_for_extension(&normalized);
                draft_map.entry(normalized.clone()).or_insert(order);
                new_ext_input.clear();
            }
        });
    });
    vspace(ui, th.spacing_md);

    // 초안의 확장자와 등록된 후보가 있는 확장자를 함께 표시한다.
    let all_exts = file_format.all_advertised_extensions();
    let mut visible: std::collections::BTreeSet<String> = draft_map.keys().cloned().collect();
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
            vspace(ui, th.spacing_md);
        }
    }
}

/// 한 확장자 묶음. 확장자 머리줄 아래에 detector 순서 목록을 그린다(먼저 맞는 detector가 이긴다).
fn draw_extension_row(
    ui: &mut egui::Ui,
    ext: &str,
    draft_map: &mut BTreeMap<String, Vec<DetectorId>>,
    file_format: &FileFormatRegistry,
) {
    let th = crate::theme::theme();
    let candidates = file_format.detectors_for_extension(ext);
    if candidates.is_empty() {
        egui::Frame::new()
            .inner_margin(egui::vec2(th.spacing_sm.value(), th.spacing_xs.value()))
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

    // 머리줄은 위·아래에만 space-xs 여백을 둔다(시안 `padding: xs 0`).
    let head_y = th.spacing_xs.value().round() as i8;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            top: head_y,
            bottom: head_y,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(".{}", ext))
                        .monospace()
                        .size(th.font_size_caption.value())
                        .color(th.text_secondary().to_egui()),
                );
                hspace(ui, th.spacing_sm);
                if draft_map.contains_key(ext)
                    && ui
                        .small_button(t("settings.file_handler.extension_mapping.reset"))
                        .on_hover_text(t("settings.file_handler.extension_mapping.reset_tooltip"))
                        .clicked()
                {
                    draft_map.insert(ext.to_string(), Vec::new());
                }
            });
        });
    // 후보 = 켜져 있고 이 확장자를 지원하는 detector. 비후보(꺼짐·미설치) 행은 자리를 지키고
    // ▲▼를 모두 disabled로 둔다. ▼는 뒤에 후보가 더 없으면 disabled다.
    let last_candidate = last_candidate(&order, &candidates);
    let mut move_up: Option<usize> = None;
    let mut move_down: Option<usize> = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (i, id) in order.iter().enumerate() {
            let candidate = candidates.contains(id);
            let (up, down) = detector_row(
                ui,
                &th,
                i,
                id.as_str(),
                candidate,
                arrows_enabled(i, candidate, last_candidate),
            );
            if up {
                move_up = Some(i);
            }
            if down {
                move_down = Some(i);
            }
        }
    });
    if let Some(i) = move_up {
        let mut new_order = order.clone();
        new_order.swap(i - 1, i);
        draft_map.insert(ext.to_string(), new_order);
    } else if let Some(i) = move_down {
        let mut new_order = order.clone();
        new_order.swap(i, i + 1);
        draft_map.insert(ext.to_string(), new_order);
    }
}

/// 순서 목록에서 마지막 후보의 위치. 그 뒤에는 비후보 행만 온다.
fn last_candidate(order: &[DetectorId], candidates: &[DetectorId]) -> Option<usize> {
    order.iter().rposition(|id| candidates.contains(id))
}

/// (▲, ▼) 활성 여부. 맨 위 행의 ▲와 마지막 후보의 ▼는 disabled이고,
/// 비후보 행은 둘 다 disabled다. 숨기지 않으므로 행마다 자리가 같다.
fn arrows_enabled(index: usize, candidate: bool, last_candidate: Option<usize>) -> (bool, bool) {
    (
        candidate && index > 0,
        candidate && Some(index) != last_candidate,
    )
}

/// detector 한 행 — 순번 · 이름 · (비후보면 Tag disabled "off") · ▲ · ▼.
/// ▲·▼가 눌렸는지를 돌려준다. disabled 버튼은 클릭을 받지 않는다.
fn detector_row(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    index: usize,
    name: &str,
    candidate: bool,
    (up_enabled, down_enabled): (bool, bool),
) -> (bool, bool) {
    let mut up = false;
    let mut down = false;
    let resp = ui.horizontal(|ui| {
        ui.set_min_height(th.settings_row_min_height().value());
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let (num_rect, _) = ui.allocate_exact_size(
            egui::vec2(th.spacing_lg.value(), th.font_size_caption.value()),
            egui::Sense::hover(),
        );
        ui.painter().text(
            num_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            (index + 1).to_string(),
            egui::FontId::monospace(th.font_size_caption.value()),
            th.text_muted().to_egui(),
        );
        let name_fg = if candidate {
            th.text_secondary()
        } else {
            th.text_disabled()
        };
        ui.label(
            egui::RichText::new(name)
                .size(th.font_size_body.value())
                .color(name_fg.to_egui()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            down = IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .enabled(down_enabled)
                .show(ui, th, &|ui, rect, c| {
                    icons::CHEVRON_DOWN
                        .image(rect.width(), c)
                        .paint_at(ui, rect)
                })
                .clicked();
            up = IconButton::new()
                .variant(IconButtonVariant::Ghost)
                .size(ControlSize::Sm)
                .enabled(up_enabled)
                .show(ui, th, &|ui, rect, c| {
                    icons::CHEVRON_UP.image(rect.width(), c).paint_at(ui, rect)
                })
                .clicked();
            if !candidate {
                tag_disabled(
                    ui,
                    th,
                    t("settings.file_handler.extension_mapping.off"),
                    false,
                );
            }
        });
    });
    let rect = resp.response.rect;
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(th.border_width.value(), th.separator.to_egui()),
    );
    (up, down)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시안 규칙: ▲는 맨 위 행, ▼는 마지막 후보에서 disabled다. 비후보(꺼짐·미설치) 행은
    /// 위치와 관계없이 둘 다 disabled다. 뒤에 비후보만 남은 후보는 ▼를 쓸 수 없다.
    #[test]
    fn arrows_follow_the_top_row_last_candidate_and_non_candidate_rule() {
        let id = |s: &str| DetectorId::new(s);
        let order = [id("markdown"), id("editor"), id("html-preview")];
        let candidates = [id("markdown"), id("editor")];
        let last = last_candidate(&order, &candidates);
        assert_eq!(last, Some(1));
        let rows: Vec<(bool, bool)> = order
            .iter()
            .enumerate()
            .map(|(i, d)| arrows_enabled(i, candidates.contains(d), last))
            .collect();
        assert_eq!(rows, [(false, true), (true, false), (false, false)]);

        // 비후보가 가운데에 끼어도 마지막 후보만 ▼가 막힌다.
        let order = [id("a"), id("off"), id("b")];
        let candidates = [id("a"), id("b")];
        let last = last_candidate(&order, &candidates);
        let rows: Vec<(bool, bool)> = order
            .iter()
            .enumerate()
            .map(|(i, d)| arrows_enabled(i, candidates.contains(d), last))
            .collect();
        assert_eq!(rows, [(false, true), (false, false), (true, false)]);
    }
}
