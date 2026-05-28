//! `MemoryStorage` trait 의 `MemoryStore` impl — 기존 inherent method 들 delegation.

use crate::port::MemoryStorage;
use crate::{
    ImportStats, ListOpts, MemoryChange, MemoryConfig, MemoryEntry, MemoryStats, MemoryStore,
    MemoryValue, PurgeStats, PutOpts, Result, Scope,
};

impl MemoryStorage for MemoryStore {
    fn config(&self) -> &MemoryConfig {
        MemoryStore::config(self)
    }

    // ─── Regular ───
    fn put(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64> {
        MemoryStore::put(self, owner, scope, key, value, opts)
    }

    fn get(&self, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        MemoryStore::get(self, scope, key)
    }

    fn exists(&self, scope: &Scope, key: &str) -> Result<bool> {
        MemoryStore::exists(self, scope, key)
    }

    fn delete(&mut self, owner: &str, scope: &Scope, key: &str, cas: Option<u64>) -> Result<()> {
        MemoryStore::delete(self, owner, scope, key, cas)
    }

    fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        MemoryStore::list(self, scope, opts)
    }

    fn count(&self, scope: &Scope, prefix: Option<&str>) -> Result<u64> {
        MemoryStore::count(self, scope, prefix)
    }

    fn scopes(&self) -> Result<Vec<String>> {
        MemoryStore::scopes(self)
    }

    fn stats(&self, scope: Option<&Scope>) -> Result<MemoryStats> {
        MemoryStore::stats(self, scope)
    }

    fn query(
        &self,
        scope: &Scope,
        path: &str,
        expected: &serde_json::Value,
        opts: &ListOpts,
    ) -> Result<Vec<MemoryEntry>> {
        MemoryStore::query(self, scope, path, expected, opts)
    }

    fn export_regular(&self, scope: Option<&Scope>) -> Result<Vec<MemoryEntry>> {
        MemoryStore::export_regular(self, scope)
    }

    fn import_regular(
        &mut self,
        caller_owner: &str,
        entries: &[MemoryEntry],
        replace: bool,
    ) -> Result<ImportStats> {
        MemoryStore::import_regular(self, caller_owner, entries, replace)
    }

    // ─── Secret ───
    fn put_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        value: &MemoryValue,
        opts: &PutOpts,
    ) -> Result<u64> {
        MemoryStore::put_secret(self, owner, scope, key, value, opts)
    }

    fn get_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        MemoryStore::get_secret(self, owner, scope, key)
    }

    fn exists_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<bool> {
        MemoryStore::exists_secret(self, owner, scope, key)
    }

    fn delete_secret(
        &mut self,
        owner: &str,
        scope: &Scope,
        key: &str,
        cas: Option<u64>,
    ) -> Result<()> {
        MemoryStore::delete_secret(self, owner, scope, key, cas)
    }

    fn list_secret(&self, owner: &str, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        MemoryStore::list_secret(self, owner, scope, opts)
    }

    fn count_secret(&self, owner: &str, scope: &Scope, prefix: Option<&str>) -> Result<u64> {
        MemoryStore::count_secret(self, owner, scope, prefix)
    }

    fn scopes_secret(&self, owner: &str) -> Result<Vec<String>> {
        MemoryStore::scopes_secret(self, owner)
    }

    fn stats_secret(&self, owner: &str, scope: Option<&Scope>) -> Result<MemoryStats> {
        MemoryStore::stats_secret(self, owner, scope)
    }

    // ─── Maintenance ───
    fn purge_expired(&mut self) -> Result<PurgeStats> {
        MemoryStore::purge_expired(self)
    }

    fn purge_scope(&mut self, scope: &Scope) -> Result<PurgeStats> {
        MemoryStore::purge_scope(self, scope)
    }

    fn take_pending_changes(&mut self) -> Vec<MemoryChange> {
        MemoryStore::take_pending_changes(self)
    }
}
