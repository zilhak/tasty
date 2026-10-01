//! 불변 payload generation. 내용은 한 번 쓰면 바꾸지 않는다. 삭제는 참조(pin) 해제이고,
//! 실제 행 삭제는 참조가 하나도 없는 payload만 지우는 [`EventStore::gc_payloads`]가 한다.
//!
//! 새로 넣은 payload는 참조가 생기기 전까지 GC 대상이다. 참조할 기록을 commit하기 전에 GC가
//! 돌았으면 그 commit은 [`StoreError::PayloadMissing`]으로 실패한다.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::error::{StoreError, StoreResult};
use crate::store::{EventStore, to_i64, to_u64};
use crate::types::{PayloadRef, WriterEpoch};

impl EventStore {
    /// payload를 새 generation으로 저장한다. 같은 내용이어도 새 참조를 만든다.
    pub fn put_payload(&mut self, epoch: WriterEpoch, bytes: &[u8]) -> StoreResult<PayloadRef> {
        let tx = self.write_tx(epoch)?;
        let payload = insert(&tx, bytes)?;
        tx.commit()?;
        Ok(payload)
    }

    /// Create an immutable payload and its in-flight holder in one transaction. GC cannot run
    /// between creation and the eventual event pin, even when command admission is asynchronous.
    pub fn put_payload_pinned(
        &mut self,
        epoch: WriterEpoch,
        bytes: &[u8],
        holder: &str,
    ) -> StoreResult<PayloadRef> {
        let tx = self.write_tx(epoch)?;
        let payload = insert(&tx, bytes)?;
        pin_in(&tx, payload, holder)?;
        tx.commit()?;
        Ok(payload)
    }

    /// Cumulative preparation bound for one unresolved admission. This is not an effect-result
    /// write: accepted effect completion continues to use put_payload_pinned under pressure.
    pub fn put_admission_payload_pinned(
        &mut self,
        epoch: WriterEpoch,
        bytes: &[u8],
        holder: &str,
    ) -> StoreResult<PayloadRef> {
        let limit = self.admission_budget.command_credit_bytes;
        let tx = self.write_tx(epoch)?;
        let used:u64=tx.query_row("SELECT COALESCE(SUM(length(p.bytes)),0) FROM payloads p JOIN payload_pins h ON h.payload_id=p.payload_id WHERE h.holder=?1",[holder],|row|row.get(0))?;
        let requested = bytes.len() as u64;
        if used
            .checked_add(requested)
            .is_none_or(|total| total > limit)
        {
            return Err(StoreError::AdmissionPayloadCapacity {
                used,
                requested,
                limit,
            });
        }
        let payload = insert(&tx, bytes)?;
        pin_in(&tx, payload, holder)?;
        tx.commit()?;
        Ok(payload)
    }

    /// Release only one explicit holder; event/snapshot/undo pins are not affected.
    pub fn release_payload_holder(&mut self, epoch: WriterEpoch, holder: &str) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        tx.execute("DELETE FROM payload_pins WHERE holder = ?1", [holder])?;
        tx.commit()?;
        Ok(())
    }

    /// Admission holders are process-local preparation. A fenced new writer can remove the old
    /// namespace: accepted commands already have event pins, unaccepted inputs may be collected.
    pub fn release_abandoned_admission_holders(&mut self, epoch: WriterEpoch) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        let current = format!("admission/{}/", epoch.0);
        tx.execute("DELETE FROM payload_pins WHERE substr(holder,1,10) = 'admission/' AND substr(holder,1,length(?1)) != ?1",[current])?;
        tx.commit()?;
        Ok(())
    }

    /// Atomically transfer an owner's complete reference set. An invalid replacement rolls back
    /// the release as well, so a failed capture/import cannot expose the previous data to GC.
    pub fn replace_payload_holder(
        &mut self,
        epoch: WriterEpoch,
        holder: &str,
        references: &[PayloadRef],
    ) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        replace_holder_in(&tx, holder, references)?;
        tx.commit()?;
        Ok(())
    }

    /// Copy into independently owned immutable generations. The source holder is durable before
    /// the destination transaction starts and is deliberately not released here: only a successful
    /// destination import plus manifest handoff authorizes the caller to release it.
    pub fn import_payloads(
        &mut self,
        epoch: WriterEpoch,
        source: &mut EventStore,
        source_epoch: WriterEpoch,
        references: &[PayloadRef],
        source_holder: &str,
        destination_holder: &str,
    ) -> StoreResult<Vec<(PayloadRef, PayloadRef)>> {
        let mut size = crate::write_limits::Budget::new();
        let source_tx = source.write_tx(source_epoch)?;
        let mut contents = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for reference in references {
            if seen.insert(reference.0) {
                let bytes = read_verified(&source_tx, *reference)?;
                crate::write_limits::blob(&bytes)?;
                size.add(bytes.len())?;
                size.add(256)?;
                pin_in(&source_tx, *reference, source_holder)?;
                contents.push((*reference, bytes));
            }
        }
        source_tx.commit()?;
        let tx = self.write_tx(epoch)?;
        let mut mapping = Vec::new();
        for (reference, bytes) in contents {
            let copied = insert(&tx, &bytes)?;
            pin_in(&tx, copied, destination_holder)?;
            mapping.push((reference, copied));
        }
        tx.commit()?;
        Ok(mapping)
    }

    /// 내용을 읽고 checksum을 검증한다.
    pub fn read_payload(&self, payload: PayloadRef) -> StoreResult<Vec<u8>> {
        read_verified(&self.conn, payload)
    }

    /// Check size in the same read snapshot before allocating the BLOB buffer.
    pub fn read_payload_bounded(&self, payload: PayloadRef, limit: usize) -> StoreResult<Vec<u8>> {
        let tx = self.conn.unchecked_transaction()?;
        let size: Option<i64> = tx
            .query_row(
                "SELECT length(bytes) FROM payloads WHERE payload_id = ?1",
                [to_i64(payload.0)?],
                |row| row.get(0),
            )
            .optional()?;
        let size = to_u64(size.ok_or(StoreError::PayloadMissing(payload.0))?)?;
        if size > limit as u64 {
            return Err(StoreError::PayloadTooLarge {
                payload: payload.0,
                size,
                limit,
            });
        }
        read_verified(&tx, payload)
    }

    /// 외부 보유자(undo·View 복원 기록·import 등)의 참조를 건다.
    pub fn pin_payload(
        &mut self,
        epoch: WriterEpoch,
        payload: PayloadRef,
        holder: &str,
    ) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        pin_in(&tx, payload, holder)?;
        tx.commit()?;
        Ok(())
    }

    /// 참조를 푼다. 내용은 GC가 지울 때까지 남는다.
    pub fn unpin_payload(
        &mut self,
        epoch: WriterEpoch,
        payload: PayloadRef,
        holder: &str,
    ) -> StoreResult<()> {
        let tx = self.write_tx(epoch)?;
        tx.execute(
            "DELETE FROM payload_pins WHERE payload_id = ?1 AND holder = ?2",
            params![to_i64(payload.0)?, holder],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// payload의 현재 참조 보유자 목록.
    pub fn payload_holders(&self, payload: PayloadRef) -> StoreResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT holder FROM payload_pins WHERE payload_id = ?1 ORDER BY holder")?;
        let rows = stmt.query_map([to_i64(payload.0)?], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 참조가 없는 payload를 지우고 지운 개수를 돌려준다.
    pub fn gc_payloads(&mut self, epoch: WriterEpoch) -> StoreResult<usize> {
        let tx = self.write_tx(epoch)?;
        let removed = tx.execute(
            "DELETE FROM payloads
             WHERE payload_id NOT IN (SELECT payload_id FROM payload_pins)",
            [],
        )?;
        tx.commit()?;
        Ok(removed)
    }
}

pub(crate) fn insert(conn: &Connection, bytes: &[u8]) -> StoreResult<PayloadRef> {
    crate::write_limits::blob(bytes)?;
    let digest = Sha256::digest(bytes).to_vec();
    conn.execute(
        "INSERT INTO payloads (sha256, bytes) VALUES (?1, ?2)",
        params![digest, bytes],
    )?;
    Ok(PayloadRef(to_u64(conn.last_insert_rowid())?))
}

pub(crate) fn pin_in(conn: &Connection, payload: PayloadRef, holder: &str) -> StoreResult<()> {
    let id = to_i64(payload.0)?;
    let exists: Option<i64> = conn
        .query_row(
            "SELECT payload_id FROM payloads WHERE payload_id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(StoreError::PayloadMissing(payload.0));
    }
    conn.execute(
        "INSERT OR IGNORE INTO payload_pins (payload_id, holder) VALUES (?1, ?2)",
        params![id, holder],
    )?;
    Ok(())
}

pub(crate) fn read_verified(conn: &Connection, payload: PayloadRef) -> StoreResult<Vec<u8>> {
    let row: Option<(Vec<u8>, Vec<u8>)> = conn
        .query_row(
            "SELECT sha256, bytes FROM payloads WHERE payload_id = ?1",
            [to_i64(payload.0)?],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((digest, bytes)) = row else {
        return Err(StoreError::PayloadMissing(payload.0));
    };
    if Sha256::digest(&bytes).as_slice() != digest.as_slice() {
        return Err(StoreError::PayloadCorrupt(payload.0));
    }
    Ok(bytes)
}

pub(crate) fn replace_holder_in(
    conn: &Connection,
    holder: &str,
    references: &[PayloadRef],
) -> StoreResult<()> {
    // Verify before replacing: checksum damage is not a valid ownership transfer either.
    for reference in references {
        read_verified(conn, *reference)?;
    }
    conn.execute("DELETE FROM payload_pins WHERE holder = ?1", [holder])?;
    for reference in references {
        pin_in(conn, *reference, holder)?;
    }
    Ok(())
}
