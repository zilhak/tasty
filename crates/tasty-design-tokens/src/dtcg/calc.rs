//! CSS `calc()`로 정의한 치수 토큰을 읽는다. 원격 canonical이 다른 토큰의 배수 합으로
//! 정의한 값을 손으로 옮기지 않도록, `<수> * var(--tasty-이름)` 항을 `+`로 이은 식만 받는다.
//! 그 밖의 연산은 생성하지 않고 스킵 사유로 남긴다.

use super::{ThemeMode, Token, TokenSet};

/// `calc()` 식의 항 하나.
pub(super) struct Term<'a> {
    pub coef: f32,
    pub token: &'a Token,
}

/// `raw`가 `calc(`로 시작하지 않으면 None. 시작하면 항 목록이나 해석 실패 사유.
pub(super) fn parse<'a>(set: &'a TokenSet, raw: &str) -> Option<Result<Vec<Term<'a>>, String>> {
    let body = raw.trim().strip_prefix("calc(")?.strip_suffix(')');
    let Some(body) = body else {
        return Some(Err(format!("닫는 괄호가 없는 calc 식 ({raw})")));
    };
    Some(body.split('+').map(|term| parse_term(set, term)).collect())
}

fn parse_term<'a>(set: &'a TokenSet, term: &str) -> Result<Term<'a>, String> {
    let (coef, var) = match term.split_once('*') {
        Some((c, v)) => (
            c.trim()
                .parse::<f32>()
                .map_err(|_| format!("calc 항의 계수를 읽지 못함 ({})", term.trim()))?,
            v.trim(),
        ),
        None => (1.0, term.trim()),
    };
    let name = var
        .strip_prefix("var(--tasty-")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| format!("calc 항이 토큰 참조가 아님 ({})", term.trim()))?;
    let token = ["component", "semantic", "primitive"]
        .iter()
        .find_map(|tier| set.get(&format!("{tier}.{name}")))
        .ok_or_else(|| format!("calc 항의 토큰 없음 (--tasty-{name})"))?;
    Ok(Term { coef, token })
}

/// 배율 1의 px 값. 각 항의 최종 값을 숫자로 해석한다.
pub(super) fn px_at_unit_zoom(set: &TokenSet, terms: &[Term<'_>]) -> Result<f32, String> {
    terms.iter().try_fold(0.0, |sum, t| {
        let terminal = set
            .resolve(&t.token.path(), ThemeMode::Mocha)
            .map_err(|e| e.to_string())?;
        let v = terminal
            .strip_suffix("px")
            .unwrap_or(&terminal)
            .trim()
            .parse::<f32>()
            .map_err(|_| format!("calc 항의 최종 값 파싱 실패 ({terminal})"))?;
        Ok(sum + t.coef * v)
    })
}
