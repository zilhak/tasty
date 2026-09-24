//! SQLite 대신 사용하는 맵 기반 시험 저장소. 구현하지 않은 메서드는 패닉한다.

use std::collections::{BTreeMap, HashMap};

use crate::port::MemoryStorage;
use crate::{
    ImportStats, ListOpts, MemoryChange, MemoryConfig, MemoryEntry, MemoryStats, MemoryValue,
    PurgeStats, PutOpts, Result, Scope,
};

#[derive(Debug)]
// 이유: test 전용 mock(`MemoryStorage` impl). `secret` 필드는 아직 미구현 메서드용 — test helper 유지.
#[allow(dead_code)]
pub struct InMemoryStorage {
    config: MemoryConfig,
    /// list가 키 순서를 유지하도록 (scope, key) 순으로 저장한다.
    regular: BTreeMap<(String, String), MemoryEntry>,
    secret: HashMap<(String, String, String), MemoryEntry>, // (owner, scope, key) → entry
    pending: Vec<MemoryChange>,
    /// 결과가 같아도 중복 호출을 검출할 수 있도록 purge_scope 호출을 순서대로 기록한다.
    purge_scope_calls: Vec<String>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::with_config(MemoryConfig::default())
    }

    pub fn with_config(config: MemoryConfig) -> Self {
        Self {
            config,
            regular: BTreeMap::new(),
            secret: HashMap::new(),
            pending: Vec::new(),
            purge_scope_calls: Vec::new(),
        }
    }

    /// 삭제된 행 수가 아닌 해당 scope의 purge_scope 호출 수.
    pub fn purge_scope_call_count(&self, scope: &Scope) -> usize {
        let token = scope.as_token();
        self.purge_scope_calls
            .iter()
            .filter(|t| **t == token)
            .count()
    }

    /// `purge_scope` 호출 이력 전체(scope token, 호출 순서).
    pub fn purge_scope_calls(&self) -> &[String] {
        &self.purge_scope_calls
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStorage for InMemoryStorage {
    fn config(&self) -> &MemoryConfig {
        &self.config
    }

    fn put(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        _opts: &PutOpts,
    ) -> Result<u64> {
        let token = scope.as_token();
        let entry = self
            .regular
            .entry((token.clone(), key.to_string()))
            .or_insert_with(|| MemoryEntry {
                scope: token,
                key: key.to_string(),
                value: value.clone(),
                created_at: 0,
                updated_at: 0,
                expires_at: None,
                version: 0,
                owner: Some(owner.to_string()),
            });
        entry.value = value.clone();
        entry.owner = Some(owner.to_string());
        entry.version += 1;
        Ok(entry.version)
    }

    fn get(&self, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        Ok(self
            .regular
            .get(&(scope.as_token(), key.to_string()))
            .cloned())
    }

    fn exists(&self, scope: &Scope, key: &str) -> Result<bool> {
        Ok(self
            .regular
            .contains_key(&(scope.as_token(), key.to_string())))
    }

    fn delete(&mut self, _owner: &str, scope: &Scope, key: &str, _cas: Option<u64>) -> Result<()> {
        self.regular.remove(&(scope.as_token(), key.to_string()));
        Ok(())
    }

    /// 운영 저장소처럼 키 오름차순이다(`regular` 가 (scope, key) 순으로 순회한다).
    fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        let token = scope.as_token();
        Ok(self
            .regular
            .values()
            .filter(|e| e.scope == token)
            .filter(|e| match &opts.prefix {
                Some(p) => e.key.starts_with(p.as_str()),
                None => true,
            })
            .cloned()
            .collect())
    }

    fn count(&self, scope: &Scope, _prefix: Option<&str>) -> Result<u64> {
        let token = scope.as_token();
        Ok(self.regular.values().filter(|e| e.scope == token).count() as u64)
    }

    fn scopes(&self) -> Result<Vec<String>> {
        let mut out: Vec<String> = self
            .regular
            .keys()
            .map(|(scope, _)| scope.clone())
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn stats(&self, _scope: Option<&Scope>) -> Result<MemoryStats> {
        unimplemented!("InMemoryStorage::stats — fill in when first test needs it")
    }

    fn query(
        &self,
        _scope: &Scope,
        _path: &str,
        _expected: &serde_json::Value,
        _opts: &ListOpts,
    ) -> Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    fn export_regular(&self, _scope: Option<&Scope>) -> Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    fn import_regular(
        &mut self,
        _caller_owner: &str,
        _entries: &[MemoryEntry],
        _replace: bool,
    ) -> Result<ImportStats> {
        unimplemented!("InMemoryStorage::import_regular — fill in when first test needs it")
    }

    fn put_secret(
        &mut self,
        _owner: &str,
        _scope: &Scope,
        _key: &str,
        _value: &MemoryValue,
        _opts: &PutOpts,
    ) -> Result<u64> {
        Ok(1)
    }

    fn get_secret(&self, _owner: &str, _scope: &Scope, _key: &str) -> Result<Option<MemoryEntry>> {
        Ok(None)
    }

    fn exists_secret(&self, _owner: &str, _scope: &Scope, _key: &str) -> Result<bool> {
        Ok(false)
    }

    fn delete_secret(
        &mut self,
        _owner: &str,
        _scope: &Scope,
        _key: &str,
        _cas: Option<u64>,
    ) -> Result<()> {
        Ok(())
    }

    fn list_secret(
        &self,
        _owner: &str,
        _scope: &Scope,
        _opts: &ListOpts,
    ) -> Result<Vec<MemoryEntry>> {
        Ok(Vec::new())
    }

    fn count_secret(&self, _owner: &str, _scope: &Scope, _prefix: Option<&str>) -> Result<u64> {
        Ok(0)
    }

    fn scopes_secret(&self, _owner: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn stats_secret(&self, _owner: &str, _scope: Option<&Scope>) -> Result<MemoryStats> {
        unimplemented!("InMemoryStorage::stats_secret — fill in when first test needs it")
    }

    fn prune_prefix_keep_recent(&mut self, prefix: &str, keep_recent: u64) -> Result<u64> {
        // 실제 store 와 같은 전제(키가 `prefix<zero-padded ts>...` 라 lexical =
        // chronological)로, 정렬 후 최근 N 개만 남긴다. scope 무관인 것도 동일하다.
        let mut keys: Vec<String> = self
            .regular
            .values()
            .filter(|e| e.key.starts_with(prefix))
            .map(|e| e.key.clone())
            .collect();
        keys.sort();
        let cut = keys.len().saturating_sub(keep_recent as usize);
        let doomed: Vec<String> = keys.into_iter().take(cut).collect();
        let n = doomed.len() as u64;
        self.regular.retain(|(_, k), _| !doomed.contains(k));
        Ok(n)
    }

    fn prune_prefix_older_than(&mut self, prefix: &str, cutoff_ms: u64) -> Result<u64> {
        let boundary = format!("{prefix}{cutoff_ms:013}");
        let before = self.regular.len();
        self.regular
            .retain(|(_, k), _| !(k.starts_with(prefix) && k.as_str() < boundary.as_str()));
        Ok((before - self.regular.len()) as u64)
    }

    fn purge_expired(&mut self) -> Result<PurgeStats> {
        // 본 in-memory mock 은 TTL/expiry 트래킹을 하지 않으므로 no-op 반환.
        Ok(PurgeStats {
            regular: 0,
            secret: 0,
        })
    }

    fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats> {
        let token = scope.as_token();
        self.purge_scope_calls.push(token.clone());
        let before = self.regular.len();
        self.regular.retain(|(s, _), _| s != &token);
        let removed = (before - self.regular.len()) as u64;
        Ok(PurgeStats {
            regular: removed,
            secret: 0,
        })
    }

    fn take_pending_changes(&mut self) -> Vec<MemoryChange> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;

    fn keys(entries: Vec<MemoryEntry>) -> Vec<String> {
        entries.into_iter().map(|e| e.key).collect()
    }

    /// 같은 입력에서 운영 저장소와 키 정렬 순서가 같은지 대조한다.
    #[test]
    fn list_returns_keys_in_the_same_order_as_the_real_store() {
        let inserted = ["b.2", "a.10", "c", "a.9", "a.1", "b.1"];
        let mut mock = InMemoryStorage::new();
        let mut real = MemoryStore::open_in_memory().unwrap();
        for k in inserted {
            let v = MemoryValue::Text(k.into());
            mock.put("_host", &Scope::Global, k, &v, &PutOpts::default())
                .unwrap();
            real.put("_host", &Scope::Global, k, &v, &PutOpts::default())
                .unwrap();
        }
        // 다른 scope 의 항목은 섞이지 않는다.
        mock.put(
            "_host",
            &Scope::Workspace(1),
            "a.0",
            &MemoryValue::Text("x".into()),
            &PutOpts::default(),
        )
        .unwrap();

        let opts = ListOpts::default();
        let got = keys(mock.list(&Scope::Global, &opts).unwrap());
        assert_eq!(got, keys(real.list(&Scope::Global, &opts).unwrap()));
        assert_eq!(got, vec!["a.1", "a.10", "a.9", "b.1", "b.2", "c"]);

        let prefixed = ListOpts {
            prefix: Some("a.".into()),
            ..Default::default()
        };
        assert_eq!(
            keys(mock.list(&Scope::Global, &prefixed).unwrap()),
            vec!["a.1", "a.10", "a.9"]
        );
    }
}
