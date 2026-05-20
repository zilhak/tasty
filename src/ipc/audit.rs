//! Phase 6.5 — IPC audit log.
//!
//! 모든 IPC 호출(allow + deny) 을 `tasty.audit.{ts:013}.{seq:04}` 키로 영속한다.
//! 권한 거부 사고 추적, agent 행동 감사, capability_elevation 사후 분석에 쓴다.
//!
//! 보존 정책: 기본 30 일 (`DEFAULT_RETENTION_MS`). load 시 lazy evict — query
//! 가 호출될 때 만료된 record 를 함께 삭제한다.
//!
//! 스토리지는 Global scope — workspace 가 닫혀도 audit 은 유지되어야 한다.
//! workspace_id 는 record 안에 함께 기록.

use serde::{Deserialize, Serialize};
use tasty_memory::{ListOpts, MemoryError, MemoryStore, MemoryValue, PutOpts, Scope};

use crate::ipc::caller::CallerContext;

pub const AUDIT_KEY_PREFIX: &str = "tasty.audit.";
/// 30일 (ms). 운영자가 변경하고 싶으면 `audit_clear` 로 명시 삭제하거나 후속
/// phase 에서 정책 설정 IPC 를 추가한다.
pub const DEFAULT_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditCallerKind {
    Local,
    Plugin,
    Agent,
}

impl AuditCallerKind {
    pub fn from_caller(caller: &CallerContext) -> Self {
        match caller {
            CallerContext::Local => Self::Local,
            CallerContext::Plugin { .. } => Self::Plugin,
            CallerContext::Agent { .. } => Self::Agent,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub ts_ms: u64,
    pub seq: u64,
    pub caller_kind: AuditCallerKind,
    /// plugin_id / agent_id / `_host` (Local/Internal).
    pub caller_id: String,
    pub method: String,
    pub decision: AuditDecision,
    /// deny 사유 (CallerError 메시지 등). allow 면 보통 None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<u32>,
}

impl AuditRecord {
    /// `tasty.audit.{ts:013}.{seq:04}` — 시간순 prefix 정렬을 위해 zero-padding.
    pub fn storage_key(&self) -> String {
        format!("{AUDIT_KEY_PREFIX}{:013}.{:04}", self.ts_ms, self.seq)
    }
}

#[derive(Debug)]
pub enum AuditError {
    Memory(MemoryError),
    Serde(serde_json::Error),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory(e) => write!(f, "memory: {e}"),
            Self::Serde(e) => write!(f, "serde: {e}"),
        }
    }
}

impl std::error::Error for AuditError {}

impl From<MemoryError> for AuditError {
    fn from(e: MemoryError) -> Self {
        Self::Memory(e)
    }
}

impl From<serde_json::Error> for AuditError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}

pub type Result<T> = std::result::Result<T, AuditError>;

#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub caller_kind: Option<AuditCallerKind>,
    pub caller_id: Option<String>,
    /// 메서드 접두사 매칭 (예: `surface.` 로 surface.* 전체).
    pub method_prefix: Option<String>,
    pub decision: Option<AuditDecision>,
    pub since_ms: Option<u64>,
    pub until_ms: Option<u64>,
    /// 결과 최대 개수. None=무제한 (실제 호출에서는 항상 cap 권장).
    pub limit: Option<usize>,
}

impl AuditQuery {
    pub fn matches(&self, r: &AuditRecord) -> bool {
        if let Some(ref kind) = self.caller_kind
            && r.caller_kind != *kind
        {
            return false;
        }
        if let Some(ref id) = self.caller_id
            && &r.caller_id != id
        {
            return false;
        }
        if let Some(ref prefix) = self.method_prefix
            && !r.method.starts_with(prefix.as_str())
        {
            return false;
        }
        if let Some(ref dec) = self.decision
            && r.decision != *dec
        {
            return false;
        }
        if let Some(since) = self.since_ms
            && r.ts_ms < since
        {
            return false;
        }
        if let Some(until) = self.until_ms
            && r.ts_ms > until
        {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AuditSummary {
    pub total: u64,
    pub allow: u64,
    pub deny: u64,
    pub by_caller: Vec<(String, u64)>,
    pub by_method: Vec<(String, u64)>,
}

pub struct AuditStore<'a> {
    mem: &'a mut MemoryStore,
    owner: String,
}

impl<'a> AuditStore<'a> {
    pub fn new(mem: &'a mut MemoryStore, owner: impl Into<String>) -> Self {
        Self {
            mem,
            owner: owner.into(),
        }
    }

    pub fn append(&mut self, record: &AuditRecord) -> Result<()> {
        let value = MemoryValue::Json(serde_json::to_value(record)?);
        self.mem.put(
            &self.owner,
            &Scope::Global,
            &record.storage_key(),
            &value,
            &PutOpts::default(),
        )?;
        Ok(())
    }

    /// 모든 record 를 시간 순으로 반환. `now_ms` 보다 30일 이상 오래된 record 는
    /// evict (lazy retention). retention 은 `retention_ms=0` 으로 끄면 evict 없음
    /// (테스트용).
    pub fn list(&mut self, retention_ms: u64, now_ms: u64) -> Result<Vec<AuditRecord>> {
        let opts = ListOpts {
            prefix: Some(AUDIT_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&Scope::Global, &opts)?;
        let cutoff = if retention_ms > 0 {
            now_ms.saturating_sub(retention_ms)
        } else {
            0
        };
        let mut alive: Vec<AuditRecord> = Vec::with_capacity(entries.len());
        let mut to_evict: Vec<String> = Vec::new();
        for e in entries {
            let MemoryValue::Json(v) = e.value else {
                continue;
            };
            let rec: AuditRecord = match serde_json::from_value(v) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if retention_ms > 0 && rec.ts_ms < cutoff {
                to_evict.push(e.key);
            } else {
                alive.push(rec);
            }
        }
        for key in to_evict {
            let _ = self.mem.delete(&self.owner, &Scope::Global, &key, None);
        }
        alive.sort_by_key(|r| (r.ts_ms, r.seq));
        Ok(alive)
    }

    pub fn query(
        &mut self,
        q: &AuditQuery,
        retention_ms: u64,
        now_ms: u64,
    ) -> Result<Vec<AuditRecord>> {
        let all = self.list(retention_ms, now_ms)?;
        let mut out: Vec<AuditRecord> = all.into_iter().filter(|r| q.matches(r)).collect();
        if let Some(limit) = q.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    pub fn summary(
        &mut self,
        q: &AuditQuery,
        retention_ms: u64,
        now_ms: u64,
        top_n: usize,
    ) -> Result<AuditSummary> {
        let records = self.query(q, retention_ms, now_ms)?;
        let mut s = AuditSummary {
            total: records.len() as u64,
            ..Default::default()
        };
        let mut by_caller: std::collections::BTreeMap<String, u64> =
            std::collections::BTreeMap::new();
        let mut by_method: std::collections::BTreeMap<String, u64> =
            std::collections::BTreeMap::new();
        for r in &records {
            match r.decision {
                AuditDecision::Allow => s.allow += 1,
                AuditDecision::Deny => s.deny += 1,
            }
            *by_caller.entry(r.caller_id.clone()).or_insert(0) += 1;
            *by_method.entry(r.method.clone()).or_insert(0) += 1;
        }
        s.by_caller = top_counts(by_caller, top_n);
        s.by_method = top_counts(by_method, top_n);
        Ok(s)
    }

    /// `tail -f` 스타일 폴링. `(after_ts_ms, after_seq)` 보다 strictly 큰 record
    /// 만 시간 순으로 반환. 커서가 없으면 현재 latest record 의 (ts, seq) 를
    /// 그대로 돌려준다 — 호출자가 그 다음 호출부터 새로 들어온 것만 받게 된다
    /// (`tail -f -n 0` 시멘틱). `limit` 가 있으면 cap.
    ///
    /// 반환 `next_after_ts_ms` / `next_after_seq` 는 마지막 반환된 record 의 값,
    /// 새 record 가 없으면 입력 커서 그대로.
    pub fn follow(
        &mut self,
        q: &AuditQuery,
        after_ts_ms: Option<u64>,
        after_seq: Option<u64>,
        retention_ms: u64,
        now_ms: u64,
        limit: Option<usize>,
    ) -> Result<(Vec<AuditRecord>, u64, u64)> {
        let all = self.list(retention_ms, now_ms)?;
        let cursor = (after_ts_ms.unwrap_or(0), after_seq.unwrap_or(0));
        let cursor_given = after_ts_ms.is_some() || after_seq.is_some();
        if !cursor_given {
            // 초기 호출: latest 의 (ts,seq) 만 반환, record 는 빈 배열.
            let last = all.last().map(|r| (r.ts_ms, r.seq)).unwrap_or((0, 0));
            return Ok((Vec::new(), last.0, last.1));
        }
        let mut out: Vec<AuditRecord> = all
            .into_iter()
            .filter(|r| (r.ts_ms, r.seq) > cursor && q.matches(r))
            .collect();
        if let Some(cap) = limit {
            out.truncate(cap);
        }
        let next = out
            .last()
            .map(|r| (r.ts_ms, r.seq))
            .unwrap_or(cursor);
        Ok((out, next.0, next.1))
    }

    /// 전체 삭제. `before_ms` 가 있으면 그 시점 이전 record 만 삭제.
    /// 반환: 삭제 개수.
    pub fn clear(&mut self, before_ms: Option<u64>) -> Result<usize> {
        let opts = ListOpts {
            prefix: Some(AUDIT_KEY_PREFIX.to_string()),
            ..Default::default()
        };
        let entries = self.mem.list(&Scope::Global, &opts)?;
        let mut removed = 0usize;
        for e in entries {
            let MemoryValue::Json(ref v) = e.value else {
                continue;
            };
            if let Some(cutoff) = before_ms {
                let ts = v.get("ts_ms").and_then(|t| t.as_u64()).unwrap_or(u64::MAX);
                if ts >= cutoff {
                    continue;
                }
            }
            if self
                .mem
                .delete(&self.owner, &Scope::Global, &e.key, None)
                .is_ok()
            {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

fn top_counts(
    map: std::collections::BTreeMap<String, u64>,
    top_n: usize,
) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if top_n > 0 {
        v.truncate(top_n);
    }
    v
}

/// Phase 6.5a dispatcher hook — IPC call 한 건을 audit log 에 기록한다.
/// `record_ipc_call` (telemetry) 와 짝을 이루며 dispatcher 경로의 모든 진입점에서
/// 호출된다. `with_store` 가 `None` 인 환경(테스트)에서는 silent skip.
pub fn record(
    caller: &CallerContext,
    method: &str,
    decision: AuditDecision,
    reason: Option<&str>,
    workspace_id: Option<u32>,
    seq: u64,
) {
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let record = AuditRecord {
        ts_ms,
        seq,
        caller_kind: AuditCallerKind::from_caller(caller),
        caller_id: caller.agent_id().as_str().to_string(),
        method: method.to_string(),
        decision,
        reason: reason.map(|s| s.to_string()),
        workspace_id,
    };
    let result = tasty_memory::with_store(|mem| {
        let mut store = AuditStore::new(mem, tasty_memory::HOST_OWNER);
        store.append(&record)
    });
    match result {
        Some(Ok(())) => {}
        Some(Err(e)) => tracing::warn!("audit: append failed: {e}"),
        None => {} // memory store 미초기화 환경 — 무시.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh() -> (TempDir, MemoryStore) {
        let td = tempfile::tempdir().unwrap();
        let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
        (td, mem)
    }

    fn rec(ts: u64, seq: u64, kind: AuditCallerKind, caller: &str, method: &str, dec: AuditDecision) -> AuditRecord {
        AuditRecord {
            ts_ms: ts,
            seq,
            caller_kind: kind,
            caller_id: caller.into(),
            method: method.into(),
            decision: dec,
            reason: None,
            workspace_id: None,
        }
    }

    #[test]
    fn append_and_list_in_order() {
        let (_td, mut mem) = fresh();
        let mut store = AuditStore::new(&mut mem, "_host");
        store.append(&rec(2_000, 0, AuditCallerKind::Agent, "child:1", "surface.list", AuditDecision::Allow)).unwrap();
        store.append(&rec(1_000, 0, AuditCallerKind::Agent, "child:1", "memory.put", AuditDecision::Deny)).unwrap();
        store.append(&rec(1_000, 1, AuditCallerKind::Agent, "child:1", "memory.put", AuditDecision::Allow)).unwrap();
        let all = store.list(0, 10_000).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].ts_ms, 1_000);
        assert_eq!(all[0].seq, 0);
        assert_eq!(all[2].ts_ms, 2_000);
    }

    #[test]
    fn retention_evicts_old() {
        let (_td, mut mem) = fresh();
        let mut store = AuditStore::new(&mut mem, "_host");
        store.append(&rec(1_000, 0, AuditCallerKind::Agent, "a", "x.y", AuditDecision::Allow)).unwrap();
        store.append(&rec(50_000, 0, AuditCallerKind::Agent, "a", "x.y", AuditDecision::Allow)).unwrap();
        // retention=10000, now=51000 → cutoff=41000. ts=1000 (오래됨) 만 evict.
        let alive = store.list(10_000, 51_000).unwrap();
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].ts_ms, 50_000);
    }

    #[test]
    fn query_filters_by_caller_method_decision_time() {
        let (_td, mut mem) = fresh();
        let mut store = AuditStore::new(&mut mem, "_host");
        store.append(&rec(1, 0, AuditCallerKind::Agent, "a", "memory.put", AuditDecision::Allow)).unwrap();
        store.append(&rec(2, 0, AuditCallerKind::Agent, "a", "memory.get", AuditDecision::Allow)).unwrap();
        store.append(&rec(3, 0, AuditCallerKind::Agent, "b", "memory.put", AuditDecision::Deny)).unwrap();
        store.append(&rec(4, 0, AuditCallerKind::Plugin, "p", "memory.put", AuditDecision::Allow)).unwrap();

        // caller_id="a" 만.
        let q = AuditQuery { caller_id: Some("a".into()), ..Default::default() };
        assert_eq!(store.query(&q, 0, 100).unwrap().len(), 2);

        // method prefix "memory." (모두 매칭).
        let q = AuditQuery { method_prefix: Some("memory.".into()), ..Default::default() };
        assert_eq!(store.query(&q, 0, 100).unwrap().len(), 4);

        // method prefix "memory.put" 만.
        let q = AuditQuery { method_prefix: Some("memory.put".into()), ..Default::default() };
        assert_eq!(store.query(&q, 0, 100).unwrap().len(), 3);

        // decision=Deny 만.
        let q = AuditQuery { decision: Some(AuditDecision::Deny), ..Default::default() };
        let denies = store.query(&q, 0, 100).unwrap();
        assert_eq!(denies.len(), 1);
        assert_eq!(denies[0].caller_id, "b");

        // since=3 (ts 3,4 만).
        let q = AuditQuery { since_ms: Some(3), ..Default::default() };
        assert_eq!(store.query(&q, 0, 100).unwrap().len(), 2);

        // limit=2.
        let q = AuditQuery { limit: Some(2), ..Default::default() };
        assert_eq!(store.query(&q, 0, 100).unwrap().len(), 2);
    }

    #[test]
    fn summary_buckets_by_caller_and_method() {
        let (_td, mut mem) = fresh();
        let mut store = AuditStore::new(&mut mem, "_host");
        for (i, (caller, method, dec)) in [
            ("a", "memory.put", AuditDecision::Allow),
            ("a", "memory.put", AuditDecision::Allow),
            ("a", "memory.get", AuditDecision::Deny),
            ("b", "surface.list", AuditDecision::Allow),
        ].iter().enumerate() {
            store.append(&rec(i as u64, 0, AuditCallerKind::Agent, caller, method, *dec)).unwrap();
        }
        let s = store.summary(&AuditQuery::default(), 0, 1_000_000, 10).unwrap();
        assert_eq!(s.total, 4);
        assert_eq!(s.allow, 3);
        assert_eq!(s.deny, 1);
        // by_caller: a=3, b=1.
        assert_eq!(s.by_caller, vec![("a".into(), 3), ("b".into(), 1)]);
        // by_method: memory.put=2, memory.get=1, surface.list=1 (tie 는 알파벳).
        assert_eq!(s.by_method[0], ("memory.put".into(), 2));
    }

    #[test]
    fn follow_returns_only_new_records_since_cursor() {
        let (_td, mut mem) = fresh();
        let mut store = AuditStore::new(&mut mem, "_host");
        for (ts, seq) in [(10, 0), (10, 1), (20, 0), (30, 0)] {
            store.append(&rec(ts, seq, AuditCallerKind::Agent, "a", "x.y", AuditDecision::Allow)).unwrap();
        }
        // 초기 호출: cursor 없음 → 빈 + latest 커서.
        let (recs, next_ts, next_seq) = store.follow(&AuditQuery::default(), None, None, 0, 1_000, None).unwrap();
        assert!(recs.is_empty());
        assert_eq!((next_ts, next_seq), (30, 0));

        // 새 record 가 들어옴.
        store.append(&rec(40, 0, AuditCallerKind::Agent, "a", "x.y", AuditDecision::Allow)).unwrap();
        store.append(&rec(40, 1, AuditCallerKind::Agent, "a", "x.y", AuditDecision::Allow)).unwrap();

        // 직전 커서 (30, 0) 으로 다시 호출 → 40,0 과 40,1 만.
        let (recs, next_ts, next_seq) = store.follow(&AuditQuery::default(), Some(30), Some(0), 0, 1_000, None).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!((recs[0].ts_ms, recs[0].seq), (40, 0));
        assert_eq!((recs[1].ts_ms, recs[1].seq), (40, 1));
        assert_eq!((next_ts, next_seq), (40, 1));

        // 또 호출 → 새 게 없으면 빈 + 커서 그대로.
        let (recs, next_ts, next_seq) = store.follow(&AuditQuery::default(), Some(40), Some(1), 0, 1_000, None).unwrap();
        assert!(recs.is_empty());
        assert_eq!((next_ts, next_seq), (40, 1));
    }

    #[test]
    fn follow_respects_filter_and_limit() {
        let (_td, mut mem) = fresh();
        let mut store = AuditStore::new(&mut mem, "_host");
        store.append(&rec(10, 0, AuditCallerKind::Agent, "a", "memory.put", AuditDecision::Allow)).unwrap();
        store.append(&rec(11, 0, AuditCallerKind::Agent, "b", "surface.list", AuditDecision::Deny)).unwrap();
        store.append(&rec(12, 0, AuditCallerKind::Agent, "a", "memory.get", AuditDecision::Allow)).unwrap();

        // 필터 caller=a, cursor=(0,0).
        let q = AuditQuery { caller_id: Some("a".into()), ..Default::default() };
        let (recs, _, _) = store.follow(&q, Some(0), Some(0), 0, 1_000, None).unwrap();
        assert_eq!(recs.len(), 2);

        // limit=1.
        let (recs, next_ts, next_seq) = store.follow(&q, Some(0), Some(0), 0, 1_000, Some(1)).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!((next_ts, next_seq), (10, 0)); // 첫 매칭만.
    }

    #[test]
    fn clear_all_or_before() {
        let (_td, mut mem) = fresh();
        let mut store = AuditStore::new(&mut mem, "_host");
        for ts in [1, 10, 100, 1000] {
            store.append(&rec(ts, 0, AuditCallerKind::Agent, "a", "x.y", AuditDecision::Allow)).unwrap();
        }
        // before_ms=50 → ts<50 인 1,10 만 삭제.
        let removed = store.clear(Some(50)).unwrap();
        assert_eq!(removed, 2);
        let remain = store.list(0, 10_000).unwrap();
        assert_eq!(remain.len(), 2);
        // 전체 삭제.
        let removed = store.clear(None).unwrap();
        assert_eq!(removed, 2);
        assert!(store.list(0, 10_000).unwrap().is_empty());
    }
}
