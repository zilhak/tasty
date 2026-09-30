//! 한 transaction의 원자 commit. revision 검증 → batch·event → command → effect 순으로 쓰고,
//! 어느 단계든 실패하면 transaction 전체를 되돌린다.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Transaction, params};

use crate::command::{self, find_by_key};
use crate::effect::{self, EffectTransition, NewEffect};
use crate::error::{StoreError, StoreResult};
use crate::payload;
use crate::store::{EventStore, stream_head, to_i64, to_u64};
use crate::types::{
    BatchCut, BatchId, CommandRecord, CommandUpdate, ExpectedRevision, NewCommand, StreamAppend,
    StreamId, WriterEpoch,
};

/// 한 번에 확정할 기록 전체.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitRequest {
    pub writer_epoch: WriterEpoch,
    /// 새 명령. 재시도 키가 이미 있으면 아무것도 쓰지 않고 [`CommitOutcome::Duplicate`]를 돌려준다.
    pub command: Option<NewCommand>,
    /// stream별 이벤트. 비어 있으면 batch를 만들지 않는다.
    pub appends: Vec<StreamAppend>,
    /// 이미 있는 명령의 진행·결과 갱신.
    pub command_updates: Vec<CommandUpdate>,
    /// 이 batch를 원인으로 하는 새 effect.
    pub effects: Vec<NewEffect>,
    /// 기존 effect의 상태 전이.
    pub effect_transitions: Vec<EffectTransition>,
}

impl CommitRequest {
    pub fn new(writer_epoch: WriterEpoch) -> Self {
        Self {
            writer_epoch,
            command: None,
            appends: Vec::new(),
            command_updates: Vec::new(),
            effects: Vec::new(),
            effect_transitions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// 기록이 확정됐다. 이벤트가 있었으면 batch 경계가 있다.
    Committed { batch: Option<BatchCut> },
    /// 같은 재시도 키와 같은 요청이 이미 있다. 저장된 기록을 그대로 사용한다.
    Duplicate(CommandRecord),
}

impl EventStore {
    /// 요청 전체를 한 transaction으로 확정한다. 오류이면 아무것도 남지 않는다.
    pub fn commit(&mut self, request: &CommitRequest) -> StoreResult<CommitOutcome> {
        let journal_id = self.journal_id().to_owned();
        let tx = self.write_tx(request.writer_epoch)?;
        if let Some(new) = &request.command
            && let Some(key) = &new.key
            && let Some(existing) = find_by_key(&tx, key)?
        {
            if existing.request_digest != new.request_digest {
                return Err(StoreError::KeyConflict {
                    caller_scope: key.caller_scope.clone(),
                    idempotency_key: key.idempotency_key.clone(),
                });
            }
            return Ok(CommitOutcome::Duplicate(existing));
        }
        let batch = write_all(&tx, request, &journal_id)?;
        tx.commit()?;
        Ok(CommitOutcome::Committed { batch })
    }
}

fn write_all(
    tx: &Transaction<'_>,
    request: &CommitRequest,
    journal_id: &str,
) -> StoreResult<Option<BatchCut>> {
    let command_id = request.command.as_ref().map(|c| c.command_id.as_str());
    if let Some(new) = &request.command {
        command::insert(tx, new)?;
    }
    let batch = if request.appends.is_empty() {
        None
    } else {
        Some(append_batch(tx, request, command_id)?)
    };
    for update in &request.command_updates {
        command::apply_update(tx, update)?;
    }
    let cause = batch.as_ref().map(|b| b.batch_id);
    for new in &request.effects {
        effect::insert(tx, new, cause, command_id)?;
    }
    for transition in &request.effect_transitions {
        effect::apply_transition(tx, transition, journal_id, request.writer_epoch)?;
    }
    Ok(batch)
}

fn append_batch(
    tx: &Transaction<'_>,
    request: &CommitRequest,
    command_id: Option<&str>,
) -> StoreResult<BatchCut> {
    check_streams(tx, &request.appends)?;
    tx.execute(
        "INSERT INTO batches (command_id, writer_epoch) VALUES (?1, ?2)",
        params![command_id, to_i64(request.writer_epoch.0)?],
    )?;
    let batch_id = to_u64(tx.last_insert_rowid())?;
    let mut revisions = BTreeMap::new();
    let mut batch_index = 0u32;
    for append in &request.appends {
        let last = append_stream(tx, append, batch_id, command_id, &mut batch_index)?;
        revisions.insert(append.stream_id.clone(), last);
    }
    Ok(BatchCut {
        batch_id,
        revisions,
    })
}

/// 모든 stream의 expected revision을 쓰기 전에 확인한다. 하나라도 다르면 batch 전체를 거절한다.
fn check_streams(tx: &Transaction<'_>, appends: &[StreamAppend]) -> StoreResult<()> {
    let mut seen = BTreeSet::new();
    for append in appends {
        if append.events.is_empty() {
            return Err(StoreError::EmptyAppend(append.stream_id.0.clone()));
        }
        if !seen.insert(&append.stream_id) {
            return Err(StoreError::DuplicateStream(append.stream_id.0.clone()));
        }
        let actual = stream_head(tx, &append.stream_id)?;
        let matches = match append.expected {
            ExpectedRevision::Any => true,
            ExpectedRevision::NoStream => actual.is_none(),
            ExpectedRevision::Exact(rev) => actual == Some(rev),
        };
        if !matches {
            return Err(StoreError::RevisionConflict {
                stream: append.stream_id.0.clone(),
                expected: append.expected,
                actual,
            });
        }
    }
    Ok(())
}

fn append_stream(
    tx: &Transaction<'_>,
    append: &StreamAppend,
    batch_id: BatchId,
    command_id: Option<&str>,
    batch_index: &mut u32,
) -> StoreResult<u64> {
    let start = stream_head(tx, &append.stream_id)?.unwrap_or(0);
    let mut revision = start;
    for event in &append.events {
        revision += 1;
        tx.execute(
            "INSERT INTO events (stream_id, stream_revision, event_id, batch_id, batch_index,
                type_tag, schema_version, payload, recorded_at_ms, command_id, causation_id,
                actor, origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                append.stream_id.as_str(),
                to_i64(revision)?,
                event.event_id,
                to_i64(batch_id)?,
                *batch_index,
                event.payload.type_tag,
                event.payload.schema_version,
                event.payload.bytes,
                to_i64(event.recorded_at_ms)?,
                command_id,
                event.causation_id,
                event.actor,
                event.origin,
            ],
        )?;
        for payload_ref in &event.payload_refs {
            payload::pin_in(tx, *payload_ref, &event_holder(&event.event_id))?;
        }
        *batch_index += 1;
    }
    record_head(tx, &append.stream_id, batch_id, start + 1, revision)?;
    Ok(revision)
}

fn record_head(
    tx: &Transaction<'_>,
    stream: &StreamId,
    batch_id: BatchId,
    first: u64,
    last: u64,
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO batch_revisions (batch_id, stream_id, first_revision, last_revision)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            to_i64(batch_id)?,
            stream.as_str(),
            to_i64(first)?,
            to_i64(last)?
        ],
    )?;
    tx.execute(
        "INSERT INTO stream_heads (stream_id, revision) VALUES (?1, ?2)
         ON CONFLICT(stream_id) DO UPDATE SET revision = excluded.revision",
        params![stream.as_str(), to_i64(last)?],
    )?;
    Ok(())
}

/// 이벤트가 payload를 참조할 때 쓰는 pin holder 이름.
pub fn event_holder(event_id: &str) -> String {
    format!("event:{event_id}")
}
