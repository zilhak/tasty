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
use std::sync::atomic::AtomicBool;

use tasty_type_appearance::theme::{PartialSurfaceTheme, Theme};

/// plugin 이 hello 직후 등록한 default 색. kind → partial.
/// 같은 kind 로 두 번 호출되면 마지막 값으로 교체.
static PLUGIN_DEFAULTS: RwLock<BTreeMap<String, PartialSurfaceTheme>> =
    RwLock::new(BTreeMap::new());

/// 현재 활성 theme 파일의 `[surfaces.<kind>]` 키 집합.
/// `apply_theme` 가 theme 을 로드할 때 갱신. 미정의(None) 면 모든 kind 가 plugin
/// default 머지 대상.
static USER_DEFINED_KINDS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// poison 뒤에도 사용자 정의 종류를 보존해 플러그인 색상이 덮어쓰지 않도록 한다.
/// 복구 시 최초 한 번 로그를 남긴다.
pub(crate) const DEFAULTS_WHAT: &str = "the plugin surface default store";
pub(crate) static DEFAULTS_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

fn read_user_defined() -> Option<HashSet<String>> {
    tasty_utils::poison::recover_read(
        USER_DEFINED_KINDS.read(),
        DEFAULTS_WHAT,
        &DEFAULTS_POISON_REPORTED,
    )
    .clone()
}

fn is_user_defined(kind: &str) -> bool {
    read_user_defined()
        .as_ref()
        .is_some_and(|set| set.contains(kind))
}

/// 플러그인의 기본 색을 저장한다. 사용자가 해당 종류를 정의하지 않았다면 전역 테마에도 적용한다.
/// 사용자 정의가 있으면 저장만 하며 이후 테마 적용에서도 사용자 정의가 우선한다.
pub fn add_plugin_surface_default(kind: &str, partial: PartialSurfaceTheme) {
    {
        let mut g = tasty_utils::poison::recover_write(
            PLUGIN_DEFAULTS.write(),
            DEFAULTS_WHAT,
            &DEFAULTS_POISON_REPORTED,
        );
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
    let mut g = tasty_utils::poison::recover_write(
        USER_DEFINED_KINDS.write(),
        DEFAULTS_WHAT,
        &DEFAULTS_POISON_REPORTED,
    );
    *g = Some(kinds);
}

/// 전역 테마 설치 전에 플러그인 기본 색을 합친다. 사용자 정의 종류는 그대로 둔다.
pub fn apply_plugin_defaults_to(theme: &mut Theme) {
    let defaults = tasty_utils::poison::recover_read(
        PLUGIN_DEFAULTS.read(),
        DEFAULTS_WHAT,
        &DEFAULTS_POISON_REPORTED,
    );
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
    use std::sync::{Mutex, MutexGuard};
    use tasty_type_appearance::color::HexColor;
    use tasty_type_appearance::theme::SurfaceTheme;

    /// 전역 상태를 초기화하는 검사들이 서로 간섭하지 않도록 직렬화한다.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 초기화 후 반환한 가드를 이름 있는 변수로 받아 검사 끝까지 유지해야 한다.
    fn reset() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        PLUGIN_DEFAULTS
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        *USER_DEFINED_KINDS
            .write()
            .unwrap_or_else(|p| p.into_inner()) = None;
        guard
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
        let _guard = reset();
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
        let _guard = reset();
        let mut g = PLUGIN_DEFAULTS.write().unwrap_or_else(|p| p.into_inner());
        g.insert("foo".to_string(), red_partial());
        drop(g);

        let mut user_set = HashSet::new();
        user_set.insert("foo".to_string());
        record_user_defined_surface_kinds(user_set);

        let mut t = crate::mocha_fallback();
        t.surface_themes
            .insert("foo".to_string(), SurfaceTheme::default());
        apply_partial_to_user_seed(&mut t);

        apply_plugin_defaults_to(&mut t);

        let st = t.surface("foo");
        // 사용자 정의 색은 플러그인 기본색으로 덮어쓰지 않아야 한다.
        assert_eq!(st.focused_bg, HexColor::from_rgb(0, 0xff, 0));
    }

    fn apply_partial_to_user_seed(t: &mut Theme) {
        if let Some(st) = t.surface_themes.get_mut("foo") {
            st.apply_partial(&green_partial());
        }
    }

    /// poison 복구가 보고되고 사용자 정의 종류의 우선순위도 유지되는지 확인한다.
    #[test]
    fn a_poisoned_default_store_keeps_the_user_priority_and_says_so() {
        let _guard = reset();
        record_user_defined_surface_kinds(HashSet::from(["foo".to_string()]));
        add_plugin_surface_default("foo", red_partial());

        let panicked = std::thread::spawn(|| {
            let _held = USER_DEFINED_KINDS.write().expect("not poisoned yet");
            panic!("poison the user-defined kind set");
        })
        .join();
        assert!(panicked.is_err());
        assert!(
            USER_DEFINED_KINDS.read().is_err(),
            "the kind-set lock must be poisoned now"
        );

        let mut t = crate::mocha_fallback();
        t.surface_themes
            .insert("foo".to_string(), SurfaceTheme::default());
        apply_partial_to_user_seed(&mut t);
        apply_plugin_defaults_to(&mut t);

        assert_eq!(
            t.surface("foo").focused_bg,
            HexColor::from_rgb(0, 0xff, 0),
            "a poisoned kind set must not let the plugin default overwrite the user colour"
        );
        assert!(
            DEFAULTS_POISON_REPORTED.load(std::sync::atomic::Ordering::Relaxed),
            "the poison must have been reported once"
        );
    }
}
