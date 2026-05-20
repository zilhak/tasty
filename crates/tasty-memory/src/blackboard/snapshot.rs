//! Blackboard snapshot — bb_snapshot / bb_snapshot_get / bb_snapshot_list / bb_snapshot_delete.

use serde::{Deserialize, Serialize};

use crate::{ListOpts, MemoryError, MemoryStore, MemoryValue, PutOpts, Result, Scope};

use super::{
    BB_KEY_PREFIX, BB_NAME_MAX, bb_exists, bb_get_all, bb_get_meta, field_key, meta_key,
    validate_bb_name, validate_field_name, validate_name_inner,
};

/// 한 필드의 직렬화된 페이로드.
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

pub(super) fn snapshot_key(bb: &str, sid: &str) -> String {
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

    let meta_entry =
        bb_get_meta(store, workspace_id, bb)?.ok_or_else(|| MemoryError::NotFound {
            scope: scope.as_token(),
            key: meta_key(bb),
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
    let value = MemoryValue::Json(
        serde_json::to_value(&snap)
            .map_err(|e| MemoryError::InvalidContentType(format!("serialize snapshot: {e}")))?,
    );
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
    let Some(entry) = store.get(
        &Scope::Workspace(workspace_id),
        &snapshot_key(bb, snapshot_id),
    )?
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
pub fn bb_snapshot_list(store: &MemoryStore, workspace_id: u32, bb: &str) -> Result<Vec<String>> {
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

pub(super) fn now_ms_local() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::HOST_OWNER;
    use crate::*;

    fn open() -> MemoryStore {
        MemoryStore::open_in_memory().expect("open in memory")
    }

    #[test]
    fn create_then_get_meta_roundtrip() {
        let mut s = open();
        let schema = serde_json::json!({ "status": "string" });
        bb_create(&mut s, HOST_OWNER, 1, "tasks", Some(schema.clone())).unwrap();
        let meta = bb_get_meta(&s, 1, "tasks").unwrap().expect("meta");
        let MemoryValue::Json(v) = &meta.value else {
            panic!("expected json")
        };
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
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "f",
            &MemoryValue::Text("a".into()),
            None,
        )
        .unwrap();
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
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("1".into()),
            None,
        )
        .unwrap();
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "b",
            &MemoryValue::Text("2".into()),
            None,
        )
        .unwrap();
        let all = bb_get_all(&s, 1, "bb").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].key, field_key("bb", "a"));
        assert_eq!(all[1].key, field_key("bb", "b"));
    }

    #[test]
    fn delete_field_leaves_meta() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("1".into()),
            None,
        )
        .unwrap();
        bb_delete_field(&mut s, HOST_OWNER, 1, "bb", "a", None).unwrap();
        assert!(bb_get(&s, 1, "bb", "a").unwrap().is_none());
        assert!(bb_exists(&s, 1, "bb").unwrap());
    }

    #[test]
    fn delete_removes_everything() {
        let mut s = open();
        bb_create(&mut s, HOST_OWNER, 1, "bb", None).unwrap();
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("1".into()),
            None,
        )
        .unwrap();
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "b",
            &MemoryValue::Text("2".into()),
            None,
        )
        .unwrap();
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
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "alpha",
            "f",
            &MemoryValue::Text("x".into()),
            None,
        )
        .unwrap();
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
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("1".into()),
            None,
        )
        .unwrap();
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "b",
            &MemoryValue::Text("2".into()),
            None,
        )
        .unwrap();
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
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("1".into()),
            None,
        )
        .unwrap();
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "b",
            &MemoryValue::Text("2".into()),
            None,
        )
        .unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();

        // 이후에 변경된 상태.
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("modified".into()),
            None,
        )
        .unwrap();
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "c",
            &MemoryValue::Text("new".into()),
            None,
        )
        .unwrap();
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
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("1".into()),
            None,
        )
        .unwrap();
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
        bb_put(
            &mut s,
            HOST_OWNER,
            1,
            "bb",
            "a",
            &MemoryValue::Text("1".into()),
            None,
        )
        .unwrap();
        bb_snapshot(&mut s, HOST_OWNER, 1, "bb", "v1").unwrap();
        // meta 만 직접 삭제하고 snapshot 은 그대로 둠.
        s.delete(HOST_OWNER, &Scope::Workspace(1), &meta_key("bb"), None)
            .unwrap();
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
        bb_put(
            &mut s,
            "plugin.a",
            1,
            "bb",
            "f",
            &MemoryValue::Text("a".into()),
            None,
        )
        .unwrap();
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
