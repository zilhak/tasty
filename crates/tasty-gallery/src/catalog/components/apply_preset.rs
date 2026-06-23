//! Apply Preset popup 데모 (Tier 3).
//!
//! 본체 `src/adapters/ui/popup/preset_apply.rs::draw_apply_preset_view` 가
//! 표현하는 시각 상태를 mock props 로 재현. 본체와 *시각 동일* 하지만
//! gallery 가 본체 binary 에 의존할 수 없으므로 view 로직은 로컬 미러
//! (POC 패턴 — `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).
//!
//! 본체 wrapper 3 종 (`draw_apply_workspace_popup` / `_tab_` / `_pane_`) 은 모두
//! 같은 view 를 호출하므로 PresetKind 차이는 시각상 *제목* 뿐이다 — 카탈로그는
//! 한 case 별로 kind 라벨만 다르게 표시한다.
//!
//! 대표 상태:
//! - Empty: 저장된 preset 0 건 ("No presets saved yet." 메시지).
//! - Workspace, 3 presets: 전형적 사용.
//! - Tab, 5 presets: 다른 PresetKind 표시.
//! - Pane, 1 preset: 단일 항목.
//! - Many: 20 entries (스크롤 영역 검증).
//!
//! Action 은 카탈로그에서 시각 검증 전용이라 표시만 (실행 없음).

use tasty_type_appearance::theme::Theme;

use crate::catalog::specimen::case_title;

struct ApplyPresetProps<'a> {
    theme: &'a Theme,
    empty_label: &'a str,
    apply_button_label: &'a str,
    cancel_button_label: &'a str,
    names: &'a [String],
    selected: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ApplyPresetAction {
    None,
    Cancel,
    Select(String),
    Apply(String),
}

/// 본체 `draw_apply_preset_view` 의 시각 미러 (gallery 측 복제).
fn draw_apply_preset_view(ui: &mut egui::Ui, props: &ApplyPresetProps<'_>) -> ApplyPresetAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return ApplyPresetAction::Cancel;
    }

    let th = props.theme;
    let names = props.names;

    let cur_index = props
        .selected
        .and_then(|s| names.iter().position(|n| n == s));
    let up = ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowUp));
    let down = ui.ctx().input(|i| i.key_pressed(egui::Key::ArrowDown));
    let arrow_index = if (up || down) && !names.is_empty() {
        let cur = cur_index.unwrap_or(0);
        Some(if up {
            if cur == 0 { names.len() - 1 } else { cur - 1 }
        } else {
            (cur + 1) % names.len()
        })
    } else {
        None
    };
    let effective_index = arrow_index.or(cur_index);
    let effective_selected: Option<&str> =
        effective_index.and_then(|i| names.get(i).map(|s| s.as_str()));

    let enter_pressed = ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));

    let mut apply_clicked = false;
    let mut cancel_clicked = false;
    let mut clicked_name: Option<String> = None;
    let mut double_clicked_name: Option<String> = None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;

        if names.is_empty() {
            ui.label(
                egui::RichText::new(props.empty_label)
                    .color(egui::Color32::from(th.subtext0))
                    .italics(),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    for name in names {
                        let is_selected = effective_selected == Some(name.as_str());
                        let full_width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(full_width, 22.0),
                            egui::Sense::click(),
                        );
                        if is_selected || resp.hovered() {
                            // 본체는 theme.hover_overlay (premultiplied). gallery
                            // 미러는 surface1 으로 시각 근사.
                            let hover = egui::Color32::from(th.surface1);
                            ui.painter().rect_filled(rect, 4.0, hover);
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            name,
                            egui::FontId::proportional(12.0),
                            if is_selected {
                                egui::Color32::from(th.text)
                            } else {
                                egui::Color32::from(th.subtext0)
                            },
                        );
                        if resp.clicked() {
                            clicked_name = Some(name.clone());
                        }
                        if resp.double_clicked() {
                            double_clicked_name = Some(name.clone());
                        }
                    }
                });
        }

        ui.separator();
        ui.horizontal(|ui| {
            let can_apply = !names.is_empty() && effective_selected.is_some();
            if ui
                .add_enabled(can_apply, egui::Button::new(props.apply_button_label))
                .clicked()
            {
                apply_clicked = true;
            }
            if ui.button(props.cancel_button_label).clicked() {
                cancel_clicked = true;
            }
        });
    });

    if let Some(name) = double_clicked_name {
        return ApplyPresetAction::Apply(name);
    }
    if enter_pressed
        && !names.is_empty()
        && let Some(name) = effective_selected.map(|s| s.to_string())
    {
        return ApplyPresetAction::Apply(name);
    }
    if apply_clicked && let Some(name) = effective_selected.map(|s| s.to_string()) {
        return ApplyPresetAction::Apply(name);
    }
    if cancel_clicked {
        return ApplyPresetAction::Cancel;
    }
    if let Some(name) = clicked_name {
        return ApplyPresetAction::Select(name);
    }
    if let Some(i) = arrow_index
        && let Some(name) = names.get(i)
    {
        return ApplyPresetAction::Select(name.clone());
    }

    ApplyPresetAction::None
}

fn case_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    names: &[String],
    selected: Option<&str>,
) {
    case_title(ui, theme, title);
    egui::Frame::group(ui.style())
        .corner_radius(theme.corner_radius.value())
        .show(ui, |ui| {
            ui.set_min_width(320.0);
            let props = ApplyPresetProps {
                theme,
                empty_label: "No presets saved yet.",
                apply_button_label: "Apply",
                cancel_button_label: "Cancel",
                names,
                selected,
            };
            let _action = draw_apply_preset_view(ui, &props);
        });
    ui.add_space(16.0);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "draw_apply_preset_view — preset picker for Workspace/Tab/Pane (5 mock states)",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Wrapper: src/adapters/ui/popup/preset_apply.rs::draw_apply_{workspace,tab,pane}_popup",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    // Case 1: Empty.
    case_box(ui, theme, "Case 1 — Empty (0 presets)", &[], None);

    // Case 2: Workspace, 3 presets.
    let workspace = vec![
        "Default workspace".to_string(),
        "Dev (split + terminal)".to_string(),
        "Docs reading".to_string(),
    ];
    case_box(
        ui,
        theme,
        "Case 2 — Workspace, 3 presets",
        &workspace,
        Some("Dev (split + terminal)"),
    );

    // Case 3: Tab, 5 presets.
    let tabs = vec![
        "edit-and-run".to_string(),
        "git-log".to_string(),
        "markdown-preview".to_string(),
        "scratch".to_string(),
        "test-watch".to_string(),
    ];
    case_box(ui, theme, "Case 3 — Tab, 5 presets", &tabs, Some("git-log"));

    // Case 4: Pane, 1 preset.
    let pane = vec!["single-pane".to_string()];
    case_box(
        ui,
        theme,
        "Case 4 — Pane, 1 preset",
        &pane,
        Some("single-pane"),
    );

    // Case 5: Many (scroll active).
    let many: Vec<String> = (0..20).map(|i| format!("preset-{i:02}")).collect();
    case_box(
        ui,
        theme,
        "Case 5 — Many (20 presets, scroll active)",
        &many,
        Some("preset-03"),
    );

    ui.label(
        egui::RichText::new(
            "Note: hover/selection overlay 색은 본체의 theme.hover_overlay (premultiplied) 대신 \
             surface1 로 미러. 본체 wrapper 는 PresetStore::list(kind) 로 names 를 채우고 \
             Apply 시 Intent::ApplyPreset 을 dispatch — 갤러리에선 mock 정적 데이터.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
