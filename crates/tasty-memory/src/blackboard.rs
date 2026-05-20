//! Blackboard — workspace 단위로 공유되는 키-값 컬렉션.
//!
//! 한 blackboard 는 `<name>` 으로 식별되며, regular memory 영역의 다음 키 컨벤션에
//! 매핑된다:
//!
//! - `tasty.bb.<name>._meta`          — schema/메타데이터 (JSON)
//! - `tasty.bb.<name>.fields.<field>` — 개별 필드 값
//!
//! 모든 entry 는 `Scope::Workspace(id)` 스코프에 저장되어 동일 workspace 안에서만
//! 공유된다. owner 규칙은 일반 memory put/delete 와 동일 — `_host` 는 root, plugin/
//! agent owner 는 자기 entry 만 수정 가능.
//!
//! schema 검증은 본 모듈에서 수행하지 않는다 — `_meta.schema` 는 raw JSON 으로만
//! 보관되며, 호출자가 필요 시 외부 jsonschema 도구로 직접 검증한다.
//!
//! 동기 모델: 본 모듈의 함수는 모두 sync — `MemoryStore` 단일 mutex 가 호스트
//! 메인 스레드에서 직렬화한다는 가정에 따라 별도 락 없이 호출 가능하다.

use serde::{Deserialize, Serialize};

use crate::{MemoryError, Result};

/// blackboard 키 접두사. listing 시 prefix 로 사용.
pub const BB_KEY_PREFIX: &str = "tasty.bb.";

/// bb 이름 최대 길이. 전체 key 가 `validate_key` 의 256 자 제한을 안전하게
/// 만족하도록 64 자로 캡.
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
