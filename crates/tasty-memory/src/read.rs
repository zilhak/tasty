//! 공개 조회 메서드. 실행 단위 안에서 저장 실패가 나면 단위를 중단시키고(`batch` 모듈),
//! 단위 밖에서는 조회 본문(`*_unrecorded`)의 결과를 그대로 돌려준다.

use crate::{ListOpts, MemoryEntry, MemoryStats, MemoryStore, Result, Scope};

impl MemoryStore {
    /// 단건 조회. 만료된 키는 `None` 취급. owner 필드 포함 (regular 의미 보존).
    pub fn get(&self, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        self.record_read(self.get_unrecorded(scope, key))
    }

    /// 스코프 내 키 리스트. 만료 제외, prefix/since/until/offset/limit 옵션.
    /// 응답 entry 에 owner 포함. `key ASC` 정렬.
    pub fn list(&self, scope: &Scope, opts: &ListOpts) -> Result<Vec<MemoryEntry>> {
        self.record_read(self.list_unrecorded(scope, opts))
    }

    /// 스코프 내 키 갯수 (만료 제외, prefix 옵션).
    pub fn count(&self, scope: &Scope, prefix: Option<&str>) -> Result<u64> {
        self.record_read(self.count_unrecorded(scope, prefix))
    }

    /// 사용 중인 스코프 목록.
    pub fn scopes(&self) -> Result<Vec<String>> {
        self.record_read(self.scopes_unrecorded())
    }

    /// 통계. 스코프별 (또는 전체) entry count + 총 byte size.
    pub fn stats(&self, scope: Option<&Scope>) -> Result<MemoryStats> {
        self.record_read(self.stats_unrecorded(scope))
    }

    /// 만료되지 않은 Regular 항목을 내보낸다. scope=None이면 전체이며 Secret는 포함하지 않는다.
    pub fn export_regular(&self, scope: Option<&Scope>) -> Result<Vec<MemoryEntry>> {
        self.record_read(self.export_regular_unrecorded(scope))
    }

    /// Secret 조회. 응답의 owner 필드는 None이다.
    pub fn get_secret(&self, owner: &str, scope: &Scope, key: &str) -> Result<Option<MemoryEntry>> {
        self.record_read(self.get_secret_unrecorded(owner, scope, key))
    }

    pub fn list_secret(
        &self,
        owner: &str,
        scope: &Scope,
        opts: &ListOpts,
    ) -> Result<Vec<MemoryEntry>> {
        self.record_read(self.list_secret_unrecorded(owner, scope, opts))
    }

    pub fn count_secret(&self, owner: &str, scope: &Scope, prefix: Option<&str>) -> Result<u64> {
        self.record_read(self.count_secret_unrecorded(owner, scope, prefix))
    }

    pub fn scopes_secret(&self, owner: &str) -> Result<Vec<String>> {
        self.record_read(self.scopes_secret_unrecorded(owner))
    }

    pub fn stats_secret(&self, owner: &str, scope: Option<&Scope>) -> Result<MemoryStats> {
        self.record_read(self.stats_secret_unrecorded(owner, scope))
    }
}
