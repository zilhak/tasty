//! General 서브탭 끝의 탐색기 드래그 반전 modifier 행. 누르고 끌면 이동과 복사가 바뀐다.

use crate::i18n::t;
use crate::settings::{GeneralSettings, KeybindingSettings};
use tasty_settings::keybindings::explorer_drag_flip_modifier_options;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{SettingsRow, select};

/// 행의 라벨·caption. 서브탭 라벨 열을 잴 때도 쓴다.
pub(super) fn row() -> SettingsRow<'static> {
    SettingsRow::new(t("settings.keybindings.explorer_drag_flip_modifier_label"))
        .caption(t("settings.keybindings.explorer_drag_flip_modifier_hint"))
}

pub(super) fn draw(
    ui: &mut egui::Ui,
    th: &Theme,
    keybindings: &mut KeybindingSettings,
    general: &GeneralSettings,
    label_col: LogicalPx,
) {
    let names = explorer_drag_flip_modifier_options(&keybindings.explorer_drag_flip_modifier);
    let labels: Vec<String> = names
        .iter()
        .map(|n| KeybindingSettings::format_display(n, general))
        .collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let mut selected = names
        .iter()
        .position(|n| *n == keybindings.explorer_drag_flip_modifier)
        .unwrap_or(0);
    row().show(ui, th, label_col, |ui| {
        if select(
            ui,
            th,
            "kb_explorer_drag_flip_modifier",
            &mut selected,
            &labels,
            th.field_width_md.value(),
            true,
        ) && let Some(name) = names.get(selected)
        {
            keybindings.explorer_drag_flip_modifier.clone_from(name);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::plugins::ui::text_probe::visible_text_rects;

    fn texts(kb: &mut KeybindingSettings) -> Vec<(String, egui::Rect)> {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 300.0));
        let th = crate::theme::theme();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw(
                        ui,
                        &th,
                        kb,
                        &GeneralSettings::default(),
                        th.settings_label_width(),
                    );
                });
            },
        );
        visible_text_rects(&output, screen)
    }

    /// 라벨 오른쪽에 현재 값이, 그 아래에 caption 이 보인다.
    #[test]
    fn the_row_shows_the_current_modifier_beside_its_label_and_the_caption_below() {
        let mut kb = KeybindingSettings {
            explorer_drag_flip_modifier: "shift".into(),
            ..Default::default()
        };
        let shown = texts(&mut kb);
        let find = |text: &str| {
            shown
                .iter()
                .find(|(s, _)| s == text)
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("{text:?} not drawn: {shown:?}"))
        };
        let label = find(t("settings.keybindings.explorer_drag_flip_modifier_label"));
        let value = find(&KeybindingSettings::format_display(
            "shift",
            &GeneralSettings::default(),
        ));
        let caption = find(t("settings.keybindings.explorer_drag_flip_modifier_hint"));
        assert!(
            value.left() > label.right(),
            "value sits right of the label"
        );
        assert!(caption.top() > label.bottom(), "caption sits under the row");
        assert_eq!(
            kb.explorer_drag_flip_modifier, "shift",
            "drawing keeps the value"
        );
    }

    /// 설정 파일의 조합 값도 그대로 보이고 바뀌지 않는다.
    #[test]
    fn a_combo_from_the_config_file_is_shown_and_kept() {
        let mut kb = KeybindingSettings {
            explorer_drag_flip_modifier: "alt+shift".into(),
            ..Default::default()
        };
        let shown = texts(&mut kb);
        let display = KeybindingSettings::format_display("alt+shift", &GeneralSettings::default());
        assert!(shown.iter().any(|(s, _)| *s == display), "{shown:?}");
        assert_eq!(kb.explorer_drag_flip_modifier, "alt+shift");
    }
}
