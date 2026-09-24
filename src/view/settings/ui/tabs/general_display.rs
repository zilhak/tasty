//! macOS 수식키 표시 스타일. 저장된 바인딩은 바꾸지 않는다.
//! 기호 글꼴의 누락을 피하려 symbol 스타일은 벡터 아이콘으로 그린다.
//! 관련 정책: docs/design/policies/key-mapping.md.

use crate::i18n::t;
use crate::settings::Settings;
use tasty_icons::Icon;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, MenuItemVariant, menu_item, vspace};

/// 표시 스타일 항목. symbol을 선택하면 버튼에는 아이콘만, 목록에는 아이콘과 라벨을 표시한다.
struct DisplayStyleOption {
    value: &'static str,
    label: &'static str,
    icon: Option<Icon>,
}

pub fn draw_general_display_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("general_display_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.general.alt_display_style_label"));
            display_style_combo(
                ui,
                &th,
                "alt_display_style",
                &mut settings.general.alt_display_style,
                &[
                    DisplayStyleOption {
                        value: "alt",
                        label: t("settings.general.alt_display_style_alt"),
                        icon: None,
                    },
                    DisplayStyleOption {
                        value: "cmd",
                        label: t("settings.general.alt_display_style_cmd"),
                        icon: None,
                    },
                    DisplayStyleOption {
                        value: "symbol",
                        label: t("settings.general.alt_display_style_symbol"),
                        icon: Some(tasty_icons::CMD_KEY),
                    },
                ],
            );
            ui.end_row();

            ui.label(t("settings.general.option_display_style_label"));
            display_style_combo(
                ui,
                &th,
                "option_display_style",
                &mut settings.general.option_display_style,
                &[
                    DisplayStyleOption {
                        value: "option",
                        label: t("settings.general.option_display_style_option"),
                        icon: None,
                    },
                    DisplayStyleOption {
                        value: "symbol",
                        label: t("settings.general.option_display_style_symbol"),
                        icon: Some(tasty_icons::OPTION_KEY),
                    },
                ],
            );
            ui.end_row();

            ui.label(t("settings.general.shift_display_style_label"));
            display_style_combo(
                ui,
                &th,
                "shift_display_style",
                &mut settings.general.shift_display_style,
                &[
                    DisplayStyleOption {
                        value: "shift",
                        label: t("settings.general.shift_display_style_shift"),
                        icon: None,
                    },
                    DisplayStyleOption {
                        value: "symbol",
                        label: t("settings.general.shift_display_style_symbol"),
                        icon: Some(tasty_icons::SHIFT_KEY),
                    },
                ],
            );
            ui.end_row();
        });
}

/// 아이콘을 지원하는 표시 스타일 드롭다운 하나. 닫힌 트리거는 현재 선택이
/// `"symbol"` 이면 아이콘 단독(+chevronDown), 아니면 텍스트(+chevronDown). 펼친
/// 옵션 리스트는 [`menu_item`] 행(아이콘 있으면 leading) + 활성 행 trailing 체크.
fn display_style_combo(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    value: &mut String,
    options: &[DisplayStyleOption],
) {
    let current = options
        .iter()
        .find(|o| o.value == value.as_str())
        .unwrap_or(&options[0]);

    let chevron_paint = |ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32| {
        tasty_icons::CHEVRON_DOWN
            .image(rect.height(), color)
            .paint_at(ui, rect);
    };
    let glyph = current.icon;
    let glyph_paint = move |ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32| {
        if let Some(icon) = glyph {
            icon.image(rect.height(), color).paint_at(ui, rect);
        }
    };

    let mut button = Button::new(if current.icon.is_some() {
        ""
    } else {
        current.label
    })
    .variant(ButtonVariant::Secondary)
    .trailing_icon(&chevron_paint);
    if current.icon.is_some() {
        button = button.leading_icon(&glyph_paint);
    }
    let resp = button.show(ui, theme);

    let popup_id = ui.make_persistent_id((id_salt, "display_style_popup"));
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            let sz = theme.icon_glyph_size_md.value();
            let pad_x = theme.menu_item_padding_x().value();
            let gap = theme.spacing_sm.value();
            let body = theme.font_size_body.value();
            // 아이콘만 있는 버튼 폭으로는 목록의 라벨·체크마크가 겹치므로 추가 폭을 확보한다.
            let content_width = options
                .iter()
                .map(|opt| {
                    let label_w = ui
                        .painter()
                        .layout_no_wrap(
                            opt.label.to_string(),
                            egui::FontId::proportional(body),
                            egui::Color32::PLACEHOLDER,
                        )
                        .rect
                        .width();
                    let icon_w = if opt.icon.is_some() { sz + gap } else { 0.0 };
                    pad_x * 2.0 + icon_w + label_w + gap + sz
                })
                .fold(0.0_f32, f32::max);
            ui.set_min_width(resp.rect.width().max(content_width));
            let mut picked: Option<&'static str> = None;
            for opt in options {
                let is_active = opt.value == value.as_str();
                let icon_closure = opt.icon.map(|icon| {
                    move |ui: &mut egui::Ui, rect: egui::Rect, color: egui::Color32| {
                        icon.image(rect.height(), color).paint_at(ui, rect);
                    }
                });
                let icon_paint = icon_closure
                    .as_ref()
                    .map(|f| f as &dyn Fn(&mut egui::Ui, egui::Rect, egui::Color32));
                let row = menu_item(
                    ui,
                    theme,
                    icon_paint,
                    opt.label,
                    None,
                    MenuItemVariant::Normal,
                    is_active,
                    true,
                );
                if is_active {
                    let center =
                        egui::pos2(row.rect.right() - pad_x - sz * 0.5, row.rect.center().y);
                    let r = egui::Rect::from_center_size(center, egui::vec2(sz, sz));
                    tasty_icons::CHECK
                        .image(sz, theme.text_primary().to_egui())
                        .paint_at(ui, r);
                }
                if row.clicked() && !is_active {
                    picked = Some(opt.value);
                }
            }
            if let Some(v) = picked {
                *value = v.to_string();
            }
        },
    );
}
