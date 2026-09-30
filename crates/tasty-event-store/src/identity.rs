//! 영속 ID 예약. kind별 high-water를 journal에 두고, 예약한 범위는 어떤 경우에도 다시 내주지 않는다.
//!
//! 예약은 그 ID를 쓰는 이벤트 commit보다 먼저 자기 transaction으로 확정한다. 뒤이은 commit이
//! 실패하거나 예약한 ID를 다 쓰지 않으면 그 구간은 빈 채로 남는다. 값 공간은 이 journal 안이다.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{StoreError, StoreResult};
use crate::store::{EventStore, to_i64, to_u64};
use crate::types::WriterEpoch;

/// 처음 예약하는 kind의 첫 ID. 0은 호출자가 예약 상수로 쓸 수 있도록 내주지 않는다.
const FIRST_ID: u64 = 1;

/// 저장할 수 있는 가장 큰 ID. 다음 예약 위치(`end`)가 SQLite 정수(i64)에 들어가야 한다.
const STORABLE_MAX_ID: u64 = i64::MAX as u64 - 1;

/// 예약한 ID 구간 `[start, end)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdRange {
    pub kind: String,
    pub start: u64,
    pub end: u64,
}

impl IdRange {
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn contains(&self, id: u64) -> bool {
        (self.start..self.end).contains(&id)
    }

    pub fn ids(&self) -> std::ops::Range<u64> {
        self.start..self.end
    }
}

impl EventStore {
    /// `kind`의 ID `count`개를 예약한다. 마지막 ID가 `max_id`(저장 가능한 상한을 넘으면 그 상한)를 넘으면 되감지 않고
    /// [`StoreError::IdSpaceExhausted`]로 거절하며 아무것도 바꾸지 않는다.
    pub fn reserve_ids(
        &mut self,
        epoch: WriterEpoch,
        kind: &str,
        count: u64,
        max_id: u64,
    ) -> StoreResult<IdRange> {
        if count == 0 {
            return Err(StoreError::EmptyReservation(kind.to_owned()));
        }
        let tx = self.write_tx(epoch)?;
        let start = next_id(&tx, kind)?;
        let limit = max_id.min(STORABLE_MAX_ID);
        let last = start
            .checked_add(count - 1)
            .filter(|last| *last <= limit)
            .ok_or_else(|| StoreError::IdSpaceExhausted {
                kind: kind.to_owned(),
                next: start,
                count,
                max_id,
            })?;
        let end = last + 1;
        tx.execute(
            "INSERT INTO id_reservations (kind, next) VALUES (?1, ?2)
             ON CONFLICT(kind) DO UPDATE SET next = excluded.next",
            params![kind, to_i64(end)?],
        )?;
        tx.commit()?;
        Ok(IdRange {
            kind: kind.to_owned(),
            start,
            end,
        })
    }

    /// `kind`에서 다음에 예약될 ID. 아직 예약한 적이 없으면 첫 ID다.
    pub fn next_unreserved_id(&self, kind: &str) -> StoreResult<u64> {
        next_id(&self.conn, kind)
    }
}

fn next_id(conn: &Connection, kind: &str) -> StoreResult<u64> {
    let next: Option<i64> = conn
        .query_row(
            "SELECT next FROM id_reservations WHERE kind = ?1",
            [kind],
            |r| r.get(0),
        )
        .optional()?;
    next.map(to_u64).transpose().map(|n| n.unwrap_or(FIRST_ID))
}
