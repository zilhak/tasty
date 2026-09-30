//! journal DB 스키마와 migration. 적용한 버전은 `schema_migrations` 표에 한 줄씩 남긴다.
//! 이 빌드가 아는 버전보다 새 journal은 열지 않는다. 모르는 형식을 조용히 읽지 않기 위해서다.

use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

use crate::error::{StoreError, StoreResult};

/// 순서대로 적용하는 migration. 인덱스 + 1이 버전이다. 이미 배포한 항목은 고치지 않고 뒤에 추가한다.
const MIGRATIONS: &[&str] = &[V1];

/// 이 빌드가 읽고 쓸 수 있는 가장 새 스키마 버전.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

const V1: &str = r#"
    -- journal 한 개의 식별·writer 세대·활성 여부. 한 줄만 둔다.
    CREATE TABLE journal_meta (
        singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
        journal_id TEXT NOT NULL,
        writer_epoch INTEGER NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('active', 'archived'))
    );

    CREATE TABLE stream_heads (
        stream_id TEXT PRIMARY KEY,
        revision INTEGER NOT NULL CHECK (revision > 0)
    );

    -- AUTOINCREMENT로 batch 번호를 재사용하지 않는다.
    CREATE TABLE batches (
        batch_id INTEGER PRIMARY KEY AUTOINCREMENT,
        command_id TEXT,
        writer_epoch INTEGER NOT NULL
    );

    -- batch가 바꾼 stream별 revision 범위. revision vector의 원본이다.
    CREATE TABLE batch_revisions (
        batch_id INTEGER NOT NULL REFERENCES batches(batch_id),
        stream_id TEXT NOT NULL,
        first_revision INTEGER NOT NULL,
        last_revision INTEGER NOT NULL,
        PRIMARY KEY (batch_id, stream_id)
    );
    CREATE INDEX idx_batch_revisions_stream ON batch_revisions(stream_id, batch_id);

    CREATE TABLE events (
        stream_id TEXT NOT NULL,
        stream_revision INTEGER NOT NULL,
        event_id TEXT NOT NULL UNIQUE,
        batch_id INTEGER NOT NULL REFERENCES batches(batch_id),
        batch_index INTEGER NOT NULL,
        type_tag TEXT NOT NULL,
        schema_version INTEGER NOT NULL,
        payload BLOB NOT NULL,
        recorded_at_ms INTEGER NOT NULL,
        command_id TEXT,
        causation_id TEXT,
        actor TEXT NOT NULL,
        origin TEXT NOT NULL,
        PRIMARY KEY (stream_id, stream_revision),
        UNIQUE (batch_id, batch_index)
    );

    -- 재시도 키가 없는 내부 명령은 두 열이 모두 NULL이다. NULL끼리는 UNIQUE에 걸리지 않는다.
    CREATE TABLE commands (
        command_id TEXT PRIMARY KEY,
        caller_scope TEXT,
        idempotency_key TEXT,
        request_digest BLOB NOT NULL,
        resolved BLOB NOT NULL,
        status TEXT NOT NULL,
        response BLOB,
        CHECK ((caller_scope IS NULL) = (idempotency_key IS NULL)),
        UNIQUE (caller_scope, idempotency_key)
    );

    CREATE TABLE effects (
        effect_id TEXT PRIMARY KEY,
        operation_id TEXT NOT NULL,
        resource_generation INTEGER NOT NULL,
        cause_batch_id INTEGER REFERENCES batches(batch_id),
        command_id TEXT,
        type_tag TEXT NOT NULL,
        schema_version INTEGER NOT NULL,
        payload BLOB NOT NULL,
        state TEXT NOT NULL,
        attempt INTEGER NOT NULL DEFAULT 0,
        result BLOB
    );
    CREATE INDEX idx_effects_state ON effects(state);

    CREATE TABLE effect_attempts (
        effect_id TEXT NOT NULL REFERENCES effects(effect_id),
        attempt INTEGER NOT NULL,
        journal_id TEXT NOT NULL,
        engine_id TEXT NOT NULL,
        surface_id TEXT NOT NULL,
        runtime_epoch INTEGER NOT NULL,
        activation_generation INTEGER NOT NULL,
        writer_epoch INTEGER NOT NULL,
        outcome TEXT,
        PRIMARY KEY (effect_id, attempt)
    );

    -- 한 activation generation의 실행권은 effect 하나만 가진다. surface가 없으면 빈 문자열이다.
    CREATE TABLE activation_claims (
        engine_id TEXT NOT NULL,
        surface_id TEXT NOT NULL,
        runtime_epoch INTEGER NOT NULL,
        activation_generation INTEGER NOT NULL,
        effect_id TEXT NOT NULL REFERENCES effects(effect_id),
        PRIMARY KEY (engine_id, surface_id, runtime_epoch, activation_generation)
    );

    -- 불변 payload. 갱신 API가 없으며 참조가 모두 풀린 뒤 GC만 행을 지운다.
    CREATE TABLE payloads (
        payload_id INTEGER PRIMARY KEY AUTOINCREMENT,
        sha256 BLOB NOT NULL,
        bytes BLOB NOT NULL
    );

    CREATE TABLE payload_pins (
        payload_id INTEGER NOT NULL REFERENCES payloads(payload_id),
        holder TEXT NOT NULL,
        PRIMARY KEY (payload_id, holder)
    );

    CREATE TABLE snapshots (
        snapshot_id INTEGER PRIMARY KEY AUTOINCREMENT,
        batch_id INTEGER NOT NULL REFERENCES batches(batch_id),
        model_version INTEGER NOT NULL,
        payload_id INTEGER NOT NULL REFERENCES payloads(payload_id)
    );

    CREATE TABLE consumer_checkpoints (
        consumer_id TEXT NOT NULL,
        projection_version INTEGER NOT NULL,
        batch_id INTEGER NOT NULL REFERENCES batches(batch_id),
        PRIMARY KEY (consumer_id, projection_version)
    );
"#;

/// 버전 표를 만들고 부족한 migration을 한 transaction씩 적용한다.
pub(crate) fn migrate(conn: &mut Connection) -> StoreResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY)",
    )?;
    let found = current_version(conn)?;
    if found > SCHEMA_VERSION {
        return Err(StoreError::SchemaTooNew {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(found as usize) {
        let version = index as u32 + 1;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [version],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// 빈 DB이거나 버전 표가 있는 journal인지 확인한다. 다른 SQLite 파일은 읽기만 하고 거절한다.
/// 잘못된 경로로 받은 기존 DB를 journal로 바꾸지 않기 위해서다.
pub(crate) fn ensure_journal_or_empty(conn: &Connection) -> StoreResult<()> {
    let (objects, has_versions): (i64, bool) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(type = 'table' AND name = 'schema_migrations'), 0) > 0
         FROM sqlite_master",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if objects > 0 && !has_versions {
        return Err(StoreError::NotAJournal);
    }
    Ok(())
}

/// 적용된 가장 큰 버전. 표가 비었으면 0이다.
pub(crate) fn current_version(conn: &Connection) -> StoreResult<u32> {
    let version: Option<u32> = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    Ok(version.unwrap_or(0))
}

/// WAL과 `synchronous=FULL`을 요청하고 실제값을 되읽는다. 하나라도 다르면 열지 않는다.
/// in-memory 대체나 NORMAL로의 완화는 하지 않는다.
pub(crate) fn apply_durability(conn: &Connection) -> StoreResult<()> {
    let journal_mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Durability {
            pragma: "journal_mode",
            required: "wal",
            effective: journal_mode,
        });
    }
    conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")?;
    // synchronous는 되읽으면 정수로 온다. FULL은 2다.
    let synchronous: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
    if synchronous != 2 {
        return Err(StoreError::Durability {
            pragma: "synchronous",
            required: "FULL(2)",
            effective: synchronous.to_string(),
        });
    }
    let foreign_keys: i64 = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    if foreign_keys != 1 {
        return Err(StoreError::Durability {
            pragma: "foreign_keys",
            required: "ON(1)",
            effective: foreign_keys.to_string(),
        });
    }
    Ok(())
}
