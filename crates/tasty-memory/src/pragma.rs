//! 두 SQLite DB 가 공유하는 연결 pragma 집합.
//!
//! `memory.db`([`crate::MemoryStore`])와 `state.db`(본 바이너리의 `Db`)는 **서로
//! 다른 `prepare`** 를 쓰지만 연결마다 세우는 pragma 는 같아야 한다. 한때 두 자리가
//! 같은 네 줄을 각자 박아 두었고, 그래서 한쪽만 고치면 다른 쪽이 조용히 뒤처졌다 —
//! 실제로 `journal_size_limit` 은 한쪽에만 먼저 들어갔다. 자유도가 없는 사본이므로
//! 사본을 두지 않고 이 함수 하나를 두 곳이 부른다.
//!
//! 적용 결과를 어떻게 관측하는지는 [`apply_connection_pragmas`] 의 doc 를 본다.

use std::path::Path;

use rusqlite::Connection;

use crate::WAL_SIZE_LIMIT_BYTES;

/// 연결에 공용 pragma 를 세운다.
///
/// 실패해도 `Err` 를 올리지 않는다 — 호출자는 둘 다 DB 를 여는 중이고, pragma 가
/// 안 서는 것은 열기 자체의 실패와 다른 축이다(열린 DB 는 그대로 쓸 수 있다).
/// 대신 **조용하지 않게** 한다: 각 실패가 `tracing::warn!` 으로 나간다.
pub fn apply_connection_pragmas(conn: &Connection, path: &Path) {
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    conn.pragma_update(None, "synchronous", "NORMAL").ok();
    conn.pragma_update(None, "foreign_keys", "ON").ok();
    // 이 pragma 가 빠지면 증상이 "조금 느려짐" 이 아니라 WAL 고착
    // (`WAL_SIZE_LIMIT_BYTES` doc)이라, 조용히 없는 것과 조용히 실패한 것을
    // 구별할 수 없으면 같은 조사를 처음부터 다시 하게 된다.
    if let Err(e) = conn.pragma_update(None, "journal_size_limit", WAL_SIZE_LIMIT_BYTES) {
        tracing::warn!(
            "{}: failed to set journal_size_limit; the WAL file can grow without bound: {e}",
            path.display()
        );
    }
}
