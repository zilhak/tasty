//! 자식 터미널의 부모 키. `Surface`는 이 Tasty의 부모 surface이고, `External`은 surface가 없는
//! 부모를 plugin이 만든 provider 중립 문자열로 가리킨다. host는 External 키를 해석하지 않는다.
//!
//! serde는 숫자를 `Surface`, 문자열을 `External`로 읽는다. 숫자 부모만 기록한 이전 레코드도
//! 그대로 읽힌다.

use serde::{Deserialize, Serialize};

/// External 키의 최대 byte 길이. 저장 파일과 응답에 그대로 실리므로 상한을 둔다.
pub(crate) const EXTERNAL_PARENT_MAX_BYTES: usize = 256;

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParentKey {
    Surface(u32),
    External(String),
}

impl ParentKey {
    pub fn surface(&self) -> Option<u32> {
        match self {
            Self::Surface(id) => Some(*id),
            Self::External(_) => None,
        }
    }

    pub fn external(&self) -> Option<&str> {
        match self {
            Self::Surface(_) => None,
            Self::External(key) => Some(key),
        }
    }
}

impl From<u32> for ParentKey {
    fn from(surface: u32) -> Self {
        Self::Surface(surface)
    }
}

/// Surface는 숫자만 쓴다. 점유 오류 문구(`{error:?}`)가 기존 u32 부모와 같은 형태로 남는다.
impl std::fmt::Debug for ParentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Surface(id) => write!(f, "{id}"),
            Self::External(key) => write!(f, "{key:?}"),
        }
    }
}

impl std::fmt::Display for ParentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Surface(id) => write!(f, "{id}"),
            Self::External(key) => f.write_str(key),
        }
    }
}

/// 비어 있거나 길거나 공백·제어문자가 있으면 거절 사유를 반환한다.
pub(crate) fn validate_external_parent(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("'parent_external' must not be empty".into());
    }
    if key.len() > EXTERNAL_PARENT_MAX_BYTES {
        return Err(format!(
            "'parent_external' is longer than {EXTERNAL_PARENT_MAX_BYTES} bytes"
        ));
    }
    if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("'parent_external' must not contain whitespace or control characters".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_reads_as_a_surface_and_a_string_as_external() {
        let surface: ParentKey = serde_json::from_value(serde_json::json!(12)).expect("number");
        assert_eq!(surface, ParentKey::Surface(12));
        let external: ParentKey =
            serde_json::from_value(serde_json::json!("codex:t1")).expect("string");
        assert_eq!(external, ParentKey::External("codex:t1".into()));
        assert_eq!(
            serde_json::to_value(&surface).unwrap(),
            serde_json::json!(12)
        );
        assert_eq!(
            serde_json::to_value(&external).unwrap(),
            serde_json::json!("codex:t1")
        );
    }

    #[test]
    fn a_surface_prints_as_the_bare_number() {
        assert_eq!(format!("{:?}", ParentKey::Surface(7)), "7");
        assert_eq!(ParentKey::Surface(7).to_string(), "7");
        assert_eq!(
            format!("{:?}", ParentKey::External("codex:t".into())),
            "\"codex:t\""
        );
    }

    #[test]
    fn an_external_key_is_bounded_and_has_no_spaces() {
        assert!(validate_external_parent("codex:0199-ab").is_ok());
        assert!(validate_external_parent("").is_err());
        assert!(validate_external_parent("a b").is_err());
        assert!(validate_external_parent("a\nb").is_err());
        assert!(validate_external_parent(&"x".repeat(EXTERNAL_PARENT_MAX_BYTES)).is_ok());
        assert!(validate_external_parent(&"x".repeat(EXTERNAL_PARENT_MAX_BYTES + 1)).is_err());
    }
}
