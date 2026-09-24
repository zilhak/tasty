//! en/ko/ja 단축키 라벨을 실제 설정 화면의 egui 글꼴로 측정해 열 폭과 비교한다.
//! apply_theme_to_egui를 호출해야 egui 기본값이 아닌 제품의 Body 크기로 측정한다.
//! 언어별 TOML을 직접 읽어 전역 OnceLock 초기화에 의존하지 않는다.

// 테스트에서는 반환값보다 UI 측정 결과를 확인한다. let_underscore_documented도 테스트 본문은 제외한다.
#![allow(clippy::let_underscore_must_use)]

use std::collections::HashMap;

/// 배율 1.0에서의 도움말 아이콘 예약 폭.
/// 실제 화면은 Theme 배율을 적용하지만 이 검사는 기본 배율만 측정한다.
const HELP_HINT_GAP: f32 = 4.0;
const ICON_SLOT: f32 = 14.0;

fn flatten(prefix: &str, value: &toml::Value, map: &mut HashMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let full = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&full, v, map);
            }
        }
        toml::Value::String(s) => {
            map.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

fn load_lang(toml_str: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(v) = toml_str.parse::<toml::Value>() {
        flatten("", &v, &mut map);
    }
    map
}

/// GENERAL_BINDING_FIELDS에서 실제 단축키 행의 라벨 키를 읽는다.
fn entry_labels() -> Vec<&'static str> {
    crate::settings::KeybindingSettings::GENERAL_BINDING_FIELDS
        .iter()
        .map(|(_, label_key)| *label_key)
        .collect()
}

/// quick_switch.rs 의 bare-target 라벨(`"{label}:"`, 아이콘 슬롯 없음) i18n key 전체.
const QUICK_SWITCH_LABELS: &[&str] = &[
    "settings.keybindings.tab_switch_slot_label",
    "settings.keybindings.tab_switch_next_label",
    "settings.keybindings.tab_switch_prev_label",
    "settings.keybindings.workspace_switch_slot_label",
    "settings.keybindings.workspace_switch_next_label",
    "settings.keybindings.workspace_switch_prev_label",
    "settings.keybindings.category_switch_slot_label",
    "settings.keybindings.category_switch_next_label",
    "settings.keybindings.category_switch_prev_label",
];

fn measure(ctx: &egui::Context, text: &str, font_id: egui::FontId) -> f32 {
    ctx.fonts(|f| {
        f.layout_no_wrap(text.to_string(), font_id, egui::Color32::WHITE)
            .size()
            .x
    })
}

/// en/ko/ja 라벨의 최대 폭이 LABEL_COL_WIDTH 이내인지 확인한다.
#[test]
fn labels_fit_within_fixed_column() {
    let ctx = egui::Context::default();
    tasty_egui_theme::install_cjk_fallback(&ctx);
    // 실제 설정 창과 같은 Body 글꼴 크기를 적용한다.
    tasty_egui_theme::apply_theme_to_egui(&crate::theme::theme(), &ctx);

    let langs: &[(&str, &str)] = &[
        ("en", include_str!("../../../../../lang/en.toml")),
        ("ko", include_str!("../../../../../lang/ko.toml")),
        ("ja", include_str!("../../../../../lang/ja.toml")),
    ];

    let column_width = super::LABEL_COL_WIDTH.value();
    let mut worst_entries = ("", "", 0.0f32);
    let mut worst_qs = ("", "", 0.0f32);

    let _ = ctx.run(Default::default(), |ctx| {
        let font_id = egui::TextStyle::Body.resolve(&ctx.style());

        for (lang, toml_str) in langs {
            let table = load_lang(toml_str);

            for key in &entry_labels() {
                let text = table
                    .get(*key)
                    .cloned()
                    .unwrap_or_else(|| (*key).to_string());
                let w = measure(ctx, &text, font_id.clone()) + HELP_HINT_GAP + ICON_SLOT;
                if w > worst_entries.2 {
                    worst_entries = (lang, key, w);
                }
            }

            for key in QUICK_SWITCH_LABELS {
                let raw = table
                    .get(*key)
                    .cloned()
                    .unwrap_or_else(|| (*key).to_string());
                // slot 라벨은 2자리 슬롯 번호("10")가 최장 케이스.
                let raw = if key.ends_with("_slot_label") {
                    raw.replace("{}", "10")
                } else {
                    raw
                };
                let display = format!("{}:", raw.trim_end_matches(':').trim());
                let w = measure(ctx, &display, font_id.clone());
                if w > worst_qs.2 {
                    worst_qs = (lang, key, w);
                }
            }
        }
    });

    println!("worst entries.rs label: {worst_entries:?} (column={column_width})");
    println!("worst quick_switch.rs label: {worst_qs:?} (column={column_width})");

    assert!(
        worst_entries.2 <= column_width,
        "entries.rs label '{}' ({}) needs {:.1}px > LABEL_COL_WIDTH {:.1}px — raise the constant",
        worst_entries.1,
        worst_entries.0,
        worst_entries.2,
        column_width
    );
    assert!(
        worst_qs.2 <= column_width,
        "quick_switch.rs label '{}' ({}) needs {:.1}px > LABEL_COL_WIDTH {:.1}px — raise the constant",
        worst_qs.1,
        worst_qs.0,
        worst_qs.2,
        column_width
    );
}
