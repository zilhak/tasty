//! OS 네이티브 표면(macOS NSMenu / 시스템 트레이 / Windows Jump List)이 조회하는
//! 번역 키가 `lang/{en,ko,ja}.toml` 세 파일 모두에 존재하는지 검증한다.
//!
//! 배경: 이 표면들은 플랫폼 전용 파일(`#[cfg]`)에서 `t()` 를 호출하므로 Linux CI 는
//! macOS/Windows 경로를 컴파일하지 않고, 런타임 키 누락은 화면에 키 문자열이 그대로
//! 노출되는 형태로만 드러난다(`docs/dev-guide/i18n.md` — 키 미존재 시 키 반환).
//! lang 파일은 플랫폼 무관이므로 여기서 키 존재를 세 언어에 대해 한 번에 고정한다.
//! 앱 이름을 결합하는 macOS 항목은 `t_fmt` 로 `{}` 하나를 채우므로 placeholder 개수도
//! 함께 검증한다.

use std::collections::BTreeMap;

/// 플랫폼 소스가 조회하는 키 전체. 소스의 `t("…")` / `t_fmt("…", …)` 리터럴과 동기.
const REQUIRED_KEYS: &[&str] = &[
    // src/platform/macos_delegate.rs
    "menu.macos.about",
    "menu.macos.hide",
    "menu.macos.hide_others",
    "menu.macos.show_all",
    "menu.macos.quit",
    "menu.macos.file",
    "menu.macos.new_window",
    "menu.macos.window",
    "menu.macos.minimize",
    "menu.macos.zoom",
    "menu.macos.close_window",
    // src/platform/system_tray.rs
    "tray.show_window",
    "tray.new_window",
    "tray.quit",
    "tray.tooltip",
    // src/platform/jump_list.rs
    "jump_list.new_window",
    "jump_list.new_window_desc",
];

/// `t_fmt(key, &process_name)` 로 앱 이름을 끼워 넣는 키 — `{}` 가 정확히 1 개여야 한다.
const APP_NAME_FMT_KEYS: &[&str] = &["menu.macos.about", "menu.macos.hide", "menu.macos.quit"];

const LANG_FILES: &[(&str, &str)] = &[
    ("en", include_str!("../lang/en.toml")),
    ("ko", include_str!("../lang/ko.toml")),
    ("ja", include_str!("../lang/ja.toml")),
];

/// `[a.b] c = "v"` 를 `a.b.c` 점 키로 평탄화한다(`tasty-i18n` 의 로더와 같은 규칙).
fn flatten(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let full = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&full, v, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

fn load(lang: &str, toml_str: &str) -> BTreeMap<String, String> {
    let value: toml::Value = toml_str
        .parse()
        .unwrap_or_else(|e| panic!("lang/{lang}.toml 파싱 실패: {e}"));
    let mut out = BTreeMap::new();
    flatten("", &value, &mut out);
    out
}

#[test]
fn native_surface_keys_exist_in_all_languages() {
    for (lang, toml_str) in LANG_FILES {
        let table = load(lang, toml_str);
        let missing: Vec<&str> = REQUIRED_KEYS
            .iter()
            .copied()
            .filter(|k| table.get(*k).is_none_or(|v| v.trim().is_empty()))
            .collect();
        assert!(
            missing.is_empty(),
            "lang/{lang}.toml 에 네이티브 표면 키가 없거나 비어 있음: {missing:?}"
        );
    }
}

#[test]
fn app_name_keys_have_exactly_one_placeholder() {
    for (lang, toml_str) in LANG_FILES {
        let table = load(lang, toml_str);
        for key in APP_NAME_FMT_KEYS {
            let value = &table[*key];
            assert_eq!(
                value.matches("{}").count(),
                1,
                "lang/{lang}.toml `{key}` = {value:?} — 앱 이름 placeholder `{{}}` 는 정확히 1 개"
            );
        }
    }
}

#[test]
fn non_fmt_keys_have_no_placeholder() {
    for (lang, toml_str) in LANG_FILES {
        let table = load(lang, toml_str);
        for key in REQUIRED_KEYS
            .iter()
            .filter(|k| !APP_NAME_FMT_KEYS.contains(k))
        {
            let value = &table[*key];
            assert!(
                !value.contains("{}"),
                "lang/{lang}.toml `{key}` = {value:?} — `t()` 로 조회하는 키에 placeholder 가 있음"
            );
        }
    }
}
