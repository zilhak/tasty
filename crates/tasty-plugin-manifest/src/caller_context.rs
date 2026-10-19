//! CLI 호출자 문맥 선언. 다른 프로그램이 tasty CLI를 실행할 때 남긴 환경변수로
//! 호출한 대화를 알아보는 규칙이다. 본체 CLI는 선언을 해석만 하고 제품 이름을 알지 않는다.

use serde::Deserialize;

use crate::validators::is_valid_env_name;

/// `_tasty_context` 안에서 CLI가 직접 쓰는 키. 선언 필드 이름으로 쓸 수 없다.
pub const RESERVED_CONTEXT_FIELDS: &[&str] = &["provider", "implicit", "execution"];

/// 호출자 문맥 하나. `[[contributes.caller_context]]`.
///
/// `id_env`가 비어 있지 않으면 이 provider의 호출자로 본다. 값이 `id_format`에 맞지 않거나
/// `path_env` 값을 절대 경로로 만들 수 없으면 엄격 적용 대상 명령을 거절한다.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CallerContextDecl {
    /// `_tasty_context.provider` 값.
    pub provider: String,
    /// 호출자 ID를 담는 환경변수.
    pub id_env: String,
    #[serde(default)]
    pub id_format: CallerIdFormat,
    /// ID를 실을 `_tasty_context` 키.
    pub id_field: String,
    /// 절대 경로로 바꿔 실을 환경변수. 비어 있으면 싣지 않는다.
    #[serde(default)]
    pub path_env: Vec<CallerPathEnvDecl>,
    /// PATH에서 찾아 `execution`(실행 파일·PATH)으로 실을 프로그램 이름.
    #[serde(default)]
    pub executable: Option<String>,
    /// 호출자가 있으면 `TASTY_SURFACE_ID`로 surface 인자를 채우지 않는다.
    /// 그 값이 호출자 프로그램이 시작될 때 물려받은 다른 터미널의 것일 수 있을 때 쓴다.
    #[serde(default = "default_true")]
    pub suppress_env_surface: bool,
}

fn default_true() -> bool {
    true
}

/// 호출자 ID 형식.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CallerIdFormat {
    /// 비어 있지 않은 UTF-8 문자열.
    #[default]
    Text,
    /// 하이픈 위치가 고정된 36자 16진수.
    Uuid,
}

impl CallerIdFormat {
    pub fn accepts(self, value: &str) -> bool {
        match self {
            Self::Text => !value.is_empty(),
            Self::Uuid => {
                value.len() == 36
                    && value.bytes().enumerate().all(|(i, b)| {
                        if [8, 13, 18, 23].contains(&i) {
                            b == b'-'
                        } else {
                            b.is_ascii_hexdigit()
                        }
                    })
            }
        }
    }
}

/// 경로 환경변수 하나와 실을 키.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CallerPathEnvDecl {
    pub env: String,
    pub field: String,
}

fn is_valid_provider(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn is_valid_field(s: &str) -> bool {
    is_valid_provider(s) && !s.contains('-') && !RESERVED_CONTEXT_FIELDS.contains(&s)
}

fn check_env(what: &str, raw: &str) -> anyhow::Result<()> {
    if !is_valid_env_name(raw) {
        anyhow::bail!(
            "invalid contributes.caller_context {what} '{raw}': must be ascii letters, digits \
             and '_', not starting with a digit"
        );
    }
    if raw.to_ascii_uppercase().starts_with("TASTY_") {
        anyhow::bail!(
            "contributes.caller_context {what} '{raw}' reads TASTY_* — the host owns its own \
             environment"
        );
    }
    Ok(())
}

/// provider·환경변수·필드 이름 형식과 중복을 확인한다.
pub(crate) fn validate(contributes: &crate::types::Contributes) -> anyhow::Result<()> {
    let (decls, requires) = (
        &contributes.caller_context,
        &contributes.requires_caller_context,
    );
    let mut providers = std::collections::HashSet::new();
    for decl in decls {
        if !is_valid_provider(&decl.provider) {
            anyhow::bail!(
                "invalid contributes.caller_context provider '{}': lowercase letters, digits, \
                 '_' and '-', starting with a letter",
                decl.provider
            );
        }
        if !providers.insert(decl.provider.as_str()) {
            anyhow::bail!(
                "duplicate contributes.caller_context provider '{}'",
                decl.provider
            );
        }
        check_env("id_env", &decl.id_env)?;
        let mut fields = std::collections::HashSet::new();
        for (env, field) in std::iter::once((&decl.id_env, &decl.id_field))
            .chain(decl.path_env.iter().map(|p| (&p.env, &p.field)))
        {
            check_env("env", env)?;
            if !is_valid_field(field) {
                anyhow::bail!(
                    "invalid contributes.caller_context field '{field}': lowercase letters, \
                     digits and '_', and not one of {RESERVED_CONTEXT_FIELDS:?}"
                );
            }
            if !fields.insert(field.as_str()) {
                anyhow::bail!("duplicate contributes.caller_context field '{field}'");
            }
        }
        if let Some(program) = &decl.executable
            && (program.is_empty()
                || program.contains(['/', '\\'])
                || program.chars().any(char::is_whitespace))
        {
            anyhow::bail!(
                "invalid contributes.caller_context executable '{program}': a program name \
                 without a path"
            );
        }
    }
    for provider in requires {
        if !is_valid_provider(provider) {
            anyhow::bail!("invalid contributes.requires_caller_context '{provider}'");
        }
    }
    Ok(())
}
