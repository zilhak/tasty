//! Plugin 이 manifest 에 선언한 surface 기본 색상의 전역 저장소.
//!
//! 우선순위 (높음 → 낮음):
//! 1. 사용자 theme TOML 의 `[surfaces.<kind>]`
//! 2. plugin manifest 의 `[surface_kinds.default_colors]`
//! 3. `FALLBACK_SURFACE`
//!
//! 사용자 정의가 있는 kind 는 plugin default 가 *덮어쓰지 않는다*.

use std::collections::{BTreeMap, HashSet};
use std::sync::RwLock;

use tasty_type_appearance::theme::{PartialSurfaceTheme, Theme};

/// plugin 이 hello 직후 등록한 default 색. kind → partial.
/// 같은 kind 로 두 번 호출되면 마지막 값으로 교체.
static PLUGIN_DEFAULTS: RwLock<BTreeMap<String, PartialSurfaceTheme>> =
    RwLock::new(BTreeMap::new());

/// 현재 활성 theme 파일의 `[surfaces.<kind>]` 키 집합.
/// `apply_theme` 가 theme 을 로드할 때 갱신. 미정의(None) 면 모든 kind 가 plugin
/// default 머지 대상.
static USER_DEFINED_KINDS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

fn read_user_defined() -> Option<HashSet<String>> {
    USER_DEFINED_KINDS
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

fn is_user_defined(kind: &str) -> bool {
    read_user_defined()
        .as_ref()
        .is_some_and(|set| set.contains(kind))
}

/// plugin manifest 의 `[surface_kinds.default_colors]` 한 항목 등록.
/// 사용자 theme 이 같은 kind 를 정의하지 *않은* 경우에 한해 현재 전역 Theme 에도
/// 즉시 머지된다. 사용자 정의가 우선이라 그 외 경우는 PLUGIN_DEFAULTS 저장만 하고
/// theme 변경 시 다음 머지에서 적용된다.
pub fn add_plugin_surface_default(kind: &str, partial: PartialSurfaceTheme) {
    {
        let mut g = PLUGIN_DEFAULTS.write().unwrap_or_else(|p| p.into_inner());
        g.insert(kind.to_string(), partial.clone());
    }
    if is_user_defined(kind) {
        return;
    }
    crate::global::mutate_theme(|theme| {
        let entry = theme.surface_themes.entry(kind.to_string()).or_default();
        entry.apply_partial(&partial);
    });
}

/// 활성 theme 파일이 정의한 surface kind 집합을 기록한다.
/// `apply_theme` 가 새 theme 을 로드할 때마다 호출.
pub fn record_user_defined_surface_kinds(kinds: HashSet<String>) {
    let mut g = USER_DEFINED_KINDS
        .write()
        .unwrap_or_else(|p| p.into_inner());
    *g = Some(kinds);
}

/// `install_global` 직후 호출되어 누적된 plugin defaults 를 새 Theme 에 머지.
/// 사용자 정의 kind 는 건드리지 않는다.
pub fn apply_plugin_defaults_to(theme: &mut Theme) {
    let defaults = PLUGIN_DEFAULTS.read().unwrap_or_else(|p| p.into_inner());
    let user_def = read_user_defined();
    for (kind, partial) in defaults.iter() {
        if user_def.as_ref().is_some_and(|set| set.contains(kind)) {
            continue;
        }
        let entry = theme.surface_themes.entry(kind.clone()).or_default();
        entry.apply_partial(partial);
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // reason: 테스트 fixture 의 합성 색상.
mod tests {
    use super::*;
    use tasty_type_appearance::color::HexColor;
    use tasty_type_appearance::theme::SurfaceTheme;

    fn reset() {
        PLUGIN_DEFAULTS
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        *USER_DEFINED_KINDS
            .write()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }

    fn red_partial() -> PartialSurfaceTheme {
        PartialSurfaceTheme {
            focused_bg: Some(HexColor::from_rgb(0xff, 0, 0)),
            focused_fg: Some(HexColor::from_rgb(0xff, 0xff, 0xff)),
            unfocused_bg: Some(HexColor::from_rgb(0x80, 0, 0)),
            unfocused_fg: Some(HexColor::from_rgb(0xcc, 0xcc, 0xcc)),
        }
    }

    fn green_partial() -> PartialSurfaceTheme {
        PartialSurfaceTheme {
            focused_bg: Some(HexColor::from_rgb(0, 0xff, 0)),
            focused_fg: Some(HexColor::from_rgb(0, 0, 0)),
            unfocused_bg: Some(HexColor::from_rgb(0, 0x80, 0)),
            unfocused_fg: Some(HexColor::from_rgb(0x33, 0x33, 0x33)),
        }
    }

    #[test]
    fn apply_to_theme_fills_unknown_kind() {
        reset();
        let mut g = PLUGIN_DEFAULTS.write().unwrap_or_else(|p| p.into_inner());
        g.insert("foo".to_string(), red_partial());
        drop(g);

        let mut t = crate::mocha_fallback();
        apply_plugin_defaults_to(&mut t);

        let st = t.surface("foo");
        assert_eq!(st.focused_bg, HexColor::from_rgb(0xff, 0, 0));
    }

    #[test]
    fn apply_to_theme_skips_user_defined_kind() {
        reset();
        let mut g = PLUGIN_DEFAULTS.write().unwrap_or_else(|p| p.into_inner());
        g.insert("foo".to_string(), red_partial());
        drop(g);

        let mut user_set = HashSet::new();
        user_set.insert("foo".to_string());
        record_user_defined_surface_kinds(user_set);

        let mut t = crate::mocha_fallback();
        // pre-seed user theme value for "foo"
        t.surface_themes
            .insert("foo".to_string(), SurfaceTheme::default());
        apply_partial_to_user_seed(&mut t);

        apply_plugin_defaults_to(&mut t);

        let st = t.surface("foo");
        // plugin red 가 아니라 user-seed green 이 살아있어야 함.
        assert_eq!(st.focused_bg, HexColor::from_rgb(0, 0xff, 0));
    }

    fn apply_partial_to_user_seed(t: &mut Theme) {
        if let Some(st) = t.surface_themes.get_mut("foo") {
            st.apply_partial(&green_partial());
        }
    }
}
