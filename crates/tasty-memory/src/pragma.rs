//! memory.db와 state.db에 같은 연결 pragma를 적용하고 실제값을 다시 읽는다.

use std::path::Path;

use rusqlite::Connection;

use crate::WAL_SIZE_LIMIT_BYTES;

/// `journal_mode` 로 요청하는 값.
const JOURNAL_MODE: &str = "WAL";

/// in-memory DB는 WAL 요청이 성공해도 journal_mode가 memory로 남는다. 이 모드의 정상값이다.
const IN_MEMORY_JOURNAL_MODE: &str = "memory";

/// `synchronous` 로 요청하는 값. 되읽으면 정수(`0`~`3`)로 오므로 [`synchronous_name`]
/// 으로 이름을 되찾아 대조한다.
const SYNCHRONOUS: &str = "NORMAL";

/// `foreign_keys` 로 요청하는 값. 되읽으면 `0`/`1` 로 온다.
const FOREIGN_KEYS: &str = "ON";

/// pragma 요청값과 실제값. 설정·조회에 오류가 없고 모드별 허용값과 맞을 때 took=true다.
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
    /// 요청한 설정이 이 DB 모드에 맞게 적용됐는지.
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
/// 파일 DB와 in-memory DB의 허용값을 서로 바꾸어 적용하지 않는다.
///
/// 하나라도 took=false면 degraded=true다. DB 열기를 중단하지 않고 경고와 상태를 제공한다.
/// 이는 DB 자체를 열지 못한 경우의 종료·임시 저장소 정책과 별개다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPragmas {
    /// 이 연결이 in-memory DB 인가. 허용 결과표의 열을 고른다.
    pub in_memory: bool,
    /// pragma 마다 한 줄. 순서는 적용 순서 그대로다.
    pub readings: Vec<PragmaReading>,
}

impl AppliedPragmas {
    /// 적용되지 않은 설정이 있는지 확인한다.
    pub fn degraded(&self) -> bool {
        self.readings.iter().any(|r| !r.took)
    }

    /// 이름으로 한 줄을 찾는다.
    pub fn get(&self, name: &str) -> Option<&PragmaReading> {
        self.readings.iter().find(|r| r.name == name)
    }
}

/// 공용 pragma를 적용하고 실제값을 다시 읽는다. 반환 성공만으로 적용을 보장하지 않기 때문이다.
/// 실패나 불일치는 경고와 AppliedPragmas에 남기며 DB 열기 오류로 전파하지 않는다.
/// 호스트는 system.pressure의 db_pragmas로 결과를 제공한다.
pub fn apply_connection_pragmas(conn: &Connection, path: &Path) -> AppliedPragmas {
    let in_memory = is_in_memory(conn);
    let size_limit = WAL_SIZE_LIMIT_BYTES.to_string();
    let set_journal = set_pragma(conn, path, "journal_mode", JOURNAL_MODE);
    let set_sync = set_pragma(conn, path, "synchronous", SYNCHRONOUS);
    let set_fk = set_pragma(conn, path, "foreign_keys", FOREIGN_KEYS);
    let set_limit = set_wal_size_limit(conn, path);
    // 설정 호출이 성공해도 실제값이 다를 수 있어 다시 읽는다.
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

/// 설정 실패를 경고하고 오류 문구를 반환한다.
fn set_pragma(conn: &Connection, path: &Path, name: &str, value: &str) -> Option<String> {
    match conn.pragma_update(None, name, value) {
        Ok(()) => None,
        Err(e) => {
            tracing::warn!("{}: failed to set {name}={value}: {e}", path.display());
            Some(e.to_string())
        }
    }
}

/// WAL 재사용 시 파일을 줄일 상한을 설정한다. 실패를 경고해 설정 미적용을 알린다.
fn set_wal_size_limit(conn: &Connection, path: &Path) -> Option<String> {
    match conn.pragma_update(None, "journal_size_limit", WAL_SIZE_LIMIT_BYTES) {
        Ok(()) => None,
        Err(e) => {
            tracing::warn!(
                "{}: failed to set journal_size_limit; the WAL file will keep its largest size after checkpoints: {e}",
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
