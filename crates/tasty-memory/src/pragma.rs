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

/// `synchronous` 로 요청하는 값. 되읽으면 정수(`0`~`3`)로 오므로 [`synchronous_name`]
/// 으로 이름을 되찾아 대조한다.
const SYNCHRONOUS: &str = "NORMAL";

/// `foreign_keys` 로 요청하는 값. 되읽으면 `0`/`1` 로 온다.
const FOREIGN_KEYS: &str = "ON";

/// pragma 하나의 요청값과 **되읽은 실제값**.
///
/// `took` 이 이 값의 판정이다 — 설정·되읽기 어느 쪽도 오류가 없고, 실제값이 이 DB
/// 모드(파일 / in-memory)의 허용 결과일 때만 `true` 다. 허용 결과의 정의는
/// [`AppliedPragmas`] 의 doc 에 있다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PragmaReading {
    /// pragma 이름 (`journal_mode` 등).
    pub name: &'static str,
    /// 요청한 값. 소스에 적힌 그대로다.
    pub requested: String,
    /// 되읽은 실제값. 되읽기가 실패했으면 `None` — 그때 사유는 `error` 에 있다.
    pub effective: Option<String>,
    /// 설정 또는 되읽기가 낸 오류. 둘 다 났으면 설정 쪽이다.
    pub error: Option<String>,
    /// 이 pragma 가 요청대로 섰는가.
    pub took: bool,
}

/// 한 연결에 [`apply_connection_pragmas`] 를 건 결과.
///
/// ## 모드별 허용 결과
///
/// | pragma | 파일 DB | in-memory DB |
/// |---|---|---|
/// | `journal_mode` | `wal` | `memory` (SQLite 가 WAL 을 못 쓴다 — [`IN_MEMORY_JOURNAL_MODE`]) |
/// | `synchronous` | `NORMAL` | `NORMAL` |
/// | `foreign_keys` | `ON` | `ON` |
/// | `journal_size_limit` | 요청값 그대로 | 요청값 그대로 |
///
/// 파일 DB 가 `memory` 로 서거나 in-memory DB 가 `wal` 이라고 답하면 **허용 결과가
/// 아니다** — 두 모드의 정상 결과를 서로의 것으로 봐 주면, 파일 DB 에서 WAL 이 조용히
/// 안 선 것을 "in-memory 의 정상값" 으로 삼켜 버린다.
///
/// ## degraded 는 오류가 아니라 상태다
///
/// 하나라도 `took == false` 면 [`degraded`](Self::degraded) 가 `true` 다. 그래도 DB 는
/// 열린 채로 쓰인다 — 열기 자체의 실패(`DbInitError` 는 안내 후 종료, `MemoryInitError` 는
/// in-memory 대체 — `crate::InitFallback`)와 다른 축이고, pragma 가 안 선 DB 는 느리거나
/// 덜 내구적일 뿐 동작은 한다.
/// 그 선택의 근거는 `docs/adr/0376-a-database-that-opened-with-pragmas-that-did-not-take-is-degraded-not-fatal.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPragmas {
    /// 이 연결이 in-memory DB 인가. 허용 결과표의 열을 고른다.
    pub in_memory: bool,
    /// pragma 마다 한 줄. 순서는 적용 순서 그대로다.
    pub readings: Vec<PragmaReading>,
}

impl AppliedPragmas {
    /// 요청대로 안 선 pragma 가 하나라도 있는가.
    pub fn degraded(&self) -> bool {
        self.readings.iter().any(|r| !r.took)
    }

    /// 이름으로 한 줄을 찾는다.
    pub fn get(&self, name: &str) -> Option<&PragmaReading> {
        self.readings.iter().find(|r| r.name == name)
    }
}

/// 연결에 공용 pragma 를 세우고 **실제로 적용됐는지** 되읽어 값으로 돌려준다.
///
/// 실패해도 `Err` 를 올리지 않는다 — 호출자는 둘 다 DB 를 여는 중이고, pragma 가
/// 안 서는 것은 열기 자체의 실패와 다른 축이다(열린 DB 는 그대로 쓸 수 있다).
/// 대신 **조용하지 않게** 한다: 어긋난 자리마다 `tracing::warn!` 이 나가고, 결과가
/// [`AppliedPragmas`] 로 남아 호출자가 밖에 내보일 수 있다(`system.pressure` 의
/// `db_pragmas` 덩어리).
///
/// ## 반환값 검사만으로는 부족하다
///
/// `journal_mode` 는 요청이 거절돼도 `pragma_update` 가 `Ok(())` 를 낸다. 실측
/// (2026-09-20): in-memory DB 에 `WAL` 을 요청하면 반환은 `Ok(())` 이고 실제값은
/// `memory` 다. 즉 **요청값과 적용값은 다른 축**이고, 소스에 `"WAL"` 이라고 적혀
/// 있다는 것을 runtime 보장으로 쓰면 안 된다. 그래서 넷 다 되읽어 대조한다 —
/// `synchronous` · `foreign_keys` 는 거절 갈래가 알려져 있지 않지만, 되읽기가 싸고
/// 그 값이 밖에 나가는 답이므로 "반환값이 `Ok` 였다" 로 갈음하지 않는다.
pub fn apply_connection_pragmas(conn: &Connection, path: &Path) -> AppliedPragmas {
    let in_memory = is_in_memory(conn);
    let size_limit = WAL_SIZE_LIMIT_BYTES.to_string();
    // 네 pragma 의 순서는 이 함수가 생기기 전 두 사본이 쓰던 것 그대로다.
    let set_journal = set_pragma(conn, path, "journal_mode", JOURNAL_MODE);
    let set_sync = set_pragma(conn, path, "synchronous", SYNCHRONOUS);
    let set_fk = set_pragma(conn, path, "foreign_keys", FOREIGN_KEYS);
    let set_limit = set_wal_size_limit(conn, path);
    // 위 네 줄이 전부 Ok 여도 journal_mode 는 안 섰을 수 있다 — 이 함수의 doc 참조.
    let journal = confirm_journal_mode(conn, path, in_memory, set_journal);
    let readings = vec![
        journal,
        confirm(conn, path, "synchronous", SYNCHRONOUS, set_sync, |v| {
            synchronous_name(v).map(str::to_string)
        }),
        confirm(conn, path, "foreign_keys", FOREIGN_KEYS, set_fk, |v| {
            Some(if v == 0 { "OFF" } else { "ON" }.to_string())
        }),
        confirm(
            conn,
            path,
            "journal_size_limit",
            &size_limit,
            set_limit,
            |v| Some(v.to_string()),
        ),
    ];
    AppliedPragmas {
        in_memory,
        readings,
    }
}

/// in-memory DB 인가. SQLite 는 in-memory·임시 DB 의 파일 이름을 빈 문자열로 답한다.
fn is_in_memory(conn: &Connection) -> bool {
    conn.path().is_none_or(str::is_empty)
}

/// 값 하나를 세우고, 실패하면 그 자리를 이름으로 말한다. 오류 문장을 돌려준다.
fn set_pragma(conn: &Connection, path: &Path, name: &str, value: &str) -> Option<String> {
    match conn.pragma_update(None, name, value) {
        Ok(()) => None,
        Err(e) => {
            tracing::warn!("{}: failed to set {name}={value}: {e}", path.display());
            Some(e.to_string())
        }
    }
}

/// WAL 크기 상한. 이 pragma 가 빠지면 증상이 "조금 느려짐" 이 아니라 WAL 고착
/// (`WAL_SIZE_LIMIT_BYTES` doc)이라, 조용히 없는 것과 조용히 실패한 것을 구별할 수
/// 없으면 같은 조사를 처음부터 다시 하게 된다.
fn set_wal_size_limit(conn: &Connection, path: &Path) -> Option<String> {
    match conn.pragma_update(None, "journal_size_limit", WAL_SIZE_LIMIT_BYTES) {
        Ok(()) => None,
        Err(e) => {
            tracing::warn!(
                "{}: failed to set journal_size_limit; the WAL file can grow without bound: {e}",
                path.display()
            );
            Some(e.to_string())
        }
    }
}

/// 되읽어서 요청과 대조한다. 요청이 조용히 거절되는 갈래는 이것만 잡는다.
fn confirm_journal_mode(
    conn: &Connection,
    path: &Path,
    in_memory: bool,
    set_error: Option<String>,
) -> PragmaReading {
    let (effective, read_error) = match effective_journal_mode(conn) {
        Ok(mode) => (Some(mode), None),
        Err(e) => {
            tracing::warn!(
                "{}: cannot read back journal_mode, so the requested {JOURNAL_MODE} is unconfirmed: {e}",
                path.display()
            );
            (None, Some(e.to_string()))
        }
    };
    let expected = effective
        .as_deref()
        .is_some_and(|m| journal_mode_is_expected(m, in_memory));
    if let Some(mode) = effective.as_deref().filter(|_| !expected) {
        tracing::warn!(
            "{}: journal_mode is {mode}, not the requested {JOURNAL_MODE}; \
             concurrent readers and writers will contend more than the design assumes",
            path.display()
        );
    }
    let error = set_error.or(read_error);
    PragmaReading {
        name: "journal_mode",
        requested: JOURNAL_MODE.to_string(),
        effective,
        took: expected && error.is_none(),
        error,
    }
}

/// 정수로 되읽히는 pragma 하나를 이름 있는 값으로 바꿔 요청과 대조한다.
fn confirm(
    conn: &Connection,
    path: &Path,
    name: &'static str,
    requested: &str,
    set_error: Option<String>,
    render: impl Fn(i64) -> Option<String>,
) -> PragmaReading {
    let read = conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, i64>(0));
    let (effective, read_error) = match read {
        Ok(v) => (Some(render(v).unwrap_or_else(|| v.to_string())), None),
        Err(e) => {
            tracing::warn!(
                "{}: cannot read back {name}, so the requested {requested} is unconfirmed: {e}",
                path.display()
            );
            (None, Some(e.to_string()))
        }
    };
    let matches = effective
        .as_deref()
        .is_some_and(|v| v.eq_ignore_ascii_case(requested));
    if let Some(v) = effective.as_deref().filter(|_| !matches) {
        tracing::warn!(
            "{}: {name} is {v}, not the requested {requested}",
            path.display()
        );
    }
    let error = set_error.or(read_error);
    PragmaReading {
        name,
        requested: requested.to_string(),
        effective,
        took: matches && error.is_none(),
        error,
    }
}

/// `PRAGMA synchronous` 가 돌려주는 정수의 이름(sqlite.org/pragma.html#pragma_synchronous).
fn synchronous_name(level: i64) -> Option<&'static str> {
    match level {
        0 => Some("OFF"),
        1 => Some("NORMAL"),
        2 => Some("FULL"),
        3 => Some("EXTRA"),
        _ => None,
    }
}

/// 지금 연결의 `journal_mode` 실제값 (소문자로 온다).
pub fn effective_journal_mode(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
}

/// 이 값이 이 모드의 정상 결과인가 — 파일 DB 는 `WAL`, in-memory DB 는 `memory` 만.
fn journal_mode_is_expected(mode: &str, in_memory: bool) -> bool {
    let expected = if in_memory {
        IN_MEMORY_JOURNAL_MODE
    } else {
        JOURNAL_MODE
    };
    mode.eq_ignore_ascii_case(expected)
}
