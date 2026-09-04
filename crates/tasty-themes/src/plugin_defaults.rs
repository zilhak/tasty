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

/// 두 락 다 맵/집합 조작뿐이라 복구가 맞다. 조용히 두면 결과가 **우선순위 역전**으로
/// 드러난다 — `USER_DEFINED_KINDS` 를 못 읽으면 사용자가 정의한 kind 도 미정의로 보여
/// plugin default 가 사용자 색을 덮어쓴다. 이 모듈 doc 이 세운 우선순위가 조용히 뒤집히는
/// 것이라 흔적이 남아야 한다.
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

/// plugin manifest 의 `[surface_kinds.default_colors]` 한 항목 등록.
/// 사용자 theme 이 같은 kind 를 정의하지 *않은* 경우에 한해 현재 전역 Theme 에도
/// 즉시 머지된다. 사용자 정의가 우선이라 그 외 경우는 PLUGIN_DEFAULTS 저장만 하고
/// theme 변경 시 다음 머지에서 적용된다.
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

/// `install_global` 직후 호출되어 누적된 plugin defaults 를 새 Theme 에 머지.
/// 사용자 정의 kind 는 건드리지 않는다.
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

    /// 이 모듈의 테스트들은 공유 전역 `PLUGIN_DEFAULTS`/`USER_DEFINED_KINDS` 를
    /// `reset()` 으로 clear 후 mutate 하므로, cargo 기본 병렬 실행에서 서로의
    /// 상태를 덮어써 순서 의존 flake 가 난다. 이 락으로 직렬화한다.
    /// 이 전역을 만지는 새 테스트는 반드시 `reset()` 이 반환한 가드를 함수 끝까지
    /// 유지해야 한다.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 전역을 초기화하고 직렬화 가드를 반환한다. 반환된 가드를 이름있는 바인딩
    /// (`let _guard = reset();`) 으로 받아 테스트 함수 끝까지 유지할 것 —
    /// 임시값으로 받으면 즉시 drop 되어 직렬화가 무효가 된다.
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

    /// 복구는 이 자리에 **이미** 있었다 — 이번에 더한 것은 관측뿐이라, "poison 이어도
    /// 우선순위가 지켜진다" 만 보는 테스트는 헬퍼를 되돌려도 그대로 통과한다(변이가 안
    /// 죽는다). 그래서 보고 플래그가 실제로 뒤집혔는지를 함께 본다. 그 플래그가 곧
    /// `tracing::error!` 가 나갔다는 증거이고, `unwrap_or_else(into_inner)` 로
    /// 되돌리면 `false` 로 남는다.
    ///
    /// 조준점을 **사용자가 정의한** kind 로 잡는 것이 요점이다 — 미정의 kind 로 겨누면
    /// plugin default 가 덮어쓰는 것이 정상 동작이라 우선순위 역전이 드러나지 않는다.
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
