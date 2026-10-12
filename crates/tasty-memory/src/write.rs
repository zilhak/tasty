//! 쓰기 연산의 SQL 본문. 각 본문은 열린 transaction 안에서 실행되고 공개할 bookkeeping을
//! [`Effect`]로 돌려준다. 단독 호출은 transaction 하나를 열어 commit하고, 실행 단위
//! ([`crate::batch`]) 안의 호출은 그 단위의 transaction을 함께 쓴다.

use rusqlite::{Connection, OptionalExtension, params};

use crate::batch::Effect;
use crate::{
    MemoryArea, MemoryChange, MemoryChangeKind, MemoryError, MemoryStore, MemoryValue, PurgeStats,
    PutOpts, Result, Scope, owner_can_modify, serialize_value, unix_ms_now, validate_key,
    validate_owner,
};

impl MemoryStore {
    /// 값을 저장(upsert). 신규면 version=1 / owner=caller, 갱신이면 +1.
    /// 갱신 시 기존 owner 가 caller 와 다르면 `OwnedByOther` (caller 가 `_host` 면 root 통과).
    pub fn put(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let bytes = serialize_value(value)?;
        self.check_entry_size(bytes.len())?;
        let scope_token = scope.as_token();
        let content_type = value.content_type();
        let quota = self.config.regular_quota_total_bytes;
        self.write_step(|conn, used_before| {
            put_regular_rows(
                conn,
                used_before,
                quota,
                owner,
                scope_token,
                key,
                &bytes,
                content_type,
                opts,
            )
        })
    }

    /// 삭제. CAS 미스면 conflict, 없으면 NotFound, owner 불일치면 OwnedByOther.
    pub fn delete(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        cas: Option<u64>,
    ) -> Result<()> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();
        self.write_step(|conn, used_before| {
            delete_regular_rows(conn, used_before, owner, scope_token, key, cas)
        })
    }

    /// Secret put. owner 차원으로 자동 분리되므로 다른 plugin 영역과 충돌 없음.
    /// Value 는 평문 BLOB 그대로 저장 — 보호 수준은 plugin 간 IPC 격리까지.
    pub fn put_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let bytes = serialize_value(value)?;
        self.check_entry_size(bytes.len())?;
        let scope_token = scope.as_token();
        let content_type = value.content_type();
        let quota = self.config.secret_quota_per_owner_bytes;
        self.write_step(|conn, _| {
            put_secret_rows(
                conn,
                quota,
                owner,
                &scope_token,
                key,
                &bytes,
                content_type,
                opts,
            )
        })
    }

    pub fn delete_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        cas: Option<u64>,
    ) -> Result<()> {
        validate_owner(owner)?;
        validate_key(key).map_err(MemoryError::InvalidKey)?;
        let scope_token = scope.as_token();
        self.write_step(|conn, _| delete_secret_rows(conn, owner, scope_token, key, cas))
    }

    // ============================================================
    // GC / Maintenance
    // ============================================================

    /// 만료된 entry (regular + secret) 일괄 DELETE. 반환: 삭제된 row 수.
    /// read 경로는 항상 `expires_at` 필터를 적용하므로 만료 entry 가 노출되진
    /// 않지만, 디스크에 row 가 남아 quota 와 파일 크기를 부풀린다. 호스트는
    /// 주기적으로 또는 `memory.gc` 명령으로 이 함수를 호출해 청소한다.
    /// 조회와 두 DELETE를 transaction 하나로 확정한다.
    pub fn purge_expired(&mut self) -> Result<PurgeStats> {
        self.maintenance_step(purge_expired_rows)
    }

    /// 특정 scope 의 모든 entry (regular + secret 양쪽) 삭제. surface/window/
    /// workspace 가 닫힐 때 호스트가 호출해 해당 수명에 묶인 키를 정리한다.
    /// 조회와 두 DELETE를 transaction 하나로 확정한다.
    pub fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats> {
        let token = scope.as_token();
        self.maintenance_step(|conn| purge_scope_rows(conn, token))
    }

    /// prefix로 시작하는 로그 키를 내림차순으로 정렬해 최근 keep_recent개만 남긴다.
    /// 키가 0으로 채운 시각을 포함해 문자열 순서가 시간 순서와 같은 로그용이다.
    /// 변경 이벤트 없이 삭제하며 삭제 수를 반환한다. scope는 구분하지 않는다.
    /// prefix의 LIKE 특수문자는 이스케이프한다.
    pub fn prune_prefix_keep_recent(&mut self, prefix: &str, keep_recent: u64) -> Result<u64> {
        let like = like_prefix(prefix);
        self.maintenance_step(|conn| {
            // OFFSET=keep_recent 위치(0-based)의 키 = "최신에서 N+1번째" = 첫 삭제 대상.
            // 그 키 이하(`<=`)를 모두 삭제 → 최신 N개만 남는다. 매칭 행 ≤ keep_recent 면
            // OFFSET 이 범위를 벗어나 cutoff=NULL → `key <= NULL` = NULL → 삭제 0 (no-op).
            let n = conn.execute(
                "DELETE FROM memory WHERE key LIKE ?1 ESCAPE '\\' AND key <= (
                     SELECT key FROM memory WHERE key LIKE ?1 ESCAPE '\\'
                     ORDER BY key DESC LIMIT 1 OFFSET ?2
                 )",
                // u64를 그대로 i64로 줄여 음수가 되면 OFFSET이 0처럼 적용될 수 있다.
                params![like, keep_recent.min(i64::MAX as u64) as i64],
            )?;
            Ok((n as u64, pruned_effect(conn, n)))
        })
    }

    /// prefix 뒤 13자리 시각이 cutoff_ms 미만인 로그를 이벤트 없이 삭제한다.
    /// 개수 제한과 함께 사용하며 삭제 수를 반환한다. prefix의 LIKE 특수문자는 이스케이프한다.
    pub fn prune_prefix_older_than(&mut self, prefix: &str, cutoff_ms: u64) -> Result<u64> {
        let like = like_prefix(prefix);
        // 같은 시각에 .seq가 붙은 키는 경계보다 커서 남는다.
        let boundary = format!("{prefix}{cutoff_ms:013}");
        self.maintenance_step(|conn| {
            let n = conn.execute(
                "DELETE FROM memory WHERE key LIKE ?1 ESCAPE '\\' AND key < ?2",
                params![like, boundary],
            )?;
            Ok((n as u64, pruned_effect(conn, n)))
        })
    }
}

fn like_prefix(prefix: &str) -> String {
    format!(
        "{}%",
        prefix
            .replace('\\', "\\\\")
            .replace('_', "\\_")
            .replace('%', "\\%")
    )
}

/// 로그 정리는 행을 지웠을 때만 quota 카운터를 다시 센다.
fn pruned_effect(conn: &Connection, deleted: usize) -> Effect {
    Effect {
        regular_used: (deleted > 0).then(|| MemoryStore::scan_regular_used(conn)),
        changes: Vec::new(),
        wrote: deleted > 0,
    }
}

// 이유: put의 판정 입력(대상·값·옵션·quota)을 그대로 받는 내부 함수다.
#[allow(clippy::too_many_arguments)]
fn put_regular_rows(
    conn: &Connection,
    used_before: i64,
    quota: u64,
    owner: &str,
    scope_token: String,
    key: &str,
    bytes: &[u8],
    content_type: &str,
    opts: &PutOpts,
) -> Result<(u64, Effect)> {
    let now = unix_ms_now();
    let existing: Option<(u64, String, i64)> = conn
        .query_row(
            "SELECT version, owner, LENGTH(value) FROM memory WHERE scope=?1 AND key=?2",
            params![&scope_token, key],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;

    // 기존 값의 크기를 빼고 새 값의 크기를 더해 전체 스캔 없이 quota를 검사한다.
    let existing_size = existing.as_ref().map(|(_, _, sz)| *sz).unwrap_or(0);
    let projected = used_before - existing_size + bytes.len() as i64;
    if (projected as u64) > quota {
        return Err(MemoryError::QuotaExceeded {
            area: MemoryArea::Regular,
            used: projected.max(0) as u64,
            limit: quota,
        });
    }

    let (new_version, change_kind) = match (existing, opts.cas) {
        (None, Some(expected)) => {
            return Err(MemoryError::CasConflict {
                expected,
                actual: 0,
            });
        }
        (Some((actual, _, _)), Some(expected)) if actual != expected => {
            return Err(MemoryError::CasConflict { expected, actual });
        }
        (Some((_, existing_owner, _)), _) if !owner_can_modify(owner, &existing_owner) => {
            return Err(MemoryError::OwnedByOther {
                owner: existing_owner,
            });
        }
        (Some((actual, _, _)), _) => {
            let new_v = actual + 1;
            conn.execute(
                "UPDATE memory SET value=?1, content_type=?2, updated_at=?3,
                                   expires_at=?4, version=?5
                 WHERE scope=?6 AND key=?7",
                params![
                    bytes,
                    content_type,
                    now,
                    opts.expires_at,
                    new_v as i64,
                    &scope_token,
                    key
                ],
            )?;
            (new_v, MemoryChangeKind::Updated)
        }
        (None, _) => {
            conn.execute(
                "INSERT INTO memory
                   (scope, key, value, content_type, created_at, updated_at, expires_at,
                    version, owner)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, 1, ?7)",
                params![
                    &scope_token,
                    key,
                    bytes,
                    content_type,
                    now,
                    opts.expires_at,
                    owner
                ],
            )?;
            (1, MemoryChangeKind::Created)
        }
    };

    Ok((
        new_version,
        Effect {
            regular_used: Some(projected),
            changes: vec![MemoryChange {
                scope: scope_token,
                key: key.to_string(),
                kind: change_kind,
                version: Some(new_version),
            }],
            wrote: true,
        },
    ))
}

fn delete_regular_rows(
    conn: &Connection,
    used_before: i64,
    owner: &str,
    scope_token: String,
    key: &str,
    cas: Option<u64>,
) -> Result<((), Effect)> {
    let existing: Option<(u64, String, i64)> = conn
        .query_row(
            "SELECT version, owner, LENGTH(value) FROM memory WHERE scope=?1 AND key=?2",
            params![&scope_token, key],
            |r| {
                Ok((
                    r.get::<_, i64>(0)? as u64,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;

    // 삭제될 entry 의 크기 (없으면 0). commit 후 카운터 감산에 사용.
    let deleted_size = existing.as_ref().map(|(_, _, sz)| *sz).unwrap_or(0);

    match (existing, cas) {
        (None, _) => {
            return Err(MemoryError::NotFound {
                scope: scope_token,
                key: key.to_string(),
            });
        }
        (Some((actual, _, _)), Some(expected)) if actual != expected => {
            return Err(MemoryError::CasConflict { expected, actual });
        }
        (Some((_, existing_owner, _)), _) if !owner_can_modify(owner, &existing_owner) => {
            return Err(MemoryError::OwnedByOther {
                owner: existing_owner,
            });
        }
        _ => {}
    }

    conn.execute(
        "DELETE FROM memory WHERE scope=?1 AND key=?2",
        params![&scope_token, key],
    )?;
    Ok((
        (),
        Effect {
            regular_used: Some((used_before - deleted_size).max(0)),
            changes: vec![MemoryChange {
                scope: scope_token,
                key: key.to_string(),
                kind: MemoryChangeKind::Deleted,
                version: None,
            }],
            wrote: true,
        },
    ))
}

// 이유: put_secret의 판정 입력(대상·값·옵션·quota)을 그대로 받는 내부 함수다.
#[allow(clippy::too_many_arguments)]
fn put_secret_rows(
    conn: &Connection,
    quota: u64,
    owner: &str,
    scope_token: &str,
    key: &str,
    bytes: &[u8],
    content_type: &str,
    opts: &PutOpts,
) -> Result<(u64, Effect)> {
    let now = unix_ms_now();
    let existing: Option<(u64, i64)> = conn
        .query_row(
            "SELECT version, LENGTH(value) FROM memory_secret
             WHERE owner=?1 AND scope=?2 AND key=?3",
            params![owner, scope_token, key],
            |r| Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)?)),
        )
        .optional()?;

    // Quota: per-owner secret 한도. 평문 byte 기준. 같은 transaction의 앞선 쓰기도 합산된다.
    let existing_size = existing.as_ref().map(|(_, sz)| *sz).unwrap_or(0);
    let current_used: i64 = conn.query_row(
        "SELECT COALESCE(SUM(LENGTH(value)), 0) FROM memory_secret WHERE owner=?1",
        params![owner],
        |r| r.get(0),
    )?;
    let projected = current_used - existing_size + bytes.len() as i64;
    if (projected as u64) > quota {
        return Err(MemoryError::QuotaExceeded {
            area: MemoryArea::Secret,
            used: projected.max(0) as u64,
            limit: quota,
        });
    }

    let new_version = match (existing, opts.cas) {
        (None, Some(expected)) => {
            return Err(MemoryError::CasConflict {
                expected,
                actual: 0,
            });
        }
        (Some((actual, _)), Some(expected)) if actual != expected => {
            return Err(MemoryError::CasConflict { expected, actual });
        }
        (Some((actual, _)), _) => {
            let new_v = actual + 1;
            conn.execute(
                "UPDATE memory_secret SET value=?1, content_type=?2, updated_at=?3,
                                          expires_at=?4, version=?5
                 WHERE owner=?6 AND scope=?7 AND key=?8",
                params![
                    bytes,
                    content_type,
                    now,
                    opts.expires_at,
                    new_v as i64,
                    owner,
                    scope_token,
                    key
                ],
            )?;
            new_v
        }
        (None, _) => {
            conn.execute(
                "INSERT INTO memory_secret
                   (owner, scope, key, value, content_type, created_at, updated_at,
                    expires_at, version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, 1)",
                params![
                    owner,
                    scope_token,
                    key,
                    bytes,
                    content_type,
                    now,
                    opts.expires_at
                ],
            )?;
            1
        }
    };
    Ok((new_version, Effect::secret_write()))
}

fn delete_secret_rows(
    conn: &Connection,
    owner: &str,
    scope_token: String,
    key: &str,
    cas: Option<u64>,
) -> Result<((), Effect)> {
    let existing: Option<u64> = conn
        .query_row(
            "SELECT version FROM memory_secret
             WHERE owner=?1 AND scope=?2 AND key=?3",
            params![owner, &scope_token, key],
            |r| r.get::<_, i64>(0).map(|v| v as u64),
        )
        .optional()?;

    match (existing, cas) {
        (None, _) => {
            return Err(MemoryError::NotFound {
                scope: scope_token,
                key: key.to_string(),
            });
        }
        (Some(actual), Some(expected)) if actual != expected => {
            return Err(MemoryError::CasConflict { expected, actual });
        }
        _ => {}
    }

    conn.execute(
        "DELETE FROM memory_secret WHERE owner=?1 AND scope=?2 AND key=?3",
        params![owner, &scope_token, key],
    )?;
    Ok(((), Effect::secret_write()))
}

fn purge_expired_rows(conn: &Connection) -> Result<(PurgeStats, Effect)> {
    let now = unix_ms_now();
    // Regular: 발화할 key 목록을 먼저 수집한 뒤 delete (secret 은 발화하지 않으므로
    // 단순 count 만 필요).
    let expired_regular: Vec<(String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT scope, key FROM memory
             WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        )?;
        let rows = stmt.query_map(params![now], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let regular = conn.execute(
        "DELETE FROM memory WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        params![now],
    )?;
    let secret = conn.execute(
        "DELETE FROM memory_secret WHERE expires_at IS NOT NULL AND expires_at <= ?1",
        params![now],
    )?;
    let changes = expired_regular
        .into_iter()
        .map(|(scope, key)| MemoryChange {
            scope,
            key,
            kind: MemoryChangeKind::Expired,
            version: None,
        })
        .collect();
    Ok((
        PurgeStats {
            regular: regular as u64,
            secret: secret as u64,
        },
        Effect {
            regular_used: Some(MemoryStore::scan_regular_used(conn)),
            changes,
            wrote: regular + secret > 0,
        },
    ))
}

fn purge_scope_rows(conn: &Connection, token: String) -> Result<(PurgeStats, Effect)> {
    // Regular keys 발화용 수집 (secret 은 발화 안 함).
    let cleared_keys: Vec<String> = {
        let mut stmt = conn.prepare("SELECT key FROM memory WHERE scope=?1")?;
        let rows = stmt.query_map(params![&token], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let regular = conn.execute("DELETE FROM memory WHERE scope=?1", params![&token])?;
    let secret = conn.execute("DELETE FROM memory_secret WHERE scope=?1", params![&token])?;
    let changes = cleared_keys
        .into_iter()
        .map(|key| MemoryChange {
            scope: token.clone(),
            key,
            kind: MemoryChangeKind::Deleted,
            version: None,
        })
        .collect();
    Ok((
        PurgeStats {
            regular: regular as u64,
            secret: secret as u64,
        },
        Effect {
            regular_used: Some(MemoryStore::scan_regular_used(conn)),
            changes,
            wrote: regular + secret > 0,
        },
    ))
}
