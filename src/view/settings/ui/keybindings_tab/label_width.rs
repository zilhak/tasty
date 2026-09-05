//! `LABEL_COL_WIDTH` 회귀 가드 — en/ko/ja 전 라벨을 실제 프로덕션 egui 폰트
//! 스택으로 실측해 고정폭 컬럼을 넘는 라벨이 없는지 확인한다.
//!
//! **`tasty_egui_theme::apply_theme_to_egui` 호출이 핵심이다.** 이걸 빼먹으면
//! `TextStyle::Body`가 egui 기본값(12.5px)으로 남는데, 실제 Settings 창은 매
//! 프레임 `apply_theme_to_egui`로 `Theme::font_size_body`(13.0px)를 박아 넣는다
//! (`src/gfx/gpu/egui_bridge.rs`). 최초 구현 때 이 호출을 빠뜨려 12.5px 기준
//! 237px로 측정했고, Gate4 리뷰의 독립 재현은 반대로 egui 교과서 기본값인
//! 14.0px를 가정해 273.5px로 측정했다 — 두 값 모두 실제 런타임 스타일과
//! 다르다. 13.0px(진짜 값)로 다시 재면 237.28 ~ 255.28px 사이(엔트리는 `(?)`
//! 아이콘 슬롯 18px 포함)이고, 그중 최장은 ja `screenshot_to_clipboard_label`
//! 255.28px다. `LABEL_COL_WIDTH`(`keybindings_tab.rs`)는 여기에 여유를 두고
//! 288px로 고정했다.
//!
//! lang 파일은 전역 i18n `OnceLock`(`crate::i18n::init`)을 거치지 않고 직접
//! `include_str!` + `toml` 파싱으로 읽는다 — `cargo test`는 모든 테스트를 한
//! 프로세스에서 돌리므로 `OnceLock` 기반 전역 초기화는 테스트 실행 순서에 따라
//! 다른 언어를 덮어써 버릴 수 있어 재현성이 없다.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

use std::collections::HashMap;

/// entries.rs 의 `(?)` 아이콘 슬롯 예약폭 — `Theme.spacing_xs`(4) + `icon_glyph_size_sm`(14).
///
/// **배율 1.0 에서 언 사본이다.** entries.rs 는 두 값을 모두 `Theme` 에서 읽으므로
/// 배율이 오르면 그쪽 슬롯은 넓어지고 여기는 안 넓어진다. 이 파일이 재는 것은 라벨 컬럼
/// 폭의 **상대 비교**(어느 라벨이 더 긴가)라 배율이 곱해져도 순서가 안 바뀌어 지금은
/// 하중을 받지 않는다 — 그래서 사본을 두되 언 사본이라는 것을 적어 둔다.
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

/// entries.rs 의 모든 서브탭(General/Workspace/Pane/Tab/Surface/Clipboard/Zoom/
/// Image/Explorer) 라벨 i18n key 전체.
const ENTRY_LABELS: &[&str] = &[
    "settings.keybindings.toggle_settings_label",
    "settings.keybindings.toggle_notifications_label",
    "settings.keybindings.toggle_dag_list_label",
    "settings.keybindings.restore_closed_label",
    "settings.keybindings.new_window_label",
    "settings.keybindings.quit_label",
    "settings.keybindings.quit_immediate_label",
    "settings.keybindings.quit_minimize_label",
    "settings.keybindings.minimize_window_label",
    "settings.keybindings.maximize_window_label",
    "settings.keybindings.close_window_label",
    "settings.keybindings.new_workspace_label",
    "settings.keybindings.rename_workspace_label",
    "settings.keybindings.rename_workspace_subtitle_label",
    "settings.keybindings.close_workspace_label",
    "settings.keybindings.split_pane_vertical_label",
    "settings.keybindings.split_pane_horizontal_label",
    "settings.keybindings.focus_pane_next_label",
    "settings.keybindings.focus_pane_prev_label",
    "settings.keybindings.close_pane_label",
    "settings.keybindings.new_tab_label",
    "settings.keybindings.open_markdown_label",
    "settings.keybindings.next_tab_label",
    "settings.keybindings.prev_tab_label",
    "settings.keybindings.rename_tab_label",
    "settings.keybindings.close_active_label",
    "settings.keybindings.split_surface_vertical_label",
    "settings.keybindings.split_surface_horizontal_label",
    "settings.keybindings.focus_surface_next_label",
    "settings.keybindings.focus_surface_prev_label",
    "settings.keybindings.convert_surface_label",
    "settings.keybindings.convert_to_markdown_label",
    "settings.keybindings.close_surface_label",
    "settings.keybindings.copy_label",
    "settings.keybindings.copy_path_label",
    "settings.keybindings.cut_label",
    "settings.keybindings.select_all_label",
    "settings.keybindings.paste_label",
    "settings.keybindings.screenshot_to_clipboard_label",
    "settings.keybindings.zoom_in_label",
    "settings.keybindings.zoom_out_label",
    "settings.keybindings.zoom_reset_label",
    "settings.keybindings.image_undo_label",
    "settings.keybindings.image_redo_label",
    "settings.keybindings.explorer_refresh_label",
    "settings.keybindings.explorer_go_up_label",
];

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

/// en/ko/ja 전체 라벨 중 최장 실측 폭이 `LABEL_COL_WIDTH` 를 넘지 않는지 확인한다.
/// 새 언어가 추가되거나 번역이 길어지면 이 테스트가 실패해 알려준다.
#[test]
fn labels_fit_within_fixed_column() {
    let ctx = egui::Context::default();
    tasty_egui_theme::install_cjk_fallback(&ctx);
    // 프로덕션과 동일한 TextStyle::Body 크기(Theme::font_size_body, 13.0px)를
    // 반드시 적용해야 한다 — 없으면 egui 기본값(12.5px)으로 측정돼 실제보다
    // 좁게 나온다(최초 구현의 버그).
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

            for key in ENTRY_LABELS {
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
