//! Blackboard CRUD — create / get_meta / exists / put / get / get_all / delete_field / delete.

use crate::{
    ListOpts, MemoryEntry, MemoryError, MemoryStorage, MemoryValue, PutOpts, Result, Scope,
};

use super::{
    BB_KEY_PREFIX, BlackboardMeta, bb_snapshot_list, field_key, meta_key, now_ms_local,
    snapshot_key, validate_bb_name, validate_field_name,
};

pub fn bb_create(
    store: &mut dyn MemoryStorage,
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
    let value = MemoryValue::Json(
        serde_json::to_value(&meta)
            .map_err(|e| MemoryError::InvalidContentType(format!("serialize bb meta: {e}")))?,
    );
    store.put(owner, &scope, &key, &value, &PutOpts::default())
}

/// `_meta` entry 조회. bb 가 없으면 `Ok(None)`.
pub fn bb_get_meta(
    store: &dyn MemoryStorage,
    workspace_id: u32,
    bb: &str,
) -> Result<Option<MemoryEntry>> {
    validate_bb_name(bb)?;
    store.get(&Scope::Workspace(workspace_id), &meta_key(bb))
}

/// bb 존재 여부 (= `_meta` 존재 여부).
pub fn bb_exists(store: &dyn MemoryStorage, workspace_id: u32, bb: &str) -> Result<bool> {
    Ok(bb_get_meta(store, workspace_id, bb)?.is_some())
}

/// 필드 쓰기. `_meta` 가 없으면 `NotFound` 반환.
pub fn bb_put(
    store: &mut dyn MemoryStorage,
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
    let opts = PutOpts {
        expires_at: None,
        cas,
    };
    store.put(owner, &scope, &key, value, &opts)
}

/// 단일 필드 조회. 만료/미존재면 `Ok(None)`.
pub fn bb_get(
    store: &dyn MemoryStorage,
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
    store: &dyn MemoryStorage,
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
    store: &mut dyn MemoryStorage,
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

/// bb 전체 (모든 필드 + 모든 snapshot + `_meta`) 삭제.
///
/// 필드 / snapshot / meta 순서로 삭제. 중간에 owner 불일치로 실패하면 거기까지
/// 삭제된 상태로 에러를 전파한다 (transaction 없음 — `_host` caller 단순화 우선).
///
/// Returns: 삭제된 entry 총 개수.
pub fn bb_delete(
    store: &mut dyn MemoryStorage,
    owner: &str,
    workspace_id: u32,
    bb: &str,
) -> Result<usize> {
    validate_bb_name(bb)?;
    let scope = Scope::Workspace(workspace_id);
    let mut removed = 0;
    for entry in bb_get_all(store, workspace_id, bb)? {
        store.delete(owner, &scope, &entry.key, None)?;
        removed += 1;
    }
    for sid in bb_snapshot_list(store, workspace_id, bb)? {
        store.delete(owner, &scope, &snapshot_key(bb, &sid), None)?;
        removed += 1;
    }
    if store.get(&scope, &meta_key(bb))?.is_some() {
        store.delete(owner, &scope, &meta_key(bb), None)?;
        removed += 1;
    }
    Ok(removed)
}

// ============================================================
// Snapshot — Phase 7.4
// ============================================================
//
// 한 snapshot 은 bb 의 한 시점을 통째로 직렬화해 `tasty.bb.<name>.snapshots.<sid>`
// 키에 보관한다. snapshot 값 = JSON([`BlackboardSnapshot`]). restore 는 현재
// fields 를 모두 지우고 snapshot 의 fields 를 동일 caller owner 로 다시 기록.
