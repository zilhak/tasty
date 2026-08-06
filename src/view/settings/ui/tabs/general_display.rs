//! macOS 전용 — Alt/Option/Shift 단축키 표시 스타일.
//!
//! 저장 포맷(바인딩 문자열)은 건드리지 않고 화면 표시 문자열만 바꾼다
//! (`docs/design/policies/key-mapping.md` 저장↔표시 분리 원칙). 백엔드는
//! `GeneralSettings::{alt,option,shift}_display_style`.
//!
//! `"symbol"` 스타일(⌘/⌥/⇧)은 egui 폰트 fallback 체인에 없는 glyph 라 텍스트로
//! 그리면 tofu box 로 깨진다 — 이 탭의 3개 드롭다운만 [`display_style_combo`] 로
//! 벡터 아이콘 표시를 지원한다. egui `ComboBox::selected_text`는
//! `impl Into<WidgetText>`라 텍스트 galley 만 그리는 자리라 이미지 라벨을 못 넣는다
//! (egui 0.31 `combo_box.rs::combo_box_dyn` 확인) — 그래서 트리거를 `Button`
//! (secondary variant, 기존 ComboBox 버튼과 동일한 룩) + `egui::popup_below_widget`
//! 로 직접 그린다.

use crate::i18n::t;
use crate::settings::Settings;
use tasty_icons::Icon;
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, MenuItemVariant, menu_item, vspace};

/// 표시 스타일 드롭다운의 옵션 하나. `icon` 이 있으면 이 옵션이 `"symbol"` 스타일 —
/// 닫힌 트리거는 이 옵션이 선택됐을 때 텍스트 없이 아이콘 단독으로(§6-1), 그 외
/// 옵션은 기존처럼 텍스트로 그린다. 펼친 옵션 리스트는 옵션마다 (아이콘 있으면)
/// 아이콘 + 텍스트 라벨 + 현재 선택 체크를 그린다(§6-1).
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
            // `menu_item` 은 `shortcut: None` 이면 우측 여백을 전혀 예약하지 않는다
            // (menu_item.rs) — 이 체크마크는 shortcut 이 아니라 별도로 그리므로, 팝업
            // 폭 자체가 (아이콘+라벨+체크마크)를 담을 만큼 넓어야 겹치지 않는다.
            // 트리거 폭(`resp.rect.width()`)만으로는 부족하다 — 트리거가 아이콘
            // 단독("symbol" 선택 시)이면 목록의 텍스트 라벨보다 훨씬 좁기 때문.
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
