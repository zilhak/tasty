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

use crate::{
    ListOpts, MemoryEntry, MemoryError, MemoryStore, MemoryValue, PutOpts, Result, Scope,
};

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

fn validate_name_inner(name: &str, max: usize, label: &str) -> Result<()> {
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
pub fn meta_key(bb: &str) -> String {
    format!("{BB_KEY_PREFIX}{bb}._meta")
}

/// `fields.<field>` key 생성.
pub fn field_key(bb: &str, field: &str) -> String {
    format!("{BB_KEY_PREFIX}{bb}.fields.{field}")
}

/// bb 생성. 이미 존재하면 `AlreadyExists` 반환.
///
/// Returns: 새 `_meta` entry 의 version (= 1).
pub fn bb_create(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    bb: &str,
    schema: Option<serde_json::Value>,
) -> Result<u64> {
    validate_bb_name(bb)?;
    let scope = Scope::Workspace(workspace_id);
    let key = meta_key(bb);
    if store.get(&scope, &key)?.is_some() {
        return Err(MemoryError::AlreadyExists {
            scope: scope.as_token(),
            key,
        });
    }
    let meta = BlackboardMeta {
        name: bb.to_string(),
        schema,
        created_at: now_ms_local(),
        created_by: owner.to_string(),
    };
    let value = MemoryValue::Json(serde_json::to_value(&meta).map_err(|e| {
        MemoryError::InvalidContentType(format!("serialize bb meta: {e}"))
    })?);
    store.put(owner, &scope, &key, &value, &PutOpts::default())
}

/// `_meta` entry 조회. bb 가 없으면 `Ok(None)`.
pub fn bb_get_meta(
    store: &MemoryStore,
    workspace_id: u32,
    bb: &str,
) -> Result<Option<MemoryEntry>> {
    validate_bb_name(bb)?;
    store.get(&Scope::Workspace(workspace_id), &meta_key(bb))
}

/// bb 존재 여부 (= `_meta` 존재 여부).
pub fn bb_exists(store: &MemoryStore, workspace_id: u32, bb: &str) -> Result<bool> {
    Ok(bb_get_meta(store, workspace_id, bb)?.is_some())
}

/// 필드 쓰기. `_meta` 가 없으면 `NotFound` 반환.
pub fn bb_put(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    bb: &str,
    field: &str,
    value: &MemoryValue,
    cas: Option<u64>,
) -> Result<u64> {
    validate_bb_name(bb)?;
    validate_field_name(field)?;
    let scope = Scope::Workspace(workspace_id);
    if !bb_exists(store, workspace_id, bb)? {
        return Err(MemoryError::NotFound {
            scope: scope.as_token(),
            key: meta_key(bb),
        });
    }
    let key = field_key(bb, field);
    let opts = PutOpts { expires_at: None, cas };
    store.put(owner, &scope, &key, value, &opts)
}

/// 단일 필드 조회. 만료/미존재면 `Ok(None)`.
pub fn bb_get(
    store: &MemoryStore,
    workspace_id: u32,
    bb: &str,
    field: &str,
) -> Result<Option<MemoryEntry>> {
    validate_bb_name(bb)?;
    validate_field_name(field)?;
    store.get(&Scope::Workspace(workspace_id), &field_key(bb, field))
}

/// bb 의 모든 필드 (meta 제외) 를 `key ASC` 순서로 반환.
pub fn bb_get_all(
    store: &MemoryStore,
    workspace_id: u32,
    bb: &str,
) -> Result<Vec<MemoryEntry>> {
    validate_bb_name(bb)?;
    let prefix = format!("{BB_KEY_PREFIX}{bb}.fields.");
    let opts = ListOpts {
        prefix: Some(prefix),
        ..Default::default()
    };
    store.list(&Scope::Workspace(workspace_id), &opts)
}

/// 단일 필드 삭제. CAS 지원.
pub fn bb_delete_field(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    bb: &str,
    field: &str,
    cas: Option<u64>,
) -> Result<()> {
    validate_bb_name(bb)?;
    validate_field_name(field)?;
    store.delete(
        owner,
        &Scope::Workspace(workspace_id),
        &field_key(bb, field),
        cas,
    )
}

/// bb 전체 (모든 필드 + `_meta`) 삭제.
///
/// 필드부터 순차 삭제한 뒤 마지막에 meta 를 삭제한다. 중간에 owner 불일치로
/// 실패하면 거기까지 삭제된 상태로 에러를 전파한다 (transaction 없음 — caller
/// 가 root 권한 `_host` 인 경우 단순화 우선).
///
/// Returns: 삭제된 entry 총 개수.
pub fn bb_delete(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    bb: &str,
) -> Result<usize> {
    validate_bb_name(bb)?;
    let scope = Scope::Workspace(workspace_id);
    let fields = bb_get_all(store, workspace_id, bb)?;
    let mut removed = 0;
    for entry in fields {
        store.delete(owner, &scope, &entry.key, None)?;
        removed += 1;
    }
    if store.get(&scope, &meta_key(bb))?.is_some() {
        store.delete(owner, &scope, &meta_key(bb), None)?;
        removed += 1;
    }
    Ok(removed)
}

/// 워크스페이스에 존재하는 bb 이름 목록 (정렬). `_meta` 존재로 판단.
pub fn bb_list(store: &MemoryStore, workspace_id: u32) -> Result<Vec<String>> {
    let opts = ListOpts {
        prefix: Some(BB_KEY_PREFIX.to_string()),
        ..Default::default()
    };
    let entries = store.list(&Scope::Workspace(workspace_id), &opts)?;
    let mut names = Vec::new();
    for e in entries {
        let rest = e.key.strip_prefix(BB_KEY_PREFIX).unwrap_or("");
        if let Some(name) = rest.strip_suffix("._meta") {
            // name 안에 `.fields.` 같은 게 들어가는 일은 없다 — bb_name 은 도트 금지.
            // 다만 손상된 데이터 방어 차원에서 한번 더 검증.
            if !name.contains('.') {
                names.push(name.to_string());
            }
        }
    }
    Ok(names)
}

fn now_ms_local() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HOST_OWNER;

    fn open() -> MemoryStore {
        MemoryStore::open_in_memory().expect("open in memory")
    }

    #[test]
    fn create_then_get_meta_roundtrip() {
        let mut s = open();
        let schema = serde_json::json!({ "status": "string" });
        bb_create(&mut s, HOST_OWNER, 1, "tasks", Some(schema.clone())).unwrap();
        let meta = bb_get_meta(&s, 1, "tasks").unwrap().expect("meta");
        let MemoryValue::Json(v) = &meta.value else { panic!("expected json") };
        assert_eq!(v["name"], "tasks");
        assert_eq!(v["schema"], schema);
        assert_eq!(v["created_by"], HOST_OWNER);
        assert!(bb_exists(&s, 1, "tasks").unwrap());
    }

    #[test]
    fn create_duplicate_fails() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb1", None).unwrap();
        let err = bb_create(&mut s, HOST_OWNER, 1, "bb1", None).unwrap_err();
        assert!(matches!(err, MemoryError::AlreadyExists { .. }), "{err:?}");
    }

    #[test]
    fn put_requires_meta() {
        let mut s = open();
        let v = MemoryValue::Text("x".into());
        let err = bb_put(&mut s, HOST_OWNER, 1, "ghost", "f", &v, None).unwrap_err();
        assert!(matches!(err, MemoryError::NotFound { .. }), "{err:?}");
    }

    #[test]
    fn put_then_get_field() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        let v1 = bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "status",
            &MemoryValue::Text("ready".into()),
            None,
        )
        .unwrap();
        assert_eq!(v1, 1);
        let got = bb_get(&s, 1, "bb", "status").unwrap().expect("entry");
        assert_eq!(got.value, MemoryValue::Text("ready".into()));
        assert_eq!(got.version, 1);
    }

    #[test]
    fn cas_conflict_on_put() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "f", &MemoryValue::Text("a".into()), None).unwrap();
        let err = bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "f",
            &MemoryValue::Text("b".into()),
            Some(99),
        )
        .unwrap_err();
        assert!(matches!(err, MemoryError::CasConflict { .. }), "{err:?}");
    }

    #[test]
    fn get_all_returns_fields_excluding_meta() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("1".into()), None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "b", &MemoryValue::Text("2".into()), None).unwrap();
        let all = bb_get_all(&s, 1, "bb").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].key, field_key("bb", "a"));
        assert_eq!(all[1].key, field_key("bb", "b"));
    }

    #[test]
    fn delete_field_leaves_meta() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("1".into()), None).unwrap();
        bb_delete_field(&mut s, HOST_OWNER, 1, "bb", "a", None).unwrap();
        assert!(bb_get(&s, 1, "bb", "a").unwrap().is_none());
        assert!(bb_exists(&s, 1, "bb").unwrap());
    }

    #[test]
    fn delete_removes_everything() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("1".into()), None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "b", &MemoryValue::Text("2".into()), None).unwrap();
        let removed = bb_delete(&mut s, HOST_OWNER, 1, "bb").unwrap();
        assert_eq!(removed, 3);
        assert!(!bb_exists(&s, 1, "bb").unwrap());
        assert!(bb_get_all(&s, 1, "bb").unwrap().is_empty());
    }

    #[test]
    fn list_returns_only_bb_names() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "alpha", None).unwrap();
        bb_create(&mut s, HOST_OWNER, 1, "beta", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "alpha", "f", &MemoryValue::Text("x".into()), None).unwrap();
        let names = bb_list(&s, 1).unwrap();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn list_isolated_by_workspace() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "a", None).unwrap();
        bb_create(&mut s, HOST_OWNER, 2, "b", None).unwrap();
        assert_eq!(bb_list(&s, 1).unwrap(), vec!["a".to_string()]);
        assert_eq!(bb_list(&s, 2).unwrap(), vec!["b".to_string()]);
    }

    #[test]
    fn validate_bb_name_rejects_invalid() {
        validate_bb_name("").unwrap_err();
        validate_bb_name("UPPER").unwrap_err();
        validate_bb_name("with.dot").unwrap_err();
        validate_bb_name("with space").unwrap_err();
        validate_bb_name(&"a".repeat(BB_NAME_MAX + 1)).unwrap_err();
        validate_bb_name("ok_name-1").unwrap();
    }

    #[test]
    fn owner_enforced_on_put() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, "plugin.a", 1, "bb", "f", &MemoryValue::Text("a".into()), None).unwrap();
        let err = bb_put(
            &mut s,
            "plugin.b",
            1,
            "bb",
            "f",
            &MemoryValue::Text("b".into()),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, MemoryError::OwnedByOther { .. }), "{err:?}");
    }
}
