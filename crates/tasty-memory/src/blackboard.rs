//! workspace에서 공유하는 키-값 컬렉션.
//! tasty.bb.<name>._meta에는 메타데이터, tasty.bb.<name>.fields.<field>에는 필드를 저장한다.
//! owner와 수정 권한은 일반 memory와 같으며 _host는 모든 항목을 수정할 수 있다.
//! schema는 JSON으로 보관할 뿐 여기서 검증하지 않는다. 호출자가 저장소 접근을 직렬화한다.

use serde::{Deserialize, Serialize};

use crate::{MemoryError, Result};

/// blackboard 키 접두사. listing 시 prefix 로 사용.
pub const BB_KEY_PREFIX: &str = "tasty.bb.";

/// 전체 키 길이 제한 안에 들어가도록 blackboard 이름은 64자로 제한한다.
pub const BB_NAME_MAX: usize = 64;

/// field 이름 최대 길이.
pub const BB_FIELD_MAX: usize = 64;

/// `_meta` 페이로드 형태. user-provided `schema` 는 raw JSON 으로 보관된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlackboardMeta {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub schema: Option<serde_json::Value>,
    pub created_at: i64,
    pub created_by: String,
}

/// bb 이름 검증. 1..=64, `[a-z0-9_-]+`. 도트는 우리가 컨벤션 구분자로 쓰므로 금지.
pub fn validate_bb_name(name: &str) -> Result<()> {
    validate_name_inner(name, BB_NAME_MAX, "bb_name")
}

/// field 이름 검증. bb 이름과 동일 규칙.
pub fn validate_field_name(name: &str) -> Result<()> {
    validate_name_inner(name, BB_FIELD_MAX, "field_name")
}

pub(super) fn validate_name_inner(name: &str, max: usize, label: &str) -> Result<()> {
    if name.is_empty() {
        return Err(MemoryError::InvalidKey(format!("{label}: empty")));
    }
    if name.len() > max {
        return Err(MemoryError::InvalidKey(format!(
            "{label}: too long ({} > {max})",
            name.len()
        )));
    }
    for (i, c) in name.bytes().enumerate() {
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-';
        if !ok {
            return Err(MemoryError::InvalidKey(format!(
                "{label}: invalid char {:?} at {i}",
                c as char
            )));
        }
    }
    Ok(())
}

/// `_meta` key 생성.
pub(super) fn meta_key(bb: &str) -> String {
    format!("{BB_KEY_PREFIX}{bb}._meta")
}

/// `fields.<field>` key 생성.
pub(super) fn field_key(bb: &str, field: &str) -> String {
    format!("{BB_KEY_PREFIX}{bb}.fields.{field}")
}

/// bb 생성. 이미 존재하면 `AlreadyExists` 반환.
///
/// Returns: 새 `_meta` entry 의 version (= 1).
mod crud;
mod snapshot;

pub use crud::*;
pub use snapshot::*;
