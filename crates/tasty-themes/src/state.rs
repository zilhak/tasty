//! 기본 색과 사용자 변경분을 병합하고 테마 선택에 맞춰 설정을 갱신한다.

use crate::apply_context::ThemeApplyContext;
use crate::fallback::mocha_fallback_colors;
use crate::file::ThemeFile;
use crate::global::set_theme;
use crate::scan::scan_themes;
use crate::store::{BUILTIN_MOCHA_ID, rewrite_mocha_fallback};
use tasty_type_appearance::theme::Theme;

/// 테마 색 파일과 별도로 설정에서 전달해야 하는 UI 배율·모션 설정.
/// 새 Theme를 설치할 때 이 값들도 함께 전달한다.
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

/// 병합한 테마와 플러그인 기본 색을 전역에 설치한다. 사용자 정의 종류는 유지한다.
pub fn install_global<C: ThemeApplyContext + ?Sized>(ctx: &C) {
    install_global_with_runtime(ctx, ThemeRuntime::default())
}

/// 런타임 설정을 반영해 전역 테마를 설치한다.
pub fn install_global_with_runtime<C: ThemeApplyContext + ?Sized>(ctx: &C, rt: ThemeRuntime) {
    let mut t = resolve_with_runtime(ctx, rt);
    crate::plugin_defaults::apply_plugin_defaults_to(&mut t);
    set_theme(t);
}

/// 캐시에서 테마를 찾아 기본 색에 병합하고 사용자 변경분을 비운다.
/// 없는 ID는 Mocha로 대체하며 Mocha도 없으면 파일 복구를 시도하고 내장 값을 사용한다.
/// ID와 파일에 명시된 밝기 모드도 갱신한다. 전역 테마 설치는 별도로 호출해야 한다.
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

    if id == BUILTIN_MOCHA_ID {
        // 파일 복구가 실패해도 내장 색상으로 계속 진행한다.
        if let Err(e) = rewrite_mocha_fallback() {
            tracing::warn!("failed to rewrite mocha fallback: {e}");
        }
        if let Err(e) = crate::scan::rescan() {
            tracing::debug!("rescan after mocha rewrite failed: {e}");
        }
        let const_file = ThemeFile::parse(crate::MOCHA_TOML_TEXT)
            .expect("embedded mocha.toml must be valid TOML");
        let user_kinds: std::collections::HashSet<String> =
            const_file.surfaces.keys().cloned().collect();
        crate::plugin_defaults::record_user_defined_surface_kinds(user_kinds);
        let (partial, is_light) = const_file.to_partial();
        // 현재 기본 색을 내장 전체 색상으로 먼저 초기화한다.
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

    /// 설정에서 전달한 모션 감소 값이 Theme에 반영되는지 확인한다.
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
        assert!(!resolve(&ctx).reduced_motion);
    }

    #[test]
    fn resolve_overlays_overrides_on_base() {
        let mut ctx = TestCtx::mocha();
        ctx.overrides.blue = Some(HexColor::from_rgb(0, 0xff, 0));
        let t = resolve(&ctx);
        assert_eq!(t.blue, HexColor::from_rgb(0, 0xff, 0));
        assert_eq!(t.crust, mocha_fallback_colors().crust);
        assert!(!t.is_light);
    }

    #[test]
    fn resolve_respects_is_light() {
        let mut ctx = TestCtx::mocha();
        ctx.is_light = true;
        let t = resolve(&ctx);
        assert!(t.is_light);
        assert_eq!(t.hover_overlay.r, 0);
    }

    // apply_theme의 전역 스캔 캐시는 이 단위 검사에서 실제 디스크와 격리하지 않는다.
}
