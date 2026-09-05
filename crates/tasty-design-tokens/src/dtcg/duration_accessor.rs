//! component tier 의 `duration` 토큰을 `&Theme` 접근자로 내는 부분.
//!
//! 치수·색 접근자와 **한 파일에 두지 않는 이유**는 성질이 달라서가 아니라
//! `dtcg.rs` 가 SLOC 게이트(1000)를 넘겼기 때문이다. 가르는 선을 시간 축으로 잡은 것은
//! 이쪽이 가장 최근에 붙었고 다른 두 축과 공유하는 상태가 없어서다.

use super::{ThemeMode, Tier, Token, TokenSet, accessor_fn_name, alias_target};

/// duration component 접근자의 본문 형태.
///
/// 치수와 달리 **zoom 을 곱하지 않는다** — 시간은 UI 배율을 타지 않는다. 그리고
/// semantic 종착에도 `Theme` 필드가 없다(색·치수와 달리 duration 은 테마마다 달라지지
/// 않아 필드로 굽지 않는다). 그래서 형태가 둘뿐이다.
pub(super) enum DurationAccessor {
    /// alias 대상이 다른 component duration 접근자.
    Chain(String),
    /// 종착 리터럴(ms).
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
        .map_err(|_| format!("{own_path}: 터미널 값 파싱 실패 ({terminal}) — 생성 스킵"))
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
