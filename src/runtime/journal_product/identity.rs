//! One explicit journal binding per data home. Never select by process, focus, or file age.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

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

pub(super) fn open(home: &Path) -> Result<tasty_event_store::EventStore, String> {
    let directory = home.join("structure");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    File::open(home)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    let _initialization = lock_initialization(&directory)?;
    let path = directory.join("journal.json");
    let database = directory.join("journal.db");
    let mut binding = read_or_create(&path, &database)?;
    if binding.phase == Phase::Ready && !database.is_file() {
        return Err("active structure journal database is missing; refusing to recreate it".into());
    }
    let store = tasty_event_store::EventStore::open(&database, &binding.journal_id)
        .map_err(|e| e.to_string())?;
    if binding.phase == Phase::Preparing {
        // No command can be admitted until the durable Ready marker has been published.
        binding.phase = Phase::Ready;
        write_binding(&path, &binding)?;
    }
    Ok(store)
}

fn lock_initialization(directory: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("journal.binding-lock"))
        .map_err(|e| e.to_string())?;
    // Keep the lock file: deleting it could let two processes lock different inodes.
    file.try_lock()
        .map_err(|e| format!("structure journal binding is unavailable: {e}"))?;
    Ok(file)
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
    // Both UTF-16 paths remain valid for the call; the temporary file was closed and flushed.
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
