//! effect 의무 기록. 원인 batch와 같은 transaction에서 만들고, 상태는 허용된 전이로만 바꾼다.
//! 이 기록은 worker claim·시도·결과의 실행 기록이다. 논리 진행은 도메인 이벤트가 원본이다.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::{StoreError, StoreResult};
use crate::store::{EventStore, to_i64, to_u32, to_u64};
use crate::types::{BatchId, OpaquePayload, WriterEpoch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectState {
    /// 지금 실행할 수 있는 대기.
    Pending,
    /// 실행 claim을 얻은 시도. 외부 실행 완료를 뜻하지 않는다.
    Running,
    /// 허용된 대기(lazy 활성화 등). 명시 활성화로만 진행한다.
    Deferred,
    Succeeded,
    /// 알려진 실패. 재시도는 같은 effect의 다음 attempt로 명시 전이한다.
    Failed,
    /// 실행하지 않기로 확정한 종료.
    Cancelled,
    /// 새 generation으로 대체된 종료.
    Superseded,
    /// Running 뒤 결과를 모른다. 대조 결과로만 벗어난다. Cancelled로 닫으려면 실행되지 않았다는 증거가 필요하다.
    Uncertain,
}

impl EffectState {
    /// 허용된 전이인지. 표에 없는 전이는 모두 거절한다.
    pub fn can_transition_to(self, to: Self) -> bool {
        use EffectState::*;
        matches!(
            (self, to),
            (Pending, Running | Deferred | Cancelled | Superseded)
                | (Deferred, Pending | Running | Cancelled | Superseded)
                | (Running, Succeeded | Failed | Uncertain)
                | (Failed, Pending)
                | (Uncertain, Succeeded | Failed | Cancelled)
        )
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Deferred => "deferred",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Superseded => "superseded",
            Self::Uncertain => "uncertain",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "running" => Self::Running,
            "deferred" => Self::Deferred,
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "superseded" => Self::Superseded,
            "uncertain" => Self::Uncertain,
            _ => return None,
        })
    }
}

/// 실행권의 키. journal은 이 저장소 자신이므로 기록 시 journal_id를 함께 남긴다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActivationClaim {
    pub engine_id: String,
    pub surface_id: Option<String>,
    pub runtime_epoch: u64,
    pub activation_generation: u64,
}

/// 새 effect. 처음 상태는 Pending 또는 Deferred만 허용한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEffect {
    pub effect_id: String,
    pub operation_id: String,
    pub resource_generation: u64,
    pub payload: OpaquePayload,
    pub initial: EffectState,
}

/// 상태 전이 요청. `from`·generation·attempt가 저장값과 다르면 늦은 결과로 보고 거절한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectTransition {
    pub effect_id: String,
    pub from: EffectState,
    pub to: EffectState,
    pub resource_generation: u64,
    /// Running에서 벗어날 때 결과를 낸 attempt 번호.
    pub attempt: Option<u32>,
    /// Running으로 갈 때 필요한 실행권.
    pub claim: Option<ActivationClaim>,
    /// 이 전이의 결과. Uncertain→Cancelled에서는 실행되지 않았다는 대조 증거이며 필수다.
    pub result: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectRecord {
    pub effect_id: String,
    pub operation_id: String,
    pub resource_generation: u64,
    pub cause_batch_id: Option<BatchId>,
    pub command_id: Option<String>,
    pub payload: OpaquePayload,
    pub state: EffectState,
    pub attempt: u32,
    pub result: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    pub attempt: u32,
    pub journal_id: String,
    pub claim: ActivationClaim,
    pub writer_epoch: WriterEpoch,
    /// Running에서 벗어난 상태. 아직 Running이면 `None`이다.
    pub outcome: Option<EffectState>,
}

impl EventStore {
    /// effect 하나의 상태 전이만 확정한다.
    pub fn transition_effect(
        &mut self,
        epoch: WriterEpoch,
        transition: &EffectTransition,
    ) -> StoreResult<()> {
        let journal_id = self.journal_id().to_owned();
        let tx = self.write_tx(epoch)?;
        apply_transition(&tx, transition, &journal_id, epoch)?;
        tx.commit()?;
        Ok(())
    }

    pub fn effect(&self, effect_id: &str) -> StoreResult<Option<EffectRecord>> {
        let row = self
            .conn
            .query_row(
                &format!("{SELECT_EFFECT} WHERE effect_id = ?1"),
                [effect_id],
                read_effect,
            )
            .optional()?;
        row.map(finish_effect).transpose()
    }

    /// 한 상태의 effect 목록. 복구가 Pending·Running·Uncertain을 대조할 때 쓴다.
    pub fn effects_in_state(&self, state: EffectState) -> StoreResult<Vec<EffectRecord>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SELECT_EFFECT} WHERE state = ?1 ORDER BY rowid"))?;
        let rows = stmt.query_map([state.as_str()], read_effect)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(finish_effect(row?)?);
        }
        Ok(out)
    }

    pub fn effect_attempts(&self, effect_id: &str) -> StoreResult<Vec<AttemptRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT attempt, journal_id, engine_id, surface_id, runtime_epoch,
                activation_generation, writer_epoch, outcome
             FROM effect_attempts WHERE effect_id = ?1 ORDER BY attempt",
        )?;
        let rows = stmt.query_map([effect_id], read_attempt)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(finish_attempt(row?)?);
        }
        Ok(out)
    }
}

const SELECT_EFFECT: &str = "SELECT effect_id, operation_id, resource_generation, cause_batch_id,
    command_id, type_tag, schema_version, payload, state, attempt, result FROM effects";

pub(crate) fn insert(
    conn: &Connection,
    new: &NewEffect,
    cause: Option<BatchId>,
    command_id: Option<&str>,
) -> StoreResult<()> {
    if !matches!(new.initial, EffectState::Pending | EffectState::Deferred) {
        return Err(StoreError::InvalidInitialEffectState {
            effect_id: new.effect_id.clone(),
            state: new.initial,
        });
    }
    conn.execute(
        "INSERT INTO effects (effect_id, operation_id, resource_generation, cause_batch_id,
            command_id, type_tag, schema_version, payload, state)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            new.effect_id,
            new.operation_id,
            to_i64(new.resource_generation)?,
            cause.map(to_i64).transpose()?,
            command_id,
            new.payload.type_tag,
            new.payload.schema_version,
            new.payload.bytes,
            new.initial.as_str(),
        ],
    )?;
    Ok(())
}

pub(crate) fn apply_transition(
    conn: &Connection,
    t: &EffectTransition,
    journal_id: &str,
    epoch: WriterEpoch,
) -> StoreResult<()> {
    let (state, generation, attempt) = load_state(conn, &t.effect_id)?;
    validate(t, state, generation)?;
    let mut next_attempt = attempt;
    if t.from == EffectState::Running {
        close_attempt(conn, t, attempt)?;
    }
    if t.to == EffectState::Running {
        next_attempt = attempt + 1;
        open_attempt(conn, t, next_attempt, journal_id, epoch)?;
    }
    conn.execute(
        "UPDATE effects SET state = ?2, attempt = ?3, result = COALESCE(?4, result)
         WHERE effect_id = ?1",
        params![t.effect_id, t.to.as_str(), next_attempt, t.result],
    )?;
    Ok(())
}

fn load_state(conn: &Connection, effect_id: &str) -> StoreResult<(EffectState, u64, u32)> {
    let row: Option<(String, i64, i64)> = conn
        .query_row(
            "SELECT state, resource_generation, attempt FROM effects WHERE effect_id = ?1",
            [effect_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((state, generation, attempt)) = row else {
        return Err(StoreError::UnknownEffect(effect_id.to_owned()));
    };
    Ok((parse_state(&state)?, to_u64(generation)?, to_u32(attempt)?))
}

fn validate(t: &EffectTransition, state: EffectState, generation: u64) -> StoreResult<()> {
    if state != t.from {
        return Err(StoreError::EffectStateMismatch {
            effect_id: t.effect_id.clone(),
            expected: t.from,
            actual: state,
        });
    }
    if generation != t.resource_generation {
        return Err(StoreError::StaleGeneration {
            effect_id: t.effect_id.clone(),
            presented: t.resource_generation,
            current: generation,
        });
    }
    if !t.from.can_transition_to(t.to) {
        return Err(StoreError::InvalidEffectTransition {
            effect_id: t.effect_id.clone(),
            from: t.from,
            to: t.to,
        });
    }
    if t.from == EffectState::Uncertain && t.to == EffectState::Cancelled && t.result.is_none() {
        return Err(StoreError::EvidenceRequired(t.effect_id.clone()));
    }
    Ok(())
}

/// 결과를 낸 attempt가 현재 attempt인지 확인하고 결과 상태를 남긴다.
fn close_attempt(conn: &Connection, t: &EffectTransition, current: u32) -> StoreResult<()> {
    if t.attempt != Some(current) {
        return Err(StoreError::StaleAttempt {
            effect_id: t.effect_id.clone(),
            presented: t.attempt,
            current,
        });
    }
    conn.execute(
        "UPDATE effect_attempts SET outcome = ?3 WHERE effect_id = ?1 AND attempt = ?2",
        params![t.effect_id, current, t.to.as_str()],
    )?;
    Ok(())
}

fn open_attempt(
    conn: &Connection,
    t: &EffectTransition,
    attempt: u32,
    journal_id: &str,
    epoch: WriterEpoch,
) -> StoreResult<()> {
    let Some(claim) = &t.claim else {
        return Err(StoreError::ClaimRequired(t.effect_id.clone()));
    };
    take_claim(conn, claim, &t.effect_id)?;
    conn.execute(
        "INSERT INTO effect_attempts (effect_id, attempt, journal_id, engine_id, surface_id,
            runtime_epoch, activation_generation, writer_epoch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            t.effect_id,
            attempt,
            journal_id,
            claim.engine_id,
            claim.surface_id.as_deref().unwrap_or(""),
            to_i64(claim.runtime_epoch)?,
            to_i64(claim.activation_generation)?,
            to_i64(epoch.0)?,
        ],
    )?;
    Ok(())
}

/// 같은 activation generation의 실행권을 얻는다. 같은 effect는 합류하고 다른 effect는 거절한다.
fn take_claim(conn: &Connection, claim: &ActivationClaim, effect_id: &str) -> StoreResult<()> {
    let surface = claim.surface_id.as_deref().unwrap_or("");
    let runtime_epoch = to_i64(claim.runtime_epoch)?;
    let generation = to_i64(claim.activation_generation)?;
    let holder: Option<String> = conn
        .query_row(
            "SELECT effect_id FROM activation_claims WHERE engine_id = ?1 AND surface_id = ?2
                AND runtime_epoch = ?3 AND activation_generation = ?4",
            params![claim.engine_id, surface, runtime_epoch, generation],
            |r| r.get(0),
        )
        .optional()?;
    match holder {
        Some(holder) if holder == effect_id => Ok(()),
        Some(holder) => Err(StoreError::ClaimHeld {
            effect_id: effect_id.to_owned(),
            holder,
        }),
        None => {
            conn.execute(
                "INSERT INTO activation_claims (engine_id, surface_id, runtime_epoch,
                    activation_generation, effect_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    claim.engine_id,
                    surface,
                    runtime_epoch,
                    generation,
                    effect_id
                ],
            )?;
            Ok(())
        }
    }
}

struct RawEffect {
    effect_id: String,
    operation_id: String,
    resource_generation: i64,
    cause_batch_id: Option<i64>,
    command_id: Option<String>,
    type_tag: String,
    schema_version: u32,
    payload: Vec<u8>,
    state: String,
    attempt: i64,
    result: Option<Vec<u8>>,
}

fn read_effect(r: &Row<'_>) -> rusqlite::Result<RawEffect> {
    Ok(RawEffect {
        effect_id: r.get(0)?,
        operation_id: r.get(1)?,
        resource_generation: r.get(2)?,
        cause_batch_id: r.get(3)?,
        command_id: r.get(4)?,
        type_tag: r.get(5)?,
        schema_version: r.get(6)?,
        payload: r.get(7)?,
        state: r.get(8)?,
        attempt: r.get(9)?,
        result: r.get(10)?,
    })
}

fn finish_effect(raw: RawEffect) -> StoreResult<EffectRecord> {
    Ok(EffectRecord {
        effect_id: raw.effect_id,
        operation_id: raw.operation_id,
        resource_generation: to_u64(raw.resource_generation)?,
        cause_batch_id: raw.cause_batch_id.map(to_u64).transpose()?,
        command_id: raw.command_id,
        payload: OpaquePayload {
            type_tag: raw.type_tag,
            schema_version: raw.schema_version,
            bytes: raw.payload,
        },
        state: parse_state(&raw.state)?,
        attempt: to_u32(raw.attempt)?,
        result: raw.result,
    })
}

type RawAttempt = (i64, String, String, String, i64, i64, i64, Option<String>);

fn read_attempt(r: &Row<'_>) -> rusqlite::Result<RawAttempt> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
    ))
}

fn finish_attempt(raw: RawAttempt) -> StoreResult<AttemptRecord> {
    let (attempt, journal_id, engine_id, surface, runtime_epoch, generation, epoch, outcome) = raw;
    Ok(AttemptRecord {
        attempt: to_u32(attempt)?,
        journal_id,
        claim: ActivationClaim {
            engine_id,
            surface_id: (!surface.is_empty()).then_some(surface),
            runtime_epoch: to_u64(runtime_epoch)?,
            activation_generation: to_u64(generation)?,
        },
        writer_epoch: WriterEpoch(to_u64(epoch)?),
        outcome: outcome.as_deref().map(parse_state).transpose()?,
    })
}

fn parse_state(s: &str) -> StoreResult<EffectState> {
    EffectState::parse(s).ok_or_else(|| StoreError::Corrupt(format!("effect state {s}")))
}
