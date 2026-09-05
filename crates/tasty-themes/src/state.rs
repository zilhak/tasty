//! 테마 적용·resolve 흐름. settings 의 두 레이어(`theme_base`, `theme_overrides`)를
//! `Theme` 인스턴스로 합치고, 테마 변경 이벤트에서 base 를 누적 mutate 한다.
//!
//! 이 crate 의 "이벤트에서 인스턴스를 수정/merge" 책임의 본체.

use crate::apply_context::ThemeApplyContext;
use crate::fallback::mocha_fallback_colors;
use crate::file::ThemeFile;
use crate::global::set_theme;
use crate::scan::scan_themes;
use crate::store::{BUILTIN_MOCHA_ID, rewrite_mocha_fallback};
use tasty_type_appearance::theme::Theme;

/// 전역 `Theme` 이 색 파일이 아니라 **설정**에서 받아와야 하는 값들.
///
/// 하나로 묶는 이유는 인자를 빠뜨리는 것이 **이미 사고를 냈기** 때문이다 —
/// `ui_zoom` 을 안 실은 install 이 전역 Theme 을 배율 1.0 으로 되돌려 다른 윈도우의 UI 가
/// 줄어든 적이 있고, 그 사유가 호출부 주석에 남아 있다. 값이 하나 더 늘 때마다 인자가
/// 하나 더 느는 형태였으면 그 사고가 값마다 반복된다. 묶어 두면 새 값이 생겨도 호출부
/// 모양이 안 변하고, 채우는 자리는 [`ThemeRuntime`] 을 만드는 한 곳으로 모인다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeRuntime {
    /// host UI zoom 배율 (`appearance.ui_scale_factor()`).
    pub ui_zoom: f32,
    /// 접근성 "모션 감소"(`accessibility.reduced_motion`).
    pub reduced_motion: bool,
}

impl Default for ThemeRuntime {
    /// 설정을 모르는 문맥(테스트·부팅 이전)의 값 — 배율 1.0, 모션 감소 꺼짐.
    fn default() -> Self {
        Self {
            ui_zoom: 1.0,
            reduced_motion: false,
        }
    }
}

/// 두 레이어를 합쳐 실제 적용될 `Theme` 인스턴스를 만든다.
/// `theme_base` 위에 `theme_overrides` 의 `Some` 필드만 덮어쓴 결과.
pub fn resolve<C: ThemeApplyContext + ?Sized>(ctx: &C) -> Theme {
    resolve_with_runtime(ctx, ThemeRuntime::default())
}

/// `resolve()` 의 일반화 — 설정에서 오는 런타임 값([`ThemeRuntime`])을 반영한다.
pub fn resolve_with_runtime<C: ThemeApplyContext + ?Sized>(ctx: &C, rt: ThemeRuntime) -> Theme {
    let mut colors = ctx.theme_base().clone();
    colors.apply_partial(ctx.theme_overrides());
    let mut t = Theme::with_colors_and_zoom(colors, ctx.theme_is_light(), rt.ui_zoom);
    t.reduced_motion = rt.reduced_motion;
    t
}

/// `resolve()` 결과를 전역 `Theme` 에 박는다.
/// 등록된 plugin surface defaults 도 함께 머지 (사용자 정의 kind 는 보존).
pub fn install_global<C: ThemeApplyContext + ?Sized>(ctx: &C) {
    install_global_with_runtime(ctx, ThemeRuntime::default())
}

/// `install_global` 의 일반화 — 설정에서 오는 런타임 값을 반영한 Theme 으로 전역 슬롯
/// 을 갱신한다.
pub fn install_global_with_runtime<C: ThemeApplyContext + ?Sized>(ctx: &C, rt: ThemeRuntime) {
    let mut t = resolve_with_runtime(ctx, rt);
    crate::plugin_defaults::apply_plugin_defaults_to(&mut t);
    set_theme(t);
}

/// id 로 테마를 적용한다.
/// - `scan_themes()` 캐시에서 해당 id 의 `ThemeFile` 을 찾는다.
/// - 없으면 mocha 로 fallback. id == "mocha" 인데도 못 찾으면 `MOCHA_FALLBACK_COLORS` 사용
///   + `rewrite_mocha_fallback()` 으로 디스크 복구.
/// - 찾은 partial 을 `theme_base` 에 apply (누락 필드는 base 유지).
/// - `theme_overrides` 클리어, `theme_id` 갱신, `is_light` 가 파일에 있으면 갱신.
pub fn apply_theme<C: ThemeApplyContext + ?Sized>(ctx: &mut C, id: &str) {
    let resolved_id = apply_inner(ctx, id, /* allow_mocha_recursion */ true);
    ctx.set_theme_id(&resolved_id);
    ctx.theme_overrides_mut().clear();
}

#[allow(clippy::cognitive_complexity)] // complexity-exempt: 테마 적용 3갈래(발견/mocha 특수 폴백/재귀 폴백) — mocha 분기가 디스크 복구+상수 적용 절차를 통째로 안고 있어 쪼개면 ctx 전달 인자만 늘어남. 재귀 호출은 allow_recursion 가드로 1회 종료 보장.
fn apply_inner<C: ThemeApplyContext + ?Sized>(
    ctx: &mut C,
    id: &str,
    allow_recursion: bool,
) -> String {
    let entries = scan_themes();
    if let Some(entry) = entries.iter().find(|e| e.id == id) {
        let (partial, is_light) = entry.file.to_partial();
        let user_kinds: std::collections::HashSet<String> =
            entry.file.surfaces.keys().cloned().collect();
        crate::plugin_defaults::record_user_defined_surface_kinds(user_kinds);
        ctx.theme_base_mut().apply_partial(&partial);
        if let Some(l) = is_light {
            ctx.set_theme_is_light(l);
        }
        return id.to_string();
    }

    // 찾을 수 없음. mocha 로 fallback.
    if id == BUILTIN_MOCHA_ID {
        // mocha 자체가 캐시에 없다 → 디스크 복구 후 in-memory const 적용.
        if let Err(e) = rewrite_mocha_fallback() {
            tracing::warn!("failed to rewrite mocha fallback: {e}");
        }
        if let Err(e) = crate::scan::rescan() {
            tracing::debug!("rescan after mocha rewrite failed: {e}");
        }
        let const_file = ThemeFile::parse(crate::MOCHA_TOML_TEXT)
            .expect("embedded mocha.toml must parse (compile-time guaranteed)");
        let user_kinds: std::collections::HashSet<String> =
            const_file.surfaces.keys().cloned().collect();
        crate::plugin_defaults::record_user_defined_surface_kinds(user_kinds);
        let (partial, is_light) = const_file.to_partial();
        // mocha 는 풀 세트라 base 가 통째로 덮어쓰여진다 — 안전을 위해 먼저 fallback 로 초기화.
        *ctx.theme_base_mut() = mocha_fallback_colors();
        ctx.theme_base_mut().apply_partial(&partial);
        if let Some(l) = is_light {
            ctx.set_theme_is_light(l);
        } else {
            ctx.set_theme_is_light(false);
        }
        return BUILTIN_MOCHA_ID.to_string();
    }

    tracing::warn!("theme '{id}' not found; falling back to mocha");
    if allow_recursion {
        return apply_inner(ctx, BUILTIN_MOCHA_ID, false);
    }
    BUILTIN_MOCHA_ID.to_string()
}

#[cfg(test)]
// 테스트 더미 색 생성 — 정상 운영 경로 아님.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use tasty_type_appearance::color::HexColor;
    use tasty_type_appearance::theme::{PartialColors, ThemeColors};

    /// 테스트용 ctx — `AppearanceSettings` 의 핵심 필드만 흉내낸다.
    struct TestCtx {
        id: String,
        base: ThemeColors,
        overrides: PartialColors,
        is_light: bool,
    }

    impl TestCtx {
        fn mocha() -> Self {
            Self {
                id: "mocha".to_string(),
                base: mocha_fallback_colors(),
                overrides: PartialColors::default(),
                is_light: false,
            }
        }
    }

    impl ThemeApplyContext for TestCtx {
        fn theme_id(&self) -> &str {
            &self.id
        }
        fn set_theme_id(&mut self, id: &str) {
            self.id = id.to_string();
        }
        fn theme_base(&self) -> &ThemeColors {
            &self.base
        }
        fn theme_base_mut(&mut self) -> &mut ThemeColors {
            &mut self.base
        }
        fn theme_overrides(&self) -> &PartialColors {
            &self.overrides
        }
        fn theme_overrides_mut(&mut self) -> &mut PartialColors {
            &mut self.overrides
        }
        fn theme_is_light(&self) -> bool {
            self.is_light
        }
        fn set_theme_is_light(&mut self, v: bool) {
            self.is_light = v;
        }
    }

    /// 이 값이 `Theme` 까지 실려 가지 않으면 위젯이 설정을 읽을 방법이 없다 —
    /// 실제로 그랬고(넘기는 자리가 0 이었다), 그래서 이 축을 여기서 붙잡는다.
    #[test]
    fn runtime_values_reach_the_resolved_theme() {
        let ctx = TestCtx::mocha();
        let t = resolve_with_runtime(
            &ctx,
            ThemeRuntime {
                ui_zoom: 1.0,
                reduced_motion: true,
            },
        );
        assert!(t.reduced_motion);
        // 기본(설정을 모르는 문맥)은 꺼짐이어야 한다 — 모션을 임의로 끄지 않는다.
        assert!(!resolve(&ctx).reduced_motion);
    }

    #[test]
    fn resolve_overlays_overrides_on_base() {
        let mut ctx = TestCtx::mocha();
        ctx.overrides.blue = Some(HexColor::from_rgb(0, 0xff, 0));
        let t = resolve(&ctx);
        assert_eq!(t.blue, HexColor::from_rgb(0, 0xff, 0));
        // 그 외 필드는 base 그대로
        assert_eq!(t.crust, mocha_fallback_colors().crust);
        assert!(!t.is_light);
    }

    #[test]
    fn resolve_respects_is_light() {
        let mut ctx = TestCtx::mocha();
        ctx.is_light = true;
        let t = resolve(&ctx);
        assert!(t.is_light);
        // is_light=true 면 overlay 가 검정 기반.
        assert_eq!(t.hover_overlay.r, 0);
    }

    // apply_theme 통합 테스트는 scan_themes 캐시가 디스크 의존이라
    // 단위 테스트로는 다루기 어렵다 (TempDir 으로 tasty_home 재정의 불가).
    // 통합 시나리오는 본 바이너리에서 자체 검증으로 처리.
}
