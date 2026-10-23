//! Shared webhooks.toml section updates for the host and standalone CLI.
//! Every mutation reads the latest TOML under the same participating file lock. Other sections
//! survive semantically; comments/formatting are not byte-preserved. Malformed/unreadable files
//! are never replaced. The guard drops before returning to caller-side publication or actions.
//! Nonparticipating editors are checked before replacement; the final check/rename race remains.
use std::path::{Path, PathBuf};
use tasty_utils::file_replace::{self, Fingerprint};

/// First port attempted when no explicit port is stored; also the legacy automatic seed value.
pub const DEFAULT_PORT: u16 = 28429;
const FORMAT_KEY: &str = "format";
const FORMAT: i64 = 2;

/// Uses the same data-root rule for persistent registrations and listener port settings.
pub fn path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("webhooks.toml"))
        // 이유: 홈이 없을 때도 포트 설정과 영속 등록은 같은 파일을 공유한다.
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-webhooks.toml"))
}

fn read_document(path: &Path) -> std::io::Result<(toml::Table, Fingerprint)> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok((toml::Table::new(), Fingerprint::Missing));
        }
        Err(e) => return Err(e),
    };
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid_document(path))?;
    // toml::de::Error includes source excerpts: never place auth-bearing input in errors/logs.
    let table = text
        .parse::<toml::Table>()
        .map_err(|_| invalid_document(path))?;
    Ok((table, Fingerprint::of_bytes(&bytes)))
}

fn invalid_document(path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("{} is not valid UTF-8 TOML", path.display()),
    )
}

/// Read a complete document, without migration. Missing is empty; other failures are errors.
/// Mutation callers must use the participating section APIs, not retain this whole-file snapshot.
pub fn read_table(path: &Path) -> std::io::Result<toml::Table> {
    read_document(path).map(|(table, _)| table)
}

/// Private guard: section APIs cannot leak the file lock into external caller actions.
struct LockedTable {
    path: PathBuf,
    target: PathBuf,
    before: Fingerprint,
    table: toml::Table,
    _writer: file_replace::WriteLock,
}

impl LockedTable {
    fn open(path: &Path) -> std::io::Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent)?;
        let target = file_replace::resolve_link(path)?;
        let writer = file_replace::lock(path)?;
        if file_replace::resolve_link(path)? != target {
            return Err(std::io::Error::other(
                "webhooks.toml target changed while waiting for its lock",
            ));
        }
        let (table, before) = read_document(path)?;
        Ok(Self {
            path: path.into(),
            target,
            before,
            table,
            _writer: writer,
        })
    }

    fn commit(self) -> std::io::Result<()> {
        let text = toml::to_string_pretty(&self.table)
            .map_err(|_| std::io::Error::other("webhooks.toml render failed"))?;
        if file_replace::resolve_link(&self.path)? != self.target
            || Fingerprint::read(&self.path)? != self.before
        {
            return Err(std::io::Error::other(
                "webhooks.toml changed while preparing its replacement",
            ));
        }
        file_replace::replace_with(
            &self.path,
            text.as_bytes(),
            file_replace::Options {
                new_mode: Some(0o600),
                sync: false,
            },
        )
    }
}

/// Replace only the host-owned webhook array. Read/merge/replace is one participating critical
/// section shared with set/clear and read-time migration. An empty array removes this key only.
pub fn replace_webhooks(path: &Path, webhooks: Vec<toml::Value>) -> std::io::Result<()> {
    let mut file = LockedTable::open(path)?;
    if webhooks.is_empty() {
        file.table.remove("webhook");
    } else {
        file.table
            .insert("webhook".into(), toml::Value::Array(webhooks));
    }
    file.commit()
}

fn port_from_table(table: &toml::Table) -> Option<u16> {
    let n = table.get("port")?.as_integer()?;
    match u16::try_from(n) {
        Ok(port) if port > 0 => Some(port),
        _ => {
            tracing::warn!("webhooks.toml port out of range (1..=65535); ignoring");
            None
        }
    }
}

/// Migrate only in memory; a section mutation commits the final merged table once.
fn migrate(table: &mut toml::Table) -> bool {
    if table.contains_key(FORMAT_KEY) {
        return false;
    }
    if table.get("port").and_then(toml::Value::as_integer) == Some(i64::from(DEFAULT_PORT)) {
        table.remove("port");
    }
    table.insert(FORMAT_KEY.into(), toml::Value::Integer(FORMAT));
    true
}

/// Explicit port after legacy seed migration. A migration write failure keeps its historical
/// in-memory interpretation for this read, but leaves the original file and reports a warning.
pub fn read_port(path: &Path) -> Option<u16> {
    // Pure reads do not wait on a writer. Only legacy migration is a mutation; it must
    // acquire the participating lock and reread instead of writing this initial snapshot.
    match read_document(path) {
        Ok((table, before)) if before == Fingerprint::Missing || table.contains_key(FORMAT_KEY) => {
            return port_from_table(&table);
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(
                "webhooks.toml unreadable ({e}); ignoring its port and leaving the file as it is"
            );
            return None;
        }
    }
    let mut file = match LockedTable::open(path) {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!(
                "webhooks.toml unreadable ({e}); ignoring its port and leaving the file as it is"
            );
            return None;
        }
    };
    if file.before == Fingerprint::Missing {
        return None;
    }
    let migrated = migrate(&mut file.table);
    let port = port_from_table(&file.table);
    if migrated && let Err(e) = file.commit() {
        tracing::warn!("webhooks.toml format update failed: {e}");
    }
    port
}

/// Save an explicit port; it applies on the next listener start. Other sections are preserved.
pub fn set_port(path: &Path, port: u16) -> std::io::Result<()> {
    update_port(path, Some(port))
}

/// Clear the explicit port. The next listener start probes from the default port.
pub fn clear_port(path: &Path) -> std::io::Result<()> {
    update_port(path, None)
}

fn update_port(path: &Path, port: Option<u16>) -> std::io::Result<()> {
    let mut file = LockedTable::open(path)?;
    migrate(&mut file.table);
    match port {
        Some(port) => {
            file.table
                .insert("port".into(), toml::Value::Integer(i64::from(port)));
        }
        None => {
            file.table.remove("port");
        }
    }
    file.table
        .insert(FORMAT_KEY.into(), toml::Value::Integer(FORMAT));
    file.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("webhooks.toml");
        (dir, path)
    }

    #[test]
    fn nothing_is_written_when_the_file_is_absent() {
        let (_dir, path) = file();
        assert_eq!(read_port(&path), None);
        assert!(!path.exists(), "no seed port is written");
    }

    #[test]
    fn an_old_seeded_port_is_dropped_once_and_other_keys_stay() {
        let (_dir, path) = file();
        std::fs::write(&path, "port = 28429\n[[webhook]]\nid = \"a\"\n").unwrap();
        assert_eq!(read_port(&path), None);
        let table = read_table(&path).unwrap();
        assert!(table.get("port").is_none());
        assert_eq!(table.get("format").and_then(|v| v.as_integer()), Some(2));
        assert!(table.get("webhook").is_some(), "other keys stay");
        // 형식 표시 뒤에 사용자가 같은 값을 정하면 명시 지정으로 남는다.
        set_port(&path, DEFAULT_PORT).unwrap();
        assert_eq!(read_port(&path), Some(DEFAULT_PORT));
    }

    #[test]
    fn an_old_user_port_stays_explicit() {
        let (_dir, path) = file();
        std::fs::write(&path, "port = 40000\n").unwrap();
        assert_eq!(read_port(&path), Some(40000));
        clear_port(&path).unwrap();
        assert_eq!(read_port(&path), None);
    }

    #[test]
    fn set_port_preserves_other_keys() {
        let (_dir, path) = file();
        std::fs::write(&path, "keep_me = \"s5\"\nport = 100\n").unwrap();
        set_port(&path, 40000).unwrap();
        let table = read_table(&path).unwrap();
        assert_eq!(table.get("port").and_then(|v| v.as_integer()), Some(40000));
        assert_eq!(
            table.get("keep_me").and_then(|v| v.as_str()),
            Some("s5"),
            "set_port가 다른 설정 키를 보존해야 한다"
        );
    }

    #[test]
    fn a_file_that_does_not_parse_is_left_byte_for_byte() {
        let (_dir, path) = file();
        let broken = "port = 40123\n[[webhook]]\nid = \"keepme\"\nthis is not toml\n";
        std::fs::write(&path, broken).unwrap();
        assert_eq!(read_port(&path), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        assert!(set_port(&path, 40000).is_err());
        assert!(clear_port(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    }

    #[test]
    fn out_of_range_port_ignored() {
        let (_dir, path) = file();
        std::fs::write(&path, "format = 2\nport = 70000\n").unwrap();
        assert_eq!(read_port(&path), None);
        std::fs::write(&path, "format = 2\nport = 0\n").unwrap();
        assert_eq!(read_port(&path), None);
    }
}

#[cfg(test)]
mod backend_tests;
