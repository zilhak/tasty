//! SQLite 기반 영속 상태 저장소 (`~/.tasty/state.db`).
//!
//! 대상 도메인:
//! - 북마크 (explorer)
//! - 최근 파일 (markdown / html)
//! - 클립보드 히스토리 스키마 자리 (실제 기록 연결은 별도 단계)
//!
//! 사용자 설정(config.toml)이나 쉘 스크립트(bashrc)는 이 저장소에
//! 들어가지 않는다 — 텍스트 편집/버전관리 대상은 그대로 파일 유지.
//!
//! 접근 규칙:
//! - 메인 프로세스 단독 접근. 자식 CLI 프로세스는 IPC로 메인에 위임한다.
//! - 전역 `static` 싱글톤을 통해 어떤 코드라도 `with_db(|db| ...)`로 접근 가능.
//! - `init(path)`가 먼저 호출되어야 함. 실패 시 앱은 `:memory:` 인메모리
//!   DB로 폴백하여 세션 한정으로만 동작.

mod migrations;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use anyhow::{Context, Result};
use rusqlite::Connection;

pub struct Db {
    pub conn: Connection,
}

impl Db {
    /// 디스크 경로로 엶. 실패 시 Err.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        let conn =
            Connection::open(path).with_context(|| format!("open sqlite at {}", path.display()))?;
        Self::prepare(conn)
    }

    /// 인메모리 DB. DB 열기 실패 시 폴백.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("open :memory: sqlite")?;
        Self::prepare(conn)
    }

    fn prepare(mut conn: Connection) -> Result<Self> {
        // WAL: 동시 read/write 부담 완화. synchronous=NORMAL: WAL과 궁합 좋음.
        // foreign_keys: PK 제약 정확성.
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "synchronous", "NORMAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON").ok();

        migrations::run(&mut conn).context("run migrations")?;
        Ok(Self { conn })
    }
}

static DB: OnceLock<Mutex<Db>> = OnceLock::new();

/// `~/.tasty/state.db` 경로. `None`이면 홈 디렉터리 미확인.
pub fn default_db_path() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".tasty").join("state.db"))
}

/// 앱 시작 시 1회 호출. 실패 시 인메모리 폴백.
/// 호출 이후부터 `with_db`가 유효.
pub fn init() {
    if DB.get().is_some() {
        return;
    }
    let db = match default_db_path() {
        Some(path) => match Db::open(&path) {
            Ok(db) => {
                tracing::info!("opened state.db at {}", path.display());
                db
            }
            Err(e) => {
                tracing::error!(
                    "failed to open state.db at {}: {e}. falling back to in-memory (no persistence).",
                    path.display()
                );
                Db::open_in_memory().expect("in-memory sqlite should always open")
            }
        },
        None => {
            tracing::error!("cannot determine ~/.tasty path; using in-memory state.db");
            Db::open_in_memory().expect("in-memory sqlite should always open")
        }
    };
    let _ = DB.set(Mutex::new(db));
}

/// 테스트용: 이미 열린 Db를 싱글톤으로 등록. 이미 등록되어 있으면 no-op.
#[cfg(test)]
pub fn init_with(db: Db) {
    let _ = DB.set(Mutex::new(db));
}

/// 싱글톤 접근. `init()`이 호출되지 않았으면 None.
pub fn with_db<T>(f: impl FnOnce(&mut Db) -> T) -> Option<T> {
    let mutex = DB.get()?;
    let mut guard: MutexGuard<'_, Db> = mutex.lock().ok()?;
    Some(f(&mut guard))
}
