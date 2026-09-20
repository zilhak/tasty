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

/// `journal_mode` 로 요청하는 값.
const JOURNAL_MODE: &str = "WAL";

/// in-memory DB 가 `journal_mode=WAL` 요청에 대해 실제로 갖는 값.
///
/// SQLite 는 in-memory DB 에 WAL 을 적용할 수 없고(WAL 은 파일 두 개를 쓴다) 요청을
/// **조용히 거절한다** — `pragma_update` 는 `Ok(())` 를 내고 값만 `memory` 로 남는다
/// (실측 2026-09-20, rusqlite 0.32.1). 그래서 이 값은 실패가 아니라 그 모드의 정상
/// 결과이고, 경고 대상에서 뺀다.
const IN_MEMORY_JOURNAL_MODE: &str = "memory";

/// 연결에 공용 pragma 를 세우고 **실제로 적용됐는지** 본다.
///
/// 실패해도 `Err` 를 올리지 않는다 — 호출자는 둘 다 DB 를 여는 중이고, pragma 가
/// 안 서는 것은 열기 자체의 실패와 다른 축이다(열린 DB 는 그대로 쓸 수 있다).
/// 대신 **조용하지 않게** 한다: 어긋난 자리마다 `tracing::warn!` 이 나간다.
///
/// ## 반환값 검사만으로는 부족하다
///
/// `journal_mode` 는 요청이 거절돼도 `pragma_update` 가 `Ok(())` 를 낸다. 실측
/// (2026-09-20): in-memory DB 에 `WAL` 을 요청하면 반환은 `Ok(())` 이고 실제값은
/// `memory` 다. 즉 **요청값과 적용값은 다른 축**이고, 소스에 `"WAL"` 이라고 적혀
/// 있다는 것을 runtime 보장으로 쓰면 안 된다. 그래서 이 pragma 만 되읽어 대조한다.
///
/// `synchronous` · `foreign_keys` 는 거절되는 갈래가 없어 반환값이 그대로 답이다.
pub fn apply_connection_pragmas(conn: &Connection, path: &Path) {
    // 네 pragma 의 순서는 이 함수가 생기기 전 두 사본이 쓰던 것 그대로다.
    if let Err(e) = conn.pragma_update(None, "journal_mode", JOURNAL_MODE) {
        tracing::warn!(
            "{}: failed to set journal_mode={JOURNAL_MODE}: {e}",
            path.display()
        );
    }
    for (name, value) in [("synchronous", "NORMAL"), ("foreign_keys", "ON")] {
        if let Err(e) = conn.pragma_update(None, name, value) {
            tracing::warn!("{}: failed to set {name}={value}: {e}", path.display());
        }
    }
    // 이 pragma 가 빠지면 증상이 "조금 느려짐" 이 아니라 WAL 고착
    // (`WAL_SIZE_LIMIT_BYTES` doc)이라, 조용히 없는 것과 조용히 실패한 것을
    // 구별할 수 없으면 같은 조사를 처음부터 다시 하게 된다.
    if let Err(e) = conn.pragma_update(None, "journal_size_limit", WAL_SIZE_LIMIT_BYTES) {
        tracing::warn!(
            "{}: failed to set journal_size_limit; the WAL file can grow without bound: {e}",
            path.display()
        );
    }
    // 위 네 줄이 전부 Ok 여도 journal_mode 는 안 섰을 수 있다 — 아래 doc 참조.
    match effective_journal_mode(conn) {
        Ok(mode) if journal_mode_is_expected(&mode) => {}
        Ok(mode) => tracing::warn!(
            "{}: journal_mode is {mode}, not the requested {JOURNAL_MODE}; \
             concurrent readers and writers will contend more than the design assumes",
            path.display()
        ),
        Err(e) => tracing::warn!(
            "{}: cannot read back journal_mode, so the requested {JOURNAL_MODE} is unconfirmed: {e}",
            path.display()
        ),
    }
}

/// 지금 연결의 `journal_mode` 실제값 (소문자로 온다).
pub fn effective_journal_mode(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
}

/// 이 값이 정상 결과인가. `WAL` 이거나, in-memory DB 의 `memory` 면 정상이다.
fn journal_mode_is_expected(mode: &str) -> bool {
    mode.eq_ignore_ascii_case(JOURNAL_MODE) || mode.eq_ignore_ascii_case(IN_MEMORY_JOURNAL_MODE)
}
