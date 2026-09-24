//! component 시간 토큰의 Theme 접근자를 생성한다.

use super::accessor::accessor_fn_name;
use super::{ThemeMode, Tier, Token, TokenSet, alias_target};

/// 시간은 UI 배율이나 테마 색에 영향을 받지 않는다.
/// 다른 시간 접근자를 호출하거나 고정 밀리초 값을 반환한다.
pub(super) enum DurationAccessor {
    /// alias 대상이 다른 component duration 접근자.
    Chain(String),
    /// 고정 밀리초 값.
    RawMs(f32),
}

/// duration component 접근자의 본문 형태를 고른다.
pub(super) fn resolve_duration_accessor(
    set: &TokenSet,
    token: &Token,
) -> Result<DurationAccessor, String> {
    let own_path = token.path();
    if let Some(target_path) = alias_target(&token.value)
        && let Some(target) = set.get(target_path)
        && target.tier == Tier::Component
    {
        return Ok(DurationAccessor::Chain(accessor_fn_name(&target.name)));
    }
    let terminal = set
        .resolve(&own_path, ThemeMode::Mocha)
        .map_err(|e| format!("{own_path}: {e} — 생성 스킵"))?;
    let stripped = terminal.strip_suffix("ms").unwrap_or(&terminal);
    stripped
        .trim()
        .parse::<f32>()
        .map(DurationAccessor::RawMs)
        .map_err(|_| format!("{own_path}: 최종 값 파싱 실패 ({terminal}) — 생성 스킵"))
}

/// duration 접근자 하나의 `impl Theme` 메서드 텍스트.
pub(super) fn emit_duration_accessor(
    set: &TokenSet,
    token: &Token,
    acc: &DurationAccessor,
) -> String {
    let terminal = set
        .resolve(&token.path(), ThemeMode::Mocha)
        .expect("resolve_duration_accessor 통과 토큰은 resolve 가능");
    let fn_name = accessor_fn_name(&token.name);
    let body = match acc {
        DurationAccessor::Chain(target_fn) => format!("self.{target_fn}()"),
        DurationAccessor::RawMs(v) => format!("Millis({v:?})"),
    };
    format!(
        "\n    /// `{}` → `{}` = {terminal}\n    #[inline]\n    pub fn {fn_name}(&self) -> Millis {{\n        {body}\n    }}\n",
        token.path(),
        token.value,
    )
}
