//! Preset 적용 picker popup × 3.
//!
//! 저장된 Workspace/Tab/Pane preset 목록을 보여주고, 사용자가 선택해 [적용] 버튼이나
//! Enter 로 적용하면 `Intent::ApplyPreset` 을 dispatch 한다.
//! preset 목록 자체는 `state.preset_store: Arc<Mutex<PresetStore>>` (Core 의 Arc clone)
//! 에서 매 프레임 lock 으로 읽는다.
//!
//! ## Split: wrapper / view / action
//!
//! 순수 view (`draw_apply_preset_view`) 는 `ApplyPresetProps` (theme + 라벨 +
//! `&[String]` names + selected index) 만 받아 `ApplyPresetAction` 을 반환한다.
//! 세 wrapper (`draw_apply_workspace_popup` / `_tab_` / `_pane_`) 는 동일한 공통
//! helper (`draw_apply_popup`) 를 통해 `PresetStore::list(kind)` 에서 names 를
//! 읽고, view 를 호출한 뒤, action 을 `Intent` dispatch + `PopupAction` 으로
//! 번역한다.
//!
//! 같은 view 를 mock data 로 호출하는 미러는 `tasty-gallery` 의
//! `catalog::components::apply_preset` 에 존재.

use tasty_presets::PresetKind;
use tasty_type_geometry::length::LogicalPx;

// ── 디자인 스케일 밖 폰트 크기 ──────────────────────────────────────────────
//
// **`.5` 로 끝나는 값은 애초에 토큰이 될 수 없다** — 토큰 폰트 크기는 `zoomed()` 의
// `.round()` 를 거쳐 어떤 `ui_scale` 에서도 정수다. semantic 이 없는 primitive(12)도
// 같은 이유로 이름만 붙인다. 규칙 전문은 `docs/design/systems/theme.md`
// "스케일 밖 폰트 값".

/// preset 행 라벨. DTCG primitive `font-size-12` 는 있으나 semantic role 이 없어
/// `Theme` 필드가 없다 — ADR-0126 대로 **이름에 primitive 임을 남긴다**.
const PRESET_ROW_LABEL_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::intent::Intent;
use crate::state::AppState;
use crate::theme;
use crate::theme::Theme;

pub const APPLY_WORKSPACE_POPUP_ID: &str = "apply_workspace_preset";
pub const APPLY_TAB_POPUP_ID: &str = "apply_tab_preset";
pub const APPLY_PANE_POPUP_ID: &str = "apply_pane_preset";

/// Pure inputs to [`draw_apply_preset_view`]. AppState / CoreState 의존 0.
///
/// `names` 는 빈 vec 일 수 있고 (저장된 preset 이 없을 때), `selected` 는 names
/// 안의 어떤 값을 가리키거나 `None` (아직 선택 없음).
pub struct ApplyPresetProps<'a> {
    pub theme: &'a Theme,
    pub empty_label: &'a str,
    pub apply_button_label: &'a str,
    pub cancel_button_label: &'a str,
    pub names: &'a [String],
    pub selected: Option<&'a str>,
}

/// User intent surfaced by [`draw_apply_preset_view`].
///
/// wrapper 가 mutation 으로 번역:
/// - `Select(name)` → `state.dialogs.preset_picker_selected = Some(name)`
/// - `Apply(name)` → `Intent::ApplyPreset` dispatch + selection clear + Close
/// - `Cancel` → selection clear + Close
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyPresetAction {
    None,
    Cancel,
    Select(String),
    Apply(String),
}

/// PopupDef::on_close entry point (3개 팝업 공용) — 어떤 경로로 닫히든 선택/대상
/// 카테고리를 비운다. 이전엔 Cancel 액션 경로에만 이 정리가 있어서, X 버튼/외부
/// 클릭(`close_on_outside_click: true`)으로 닫으면 두 필드가 그대로 남아 다음에
/// 열 때 누출되는 버그가 있었다 — 그 버그의 수정.
pub fn on_close_apply_preset_popup(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.preset_picker_selected = None;
    state.dialogs.preset_apply_target_category = None;
}

pub fn draw_apply_workspace_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    draw_apply_popup(ui, state, engine, PresetKind::Workspace)
}

pub fn draw_apply_tab_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    draw_apply_popup(ui, state, engine, PresetKind::Tab)
}

pub fn draw_apply_pane_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    draw_apply_popup(ui, state, engine, PresetKind::Pane)
}

fn draw_apply_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    kind: PresetKind,
) -> PopupAction {
    let th = theme::theme();
    let names: Vec<String> = crate::poison::recover_mutex(
        state.preset_store.lock(),
        crate::core::PRESET_STORE_WHAT,
        &crate::core::PRESET_STORE_POISONED,
    )
    .list(kind);

    // 초기 선택 seeding: 선택이 없으면 첫 항목, 선택이 names 에 없으면 첫 항목으로 reset.
    if state.dialogs.preset_picker_selected.is_none() {
        if let Some(first) = names.first() {
            state.dialogs.preset_picker_selected = Some(first.clone());
        }
    } else if let Some(sel) = state.dialogs.preset_picker_selected.as_deref()
        && !names.iter().any(|n| n == sel)
    {
        state.dialogs.preset_picker_selected = names.first().cloned();
    }

    let empty_label = t("preset.popup.empty");
    let apply_label = t("preset.popup.apply_button");
    let cancel_label = t("preset.popup.cancel_button");

    let props = ApplyPresetProps {
        theme: &th,
        empty_label,
        apply_button_label: apply_label,
        cancel_button_label: cancel_label,
        names: &names,
        selected: state.dialogs.preset_picker_selected.as_deref(),
    };

    let action = draw_apply_preset_view(ui, &props);

    match action {
        ApplyPresetAction::None => PopupAction::None,
        ApplyPresetAction::Cancel => {
            // 선택/대상 카테고리 정리는 `on_close_apply_preset_popup` 훅이 닫힘
            // 경로와 무관하게 담당한다(중복 방지).
            PopupAction::Close
        }
        ApplyPresetAction::Select(name) => {
            state.dialogs.preset_picker_selected = Some(name);
            PopupAction::None
        }
        ApplyPresetAction::Apply(name) => {
            let category = state.dialogs.preset_apply_target_category.take();
            state.dispatch_intent(
                Intent::ApplyPreset {
                    kind,
                    name: name.clone(),
                    category,
                }
                .from_user_menu("preset_apply_popup"),
            );
            state.dialogs.preset_picker_selected = None;
            PopupAction::Close
        }
    }
}

/// Pure view: preset 목록 + Apply/Cancel 버튼만 그린다. AppState/CoreState
/// 접근 없음. 갤러리에서 mock props 로 직접 호출 가능.
pub fn draw_apply_preset_view(
    ui: &mut egui::Ui,
    props: &ApplyPresetProps<'_>,
) -> ApplyPresetAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return ApplyPresetAction::Cancel;
    }

    let th = props.theme;
    let names = props.names;

    // 화살표 이동 처리 — frame-local selected 인덱스를 만들고 update.
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
        ui.spacing_mut().item_spacing.y = th.spacing_xs.value();

        if names.is_empty() {
            ui.label(
                egui::RichText::new(props.empty_label)
                    .color(th.text_muted())
                    .italics(),
            );
        } else {
            egui::ScrollArea::vertical()
                .max_height(th.autocomplete_max_height().value())
                .show(ui, |ui| {
                    for name in names {
                        let is_selected = effective_selected == Some(name.as_str());
                        let full_width = ui.available_width();
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(full_width, 22.0),
                            egui::Sense::click(),
                        );
                        if is_selected {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.active_overlay.to_egui_premultiplied(),
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                th.hover_overlay.to_egui_premultiplied(),
                            );
                        }
                        ui.painter().text(
                            egui::pos2(rect.min.x + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            name,
                            egui::FontId::proportional(PRESET_ROW_LABEL_PRIMITIVE_12.value()),
                            if is_selected {
                                th.text_primary().into()
                            } else {
                                th.text_muted().into()
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

    // Action 우선순위: Apply > Cancel > Select > None.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_with_input(
        raw: egui::RawInput,
        names: &[String],
        selected: Option<&str>,
    ) -> ApplyPresetAction {
        let ctx = egui::Context::default();
        let mut out = ApplyPresetAction::None;
        let theme = test_theme();
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let props = ApplyPresetProps {
                    theme: &theme,
                    empty_label: "No presets saved yet.",
                    apply_button_label: "Apply",
                    cancel_button_label: "Cancel",
                    names,
                    selected,
                };
                out = draw_apply_preset_view(ui, &props);
            });
        }));
        out
    }

    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn view_returns_none_when_empty_and_no_input() {
        let action = run_with_input(egui::RawInput::default(), &[], None);
        assert_eq!(action, ApplyPresetAction::None);
    }

    #[test]
    fn view_returns_cancel_on_escape() {
        let mut raw = egui::RawInput::default();
        raw.events.push(key_event(egui::Key::Escape));
        let action = run_with_input(raw, &[], None);
        assert_eq!(action, ApplyPresetAction::Cancel);
    }

    #[test]
    fn view_returns_apply_on_enter_when_selected() {
        let names = vec!["alpha".to_string(), "beta".to_string()];
        let mut raw = egui::RawInput::default();
        raw.events.push(key_event(egui::Key::Enter));
        let action = run_with_input(raw, &names, Some("alpha"));
        assert_eq!(action, ApplyPresetAction::Apply("alpha".to_string()));
    }

    #[test]
    fn view_arrow_down_moves_selection() {
        let names = vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()];
        let mut raw = egui::RawInput::default();
        raw.events.push(key_event(egui::Key::ArrowDown));
        let action = run_with_input(raw, &names, Some("alpha"));
        assert_eq!(action, ApplyPresetAction::Select("beta".to_string()));
    }

    #[test]
    fn view_arrow_up_wraps_to_last() {
        let names = vec!["alpha".to_string(), "beta".to_string()];
        let mut raw = egui::RawInput::default();
        raw.events.push(key_event(egui::Key::ArrowUp));
        let action = run_with_input(raw, &names, Some("alpha"));
        assert_eq!(action, ApplyPresetAction::Select("beta".to_string()));
    }

    #[test]
    fn view_arrow_then_enter_applies_new_selection() {
        let names = vec!["alpha".to_string(), "beta".to_string()];
        let mut raw = egui::RawInput::default();
        raw.events.push(key_event(egui::Key::ArrowDown));
        raw.events.push(key_event(egui::Key::Enter));
        let action = run_with_input(raw, &names, Some("alpha"));
        assert_eq!(action, ApplyPresetAction::Apply("beta".to_string()));
    }

    #[test]
    fn view_renders_with_names_without_panic() {
        let names = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let action = run_with_input(egui::RawInput::default(), &names, Some("a"));
        assert_eq!(action, ApplyPresetAction::None);
    }
}
