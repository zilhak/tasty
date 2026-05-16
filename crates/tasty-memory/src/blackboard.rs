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

/// bb 전체 (모든 필드 + 모든 snapshot + `_meta`) 삭제.
///
/// 필드 / snapshot / meta 순서로 삭제. 중간에 owner 불일치로 실패하면 거기까지
/// 삭제된 상태로 에러를 전파한다 (transaction 없음 — `_host` caller 단순화 우선).
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

/// 한 필드의 직렬화된 페이로드. `MemoryValue` 의 internally-tagged enum 은
/// `serde_json::to_value` 경로에서 직렬화가 깨지므로 (newtype variant + scalar
/// 제약), snapshot 에서는 명시적인 `content_type` + JSON-호환 payload 로 평탄화한다.
///
/// `content_type` 별 payload:
///   - `"text/plain"`        → `payload: String`
///   - `"application/json"`  → `payload: serde_json::Value`
///   - `"application/octet-stream"` → `payload: String` (base64-encoded)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotField {
    pub field: String,
    pub content_type: String,
    pub payload: serde_json::Value,
}

impl SnapshotField {
    fn from_memory(field: String, v: MemoryValue) -> Self {
        match v {
            MemoryValue::Text(s) => Self {
                field,
                content_type: "text/plain".into(),
                payload: serde_json::Value::String(s),
            },
            MemoryValue::Json(j) => Self {
                field,
                content_type: "application/json".into(),
                payload: j,
            },
            MemoryValue::Binary(b) => Self {
                field,
                content_type: "application/octet-stream".into(),
                payload: serde_json::Value::String(encode_b64(&b)),
            },
        }
    }

    fn to_memory(&self) -> Result<MemoryValue> {
        match self.content_type.as_str() {
            "text/plain" => match &self.payload {
                serde_json::Value::String(s) => Ok(MemoryValue::Text(s.clone())),
                _ => Err(MemoryError::InvalidContentType(
                    "snapshot text/plain payload must be a string".into(),
                )),
            },
            "application/json" => Ok(MemoryValue::Json(self.payload.clone())),
            "application/octet-stream" => match &self.payload {
                serde_json::Value::String(s) => decode_b64(s)
                    .map(MemoryValue::Binary)
                    .map_err(MemoryError::InvalidContentType),
                _ => Err(MemoryError::InvalidContentType(
                    "snapshot binary payload must be a base64 string".into(),
                )),
            },
            other => Err(MemoryError::InvalidContentType(other.into())),
        }
    }
}

fn encode_b64(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(ALPHA[((b >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 6) & 0x3F) as usize] as char);
        out.push(ALPHA[(b & 0x3F) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let b = (bytes[i] as u32) << 16;
        out.push(ALPHA[((b >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 12) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let b = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(ALPHA[((b >> 18) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 12) & 0x3F) as usize] as char);
        out.push(ALPHA[((b >> 6) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

fn decode_b64(s: &str) -> std::result::Result<Vec<u8>, String> {
    let bytes = s.trim().as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return Err("invalid base64: length must be multiple of 4".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut buf = [0u8; 4];
    for chunk in bytes.chunks(4) {
        for (i, &b) in chunk.iter().enumerate() {
            buf[i] = match b {
                b'A'..=b'Z' => b - b'A',
                b'a'..=b'z' => b - b'a' + 26,
                b'0'..=b'9' => b - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => 64,
                _ => return Err(format!("invalid base64 char: {:?}", b as char)),
            };
        }
        let pad = chunk.iter().filter(|&&b| b == b'=').count();
        out.push((buf[0] << 2) | (buf[1] >> 4));
        if pad < 2 {
            out.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if pad < 1 {
            out.push((buf[2] << 6) | buf[3]);
        }
    }
    Ok(out)
}

/// 한 snapshot 전체.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlackboardSnapshot {
    pub bb_name: String,
    pub snapshot_id: String,
    pub taken_at: i64,
    pub taken_by: String,
    /// 캡처 시점의 `_meta` payload (있을 경우).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub meta: Option<serde_json::Value>,
    pub fields: Vec<SnapshotField>,
}

/// snapshot_id 검증. bb_name 과 동일 규칙.
pub fn validate_snapshot_id(sid: &str) -> Result<()> {
    validate_name_inner(sid, BB_NAME_MAX, "snapshot_id")
}

fn snapshot_key(bb: &str, sid: &str) -> String {
    format!("{BB_KEY_PREFIX}{bb}.snapshots.{sid}")
}

fn snapshot_prefix(bb: &str) -> String {
    format!("{BB_KEY_PREFIX}{bb}.snapshots.")
}

/// 현재 bb 상태로 새 snapshot 생성. bb 가 없으면 `NotFound`. 이미 같은 sid 가
/// 있으면 `AlreadyExists`.
pub fn bb_snapshot(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    bb: &str,
    snapshot_id: &str,
) -> Result<u64> {
    validate_bb_name(bb)?;
    validate_snapshot_id(snapshot_id)?;
    let scope = Scope::Workspace(workspace_id);

    let meta_entry = bb_get_meta(store, workspace_id, bb)?.ok_or_else(|| {
        MemoryError::NotFound {
            scope: scope.as_token(),
            key: meta_key(bb),
        }
    })?;
    let meta = match meta_entry.value {
        MemoryValue::Json(v) => Some(v),
        _ => None,
    };

    let key = snapshot_key(bb, snapshot_id);
    if store.get(&scope, &key)?.is_some() {
        return Err(MemoryError::AlreadyExists {
            scope: scope.as_token(),
            key,
        });
    }

    let fields_prefix = format!("{BB_KEY_PREFIX}{bb}.fields.");
    let entries = bb_get_all(store, workspace_id, bb)?;
    let mut fields = Vec::with_capacity(entries.len());
    for e in entries {
        let field = e
            .key
            .strip_prefix(&fields_prefix)
            .unwrap_or(&e.key)
            .to_string();
        fields.push(SnapshotField::from_memory(field, e.value));
    }

    let snap = BlackboardSnapshot {
        bb_name: bb.to_string(),
        snapshot_id: snapshot_id.to_string(),
        taken_at: now_ms_local(),
        taken_by: owner.to_string(),
        meta,
        fields,
    };
    let value = MemoryValue::Json(serde_json::to_value(&snap).map_err(|e| {
        MemoryError::InvalidContentType(format!("serialize snapshot: {e}"))
    })?);
    store.put(owner, &scope, &key, &value, &PutOpts::default())
}

/// 단일 snapshot 조회.
pub fn bb_snapshot_get(
    store: &MemoryStore,
    workspace_id: u32,
    bb: &str,
    snapshot_id: &str,
) -> Result<Option<BlackboardSnapshot>> {
    validate_bb_name(bb)?;
    validate_snapshot_id(snapshot_id)?;
    let Some(entry) =
        store.get(&Scope::Workspace(workspace_id), &snapshot_key(bb, snapshot_id))?
    else {
        return Ok(None);
    };
    let MemoryValue::Json(v) = entry.value else {
        return Err(MemoryError::InvalidContentType(format!(
            "snapshot entry is not application/json: {}",
            entry.key
        )));
    };
    serde_json::from_value::<BlackboardSnapshot>(v)
        .map(Some)
        .map_err(|e| MemoryError::InvalidContentType(format!("invalid snapshot json: {e}")))
}

/// bb 의 snapshot id 목록 (정렬).
pub fn bb_snapshot_list(
    store: &MemoryStore,
    workspace_id: u32,
    bb: &str,
) -> Result<Vec<String>> {
    validate_bb_name(bb)?;
    let prefix = snapshot_prefix(bb);
    let opts = ListOpts {
        prefix: Some(prefix.clone()),
        ..Default::default()
    };
    let entries = store.list(&Scope::Workspace(workspace_id), &opts)?;
    Ok(entries
        .into_iter()
        .filter_map(|e| e.key.strip_prefix(&prefix).map(|s| s.to_string()))
        .collect())
}

/// snapshot 삭제. 없으면 `NotFound`.
pub fn bb_snapshot_delete(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    bb: &str,
    snapshot_id: &str,
) -> Result<()> {
    validate_bb_name(bb)?;
    validate_snapshot_id(snapshot_id)?;
    store.delete(
        owner,
        &Scope::Workspace(workspace_id),
        &snapshot_key(bb, snapshot_id),
        None,
    )
}

/// snapshot 으로 bb 상태 복원.
///
/// 동작:
///   1. 현재 bb 의 모든 field 를 삭제 (caller 가 owner 이거나 `_host` 일 때만 성공)
///   2. bb 가 없으면 snapshot 의 meta 로 새로 만들고, 있으면 기존 meta 유지
///   3. snapshot 의 각 field 를 caller owner 로 다시 put
///
/// snapshot 자체 entry 는 그대로 남는다 (반복 restore 가능).
///
/// Returns: 복원 후 bb 의 field 개수.
pub fn bb_snapshot_restore(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    bb: &str,
    snapshot_id: &str,
) -> Result<usize> {
    validate_bb_name(bb)?;
    validate_snapshot_id(snapshot_id)?;
    let scope = Scope::Workspace(workspace_id);

    let snap = bb_snapshot_get(store, workspace_id, bb, snapshot_id)?.ok_or_else(|| {
        MemoryError::NotFound {
            scope: scope.as_token(),
            key: snapshot_key(bb, snapshot_id),
        }
    })?;

    // 기존 field 모두 제거.
    for entry in bb_get_all(store, workspace_id, bb)? {
        store.delete(owner, &scope, &entry.key, None)?;
    }

    // meta 가 없으면 snapshot.meta 로 새로 만들고, 있으면 그대로 둔다.
    if !bb_exists(store, workspace_id, bb)? {
        let meta_value = MemoryValue::Json(snap.meta.clone().unwrap_or(serde_json::json!({
            "name": bb,
            "restored_from": snapshot_id,
            "created_at": now_ms_local(),
            "created_by": owner,
        })));
        store.put(
            owner,
            &scope,
            &meta_key(bb),
            &meta_value,
            &PutOpts::default(),
        )?;
    }

    // snapshot field 재기록.
    let mut restored = 0;
    for sf in snap.fields {
        validate_field_name(&sf.field)?;
        let value = sf.to_memory()?;
        store.put(
            owner,
            &scope,
            &field_key(bb, &sf.field),
            &value,
            &PutOpts::default(),
        )?;
        restored += 1;
    }
    Ok(restored)
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
    fn snapshot_then_get_roundtrip() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("1".into()), None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "b", &MemoryValue::Text("2".into()), None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        let snap = bb_snapshot_get(&s, 1, "bb", "v1").unwrap().expect("snap");
        assert_eq!(snap.bb_name, "bb");
        assert_eq!(snap.snapshot_id, "v1");
        assert_eq!(snap.taken_by, HOST_OWNER);
        assert_eq!(snap.fields.len(), 2);
        assert_eq!(snap.fields[0].field, "a");
        assert_eq!(snap.fields[1].field, "b");
    }

    #[test]
    fn snapshot_without_bb_fails() {
        let mut s = open();
        let err = bb_snapshot(&mut s, HOST_OWNER, 1, "ghost", "v1").unwrap_err();
        assert!(matches!(err, MemoryError::NotFound { .. }), "{err:?}");
    }

    #[test]
    fn duplicate_snapshot_id_rejected() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        let err = bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap_err();
        assert!(matches!(err, MemoryError::AlreadyExists { .. }), "{err:?}");
    }

    #[test]
    fn snapshot_list_returns_ids() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v2").unwrap();
        let ids = bb_snapshot_list(&s, 1, "bb").unwrap();
        assert_eq!(ids, vec!["v1".to_string(), "v2".to_string()]);
    }

    #[test]
    fn snapshot_delete_removes_entry() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        bb_snapshot_delete(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        assert!(bb_snapshot_get(&s, 1, "bb", "v1").unwrap().is_none());
    }

    #[test]
    fn restore_replaces_current_fields() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("1".into()), None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "b", &MemoryValue::Text("2".into()), None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();

        // 이후에 변경된 상태.
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("modified".into()), None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "c", &MemoryValue::Text("new".into()), None).unwrap();
        bb_delete_field(&mut s, HOST_OWNER, 1, "bb", "b", None).unwrap();

        let restored = bb_snapshot_restore(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        assert_eq!(restored, 2);

        let a = bb_get(&s, 1, "bb", "a").unwrap().unwrap();
        assert_eq!(a.value, MemoryValue::Text("1".into()));
        let b = bb_get(&s, 1, "bb", "b").unwrap().unwrap();
        assert_eq!(b.value, MemoryValue::Text("2".into()));
        assert!(bb_get(&s, 1, "bb", "c").unwrap().is_none());
    }

    #[test]
    fn restore_recreates_missing_bb() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("1".into()), None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        // bb 본체 (meta + fields) 삭제. snapshot 은 별도라 보존됨.
        bb_delete(&mut s, HOST_OWNER, 1, "bb").unwrap();
        // 하지만 위 bb_delete 는 snapshot 도 같이 지운다 → snapshot 도 사라짐.
        // restore 가능 여부 확인: snapshot 없으니 NotFound 일 것.
        let err = bb_snapshot_restore(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap_err();
        assert!(matches!(err, MemoryError::NotFound { .. }), "{err:?}");
    }

    #[test]
    fn restore_recreates_when_meta_alone_removed() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(&mut s, HOST_OWNER, 1, "bb", "a", &MemoryValue::Text("1".into()), None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        // meta 만 직접 삭제하고 snapshot 은 그대로 둠.
        s.delete(HOST_OWNER, &Scope::Workspace(1), &meta_key("bb"), None).unwrap();
        assert!(!bb_exists(&s, 1, "bb").unwrap());

        bb_snapshot_restore(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        assert!(bb_exists(&s, 1, "bb").unwrap());
        let a = bb_get(&s, 1, "bb", "a").unwrap().unwrap();
        assert_eq!(a.value, MemoryValue::Text("1".into()));
    }

    #[test]
    fn bb_delete_also_removes_snapshots() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v2").unwrap();
        let removed = bb_delete(&mut s, HOST_OWNER, 1, "bb").unwrap();
        // meta + 2 snapshots = 3.
        assert_eq!(removed, 3);
        assert!(bb_snapshot_list(&s, 1, "bb").unwrap().is_empty());
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
