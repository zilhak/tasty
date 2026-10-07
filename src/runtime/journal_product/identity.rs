//! One explicit journal binding per data home. Never select by process, focus, or file age.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::StartupFailure;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Preparing,
    Ready,
}

#[derive(Debug, Serialize, Deserialize)]
struct Binding {
    version: u32,
    journal_id: String,
    phase: Phase,
}

fn structure_directory(home: &Path) -> PathBuf {
    home.join("structure")
}

/// 데이터 홈의 구조 저널 파일. 부팅 첫머리의 writer 잠금 선점과 [`open`]이 같은 경로를 쓴다.
pub(crate) fn journal_database_path(home: &Path) -> PathBuf {
    structure_directory(home).join("journal.db")
}

/// `writer_lock`은 [`journal_database_path`]에 대해 미리 얻은 잠금이다. 있으면 저장소를 열기
/// 전부터 쥐고 있으므로 schema migration도 잠금 소유자만 한다.
pub(super) fn open(
    home: &Path,
    writer_lock: Option<tasty_event_store::WriterLock>,
) -> Result<tasty_event_store::EventStore, StartupFailure> {
    let directory = structure_directory(home);
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    File::open(home)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    let _initialization = lock_initialization(&directory)?;
    let path = directory.join("journal.json");
    let database = journal_database_path(home);
    let mut binding = read_or_create(&path, &database)?;
    if binding.phase == Phase::Ready && !database.is_file() {
        return Err("active structure journal database is missing; refusing to recreate it".into());
    }
    let mut store = tasty_event_store::EventStore::open(&database, &binding.journal_id)
        .map_err(|e| e.to_string())?;
    if let Some(lock) = writer_lock {
        store.acquire_writer_with(lock).map_err(|e| e.to_string())?;
    }
    if binding.phase == Phase::Preparing {
        // No command can be admitted until the durable Ready marker has been published.
        binding.phase = Phase::Ready;
        write_binding(&path, &binding)?;
    }
    Ok(store)
}

fn lock_initialization(directory: &Path) -> Result<File, StartupFailure> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("journal.binding-lock"))
        .map_err(|e| e.to_string())?;
    // Keep the lock file: deleting it could let two processes lock different inodes.
    // 같은 프로세스가 fork한 자식은 exec 전까지 방금 놓은 잠금의 열린 파일 설명을 쥐고 있을 수 있어,
    // writer 잠금과 같은 구간 동안 다시 시도한다.
    let locked = tasty_event_store::retry_while_held(|| match file.try_lock() {
        Ok(()) => Ok(Some(())),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(StartupFailure::Other(format!(
            "structure journal binding is unavailable: {e}"
        ))),
    })?;
    match locked {
        Some(()) => Ok(file),
        // 다른 프로세스가 같은 홈의 저널을 초기화하는 중이다.
        None => Err(StartupFailure::HomeInUse(
            "structure journal binding is held by another process".into(),
        )),
    }
}

fn read_or_create(path: &Path, database: &Path) -> Result<Binding, String> {
    match std::fs::read(path) {
        Ok(bytes) => return decode(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    if database.exists() {
        return Err("structure journal exists without its explicit journal binding".into());
    }
    let binding = Binding {
        version: 1,
        journal_id: format!("journal-{:032x}", rand::random::<u128>()),
        phase: Phase::Preparing,
    };
    write_binding(path, &binding)?;
    Ok(binding)
}

pub(super) fn write_binding<T: Serialize>(path: &Path, binding: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("journal binding has no parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary
        .write_all(&serde_json::to_vec(binding).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    persist(temporary, path)?;
    #[cfg(unix)]
    File::open(parent)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(windows))]
fn persist(temporary: tempfile::NamedTempFile, path: &Path) -> Result<(), String> {
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(windows)]
fn persist(temporary: tempfile::NamedTempFile, path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;
    let temporary = temporary.into_temp_path();
    let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: Both UTF-16 paths are NUL-terminated and outlive the call; the temporary file was closed and flushed.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|e| e.to_string())?;
    // The move already removed the temporary path; retaining its destructor performs harmless cleanup.
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Binding, String> {
    let binding: Binding = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if binding.version != 1
        || binding.journal_id.len() != 40
        || !binding
            .journal_id
            .strip_prefix("journal-")
            .is_some_and(|id| id.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("unsupported or invalid structure journal binding".into());
    }
    Ok(binding)
}

#[cfg(test)]
mod tests {
    use std::fs::{File, OpenOptions};
    use std::path::Path;
    use std::time::{Duration, Instant};

    use tasty_event_store::WRITER_LOCK_WAIT;

    fn hold(directory: &Path) -> File {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("journal.binding-lock"))
            .unwrap();
        file.try_lock().unwrap();
        file
    }

    #[test]
    fn a_binding_lock_held_through_the_retry_window_is_home_in_use() {
        let home = tempfile::tempdir().unwrap();
        let held = hold(home.path());
        let started = Instant::now();
        let error = super::lock_initialization(home.path()).unwrap_err();
        assert!(error.is_home_in_use(), "{error}");
        assert!(started.elapsed() >= WRITER_LOCK_WAIT);
        drop(held);
    }

    /// fork한 자식이 exec 전까지 binding 잠금을 잠깐 공유하는 경우처럼, 재시도 구간 안에 풀리는 잠금은 기다려 얻는다.
    #[test]
    fn a_binding_lock_released_within_the_retry_window_is_acquired() {
        let home = tempfile::tempdir().unwrap();
        let held = hold(home.path());
        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(held);
        });
        let locked = super::lock_initialization(home.path());
        holder.join().unwrap();
        assert!(locked.is_ok(), "{:?}", locked.err());
    }
}
