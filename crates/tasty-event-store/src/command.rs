//! durable command identity. 재요청은 대상 존재 검사보다 먼저 이 기록으로 원래 결과를 찾는다.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{StoreError, StoreResult};
use crate::store::{EventStore, to_u64};
use crate::types::{
    CommandKey, CommandLookup, CommandRecord, CommandStatus, CommandUpdate, NewCommand,
};

const SELECT_COMMAND: &str = "SELECT command_id, caller_scope, idempotency_key, request_digest,
    resolved, status, response FROM commands";

impl EventStore {
    /// 재시도 키로 기존 명령을 찾는다. live 대상 해소 전에 호출한다.
    /// 닫힌 대상의 재요청도 여기서 원래 결과에 도달한다.
    pub fn lookup_command(&self, key: &CommandKey, digest: &[u8]) -> StoreResult<CommandLookup> {
        Ok(match find_by_key(&self.conn, key)? {
            None => CommandLookup::Miss,
            Some(record) if record.request_digest == digest => CommandLookup::Hit(record),
            Some(record) => CommandLookup::DigestMismatch(record),
        })
    }

    /// command_id로 명령을 읽는다.
    pub fn command(&self, command_id: &str) -> StoreResult<Option<CommandRecord>> {
        let sql = format!("{SELECT_COMMAND} WHERE command_id = ?1");
        let row = self
            .conn
            .query_row(&sql, [command_id], read_row)
            .optional()?;
        row.map(|r| finish(&self.conn, r)).transpose()
    }
}

pub(crate) fn find_by_key(
    conn: &Connection,
    key: &CommandKey,
) -> StoreResult<Option<CommandRecord>> {
    let sql = format!("{SELECT_COMMAND} WHERE caller_scope = ?1 AND idempotency_key = ?2");
    let row = conn
        .query_row(
            &sql,
            params![key.caller_scope, key.idempotency_key],
            read_row,
        )
        .optional()?;
    row.map(|r| finish(conn, r)).transpose()
}

pub(crate) fn insert(conn: &Connection, new: &NewCommand) -> StoreResult<()> {
    let (scope, key) = match &new.key {
        Some(k) => (
            Some(k.caller_scope.as_str()),
            Some(k.idempotency_key.as_str()),
        ),
        None => (None, None),
    };
    conn.execute(
        "INSERT INTO commands (command_id, caller_scope, idempotency_key, request_digest,
            resolved, status, response)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            new.command_id,
            scope,
            key,
            new.request_digest,
            new.resolved,
            new.status.as_str(),
            new.response,
        ],
    )?;
    Ok(())
}

/// 진행 상태를 갱신한다. 종료된 명령은 바꾸지 않는다.
pub(crate) fn apply_update(conn: &Connection, update: &CommandUpdate) -> StoreResult<()> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM commands WHERE command_id = ?1",
            [&update.command_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(status) = status else {
        return Err(StoreError::UnknownCommand(update.command_id.clone()));
    };
    let current = parse_status(&status)?;
    if current.is_terminal() {
        return Err(StoreError::CommandFinished {
            command_id: update.command_id.clone(),
            status: current,
        });
    }
    conn.execute(
        "UPDATE commands SET status = ?2, response = COALESCE(?3, response) WHERE command_id = ?1",
        params![update.command_id, update.status.as_str(), update.response],
    )?;
    Ok(())
}

struct RawCommand {
    command_id: String,
    caller_scope: Option<String>,
    idempotency_key: Option<String>,
    request_digest: Vec<u8>,
    resolved: Vec<u8>,
    status: String,
    response: Option<Vec<u8>>,
}

fn read_row(r: &Row<'_>) -> rusqlite::Result<RawCommand> {
    Ok(RawCommand {
        command_id: r.get(0)?,
        caller_scope: r.get(1)?,
        idempotency_key: r.get(2)?,
        request_digest: r.get(3)?,
        resolved: r.get(4)?,
        status: r.get(5)?,
        response: r.get(6)?,
    })
}

fn finish(conn: &Connection, raw: RawCommand) -> StoreResult<CommandRecord> {
    let key = match (raw.caller_scope, raw.idempotency_key) {
        (Some(caller_scope), Some(idempotency_key)) => Some(CommandKey {
            caller_scope,
            idempotency_key,
        }),
        _ => None,
    };
    let mut stmt =
        conn.prepare("SELECT batch_id FROM batches WHERE command_id = ?1 ORDER BY batch_id")?;
    let ids = stmt.query_map([&raw.command_id], |r| r.get::<_, i64>(0))?;
    let mut batch_ids = Vec::new();
    for id in ids {
        batch_ids.push(to_u64(id?)?);
    }
    Ok(CommandRecord {
        command_id: raw.command_id,
        key,
        request_digest: raw.request_digest,
        resolved: raw.resolved,
        status: parse_status(&raw.status)?,
        response: raw.response,
        batch_ids,
    })
}

fn parse_status(s: &str) -> StoreResult<CommandStatus> {
    CommandStatus::parse(s).ok_or_else(|| StoreError::Corrupt(format!("command status {s}")))
}
