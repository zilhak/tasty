//! em 단위 자간 토큰의 Theme 접근자를 생성한다.
//!
//! 자간은 글자 크기에 비례하므로 `LogicalPx` 상수로 만들 수 없다. 접근자는 글자 크기를
//! 받아 `em × 글자 크기`를 돌려주며, egui `TextFormat::extra_letter_spacing`에 넘긴다.
//! 글자 크기가 이미 UI 배율을 반영하므로 접근자는 `ui_zoom`을 다시 곱하지 않는다.

use super::accessor::accessor_fn_name;
use super::{ThemeMode, Tier, Token, TokenSet, alias_target};

/// 자간 접근자의 본문 형태.
enum TrackingAccessor {
    /// alias 대상이 다른 em 토큰(semantic·component) — 그 접근자를 호출한다.
    Chain(String),
    /// primitive 또는 리터럴 em 값.
    RawEm(f32),
}

/// 자간 토큰이면 em 수치를 돌려준다. 최종 값이 em 인 dimension 이거나, 이름이 자간 계열
/// (`letter-spacing-*`, `*-tracking`)이고 최종 값이 단위 없는 0 인 토큰이다. CSS 자간 0 은
/// 단위와 무관하므로 0em 으로 읽는다.
pub(super) fn terminal_em(set: &TokenSet, token: &Token) -> Option<f32> {
    if token.ty != "dimension" {
        return None;
    }
    let terminal = set.resolve(&token.path(), ThemeMode::Mocha).ok()?;
    if let Some(em) = terminal.strip_suffix("em") {
        return em.trim().parse::<f32>().ok();
    }
    let tracking_family =
        token.name.starts_with("letter-spacing-") || token.name.ends_with("-tracking");
    let zero = terminal.trim().parse::<f32>().ok().filter(|v| *v == 0.0);
    zero.filter(|_| tracking_family)
}

fn resolve_tracking_accessor(set: &TokenSet, token: &Token, em: f32) -> TrackingAccessor {
    let chained = alias_target(&token.value)
        .and_then(|p| set.get(p))
        .filter(|t| t.tier != Tier::Primitive && terminal_em(set, t).is_some());
    match chained {
        Some(target) => TrackingAccessor::Chain(accessor_fn_name(&target.name)),
        None => TrackingAccessor::RawEm(em),
    }
}

fn emit_tracking_accessor(set: &TokenSet, token: &Token, acc: &TrackingAccessor) -> String {
    let terminal = set
        .resolve(&token.path(), ThemeMode::Mocha)
        .expect("terminal_em 통과 토큰은 resolve 가능");
    let fn_name = accessor_fn_name(&token.name);
    let body = match acc {
        TrackingAccessor::Chain(target_fn) => format!("self.{target_fn}(font_size)"),
        TrackingAccessor::RawEm(v) => format!("LogicalPx({v:?} * font_size.value())"),
    };
    format!(
        "\n    /// `{}` → `{}` = {terminal}\n    #[inline]\n    pub fn {fn_name}(&self, font_size: LogicalPx) -> LogicalPx {{\n        {body}\n    }}\n",
        token.path(),
        token.value,
    )
}

/// semantic·component 계층의 em 자간 접근자 파일을 만든다. primitive는 접근자를 만들지 않는다.
pub(super) fn generate_tracking_accessors(set: &TokenSet) -> String {
    let mut body = String::new();
    for tier in [Tier::Semantic, Tier::Component] {
        for token in set.iter().filter(|t| t.tier == tier) {
            if let Some(em) = terminal_em(set, token) {
                let acc = resolve_tracking_accessor(set, token, em);
                body.push_str(&emit_tracking_accessor(set, token, &acc));
            }
        }
    }
    let header = "//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.\n\
                  //! 재생성: `cargo run -p tasty-design-tokens --bin generate`.\n\
                  //!\n\
                  //! em 단위 자간 토큰을 Theme를 통해 읽는다. 접근자는 글자 크기를 받아\n\
                  //! `em × 글자 크기`를 egui `extra_letter_spacing` 값으로 돌려준다.\n\
                  //! 글자 크기가 이미 UI 배율을 반영하므로 배율을 다시 곱하지 않는다.\n\n\
                  use tasty_type_geometry::length::LogicalPx;\n\n\
                  impl crate::theme::Theme {";
    let mut file = header.to_string();
    file.push_str(&body);
    file.push_str("}\n");
    file
}
