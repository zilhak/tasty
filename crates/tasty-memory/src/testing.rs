//! `testing::InMemoryStorage` — HashMap 기반 mock. test 시 SQLite 우회.
//!
//! 현재는 stub — Phase D.3.C 의 test 작성 시 필요한 메서드 부터 채운다.
//! 미구현 메서드는 `unimplemented!()` — 호출 시 panic 으로 알려준다.

use std::collections::HashMap;

use crate::port::MemoryStorage;
use crate::{
    ImportStats, ListOpts, MemoryChange, MemoryConfig, MemoryEntry, MemoryStats, MemoryValue,
    PurgeStats, PutOpts, Result, Scope,
};

#[derive(Debug)]
#[allow(dead_code)]
pub struct InMemoryStorage {
    config: MemoryConfig,
    regular: HashMap<(String, String), MemoryEntry>,
    secret: HashMap<(String, String, String), MemoryEntry>, // (owner, scope, key) → entry
    pending: Vec<MemoryChange>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::with_config(MemoryConfig::default())
    }

    pub fn with_config(config: MemoryConfig) -> Self {
        Self {
            config,
            regular: HashMap::new(),
            secret: HashMap::new(),
            pending: Vec::new(),
        }
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

    fn list(&self, scope: &Scope, _opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        let token = scope.as_token();
        Ok(self
            .regular
            .values()
            .filter(|e| e.scope == token)
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

    fn purge_expired(&mut self) -> Result<PurgeStats> {
        // 본 in-memory mock 은 TTL/expiry 트래킹을 하지 않으므로 no-op 반환.
        Ok(PurgeStats {
            regular: 0,
            secret: 0,
        })
    }

    fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats> {
        let token = scope.as_token();
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
