//! 데이터 루트의 webhooks.toml에서 명시 지정 웹훅 포트를 읽고 쓴다. 실행 중인 인스턴스(IPC 처리)와
//! 인스턴스가 없을 때의 `tasty webhook port`(CLI 크레이트)가 같은 규칙으로 파일을 다루도록 둘이 함께 쓰는
//! 이 크레이트 한 곳에 둔다.
//! 값이 없으면 리스너가 기본 포트부터 빈 포트를 찾는다. 이 모듈은 값을 새로 만들어 넣지 않는다.
//!
//! 이전 버전은 파일이 없을 때 기본 포트 28429를 자동으로 써 넣었다. 그 값은 사용자가 정한 것과
//! 구별할 수 없으므로, 형식 표시(`format = 2`)가 없는 파일의 `port = 28429`는 자동으로 들어간 값으로 보고
//! 처음 읽을 때 지운다. 다른 값은 사용자가 정한 것으로 남긴다. 이 모듈이 파일을 쓸 때마다 형식 표시를 함께 쓴다.
//!
//! TOML 테이블에서 port만 바꿔 읽어 온 다른 키(`[[webhook]]` 영속 등록 등)를 보존한다.
//! 읽기·파싱 실패 시에는 빈 테이블로 처리하므로 기존 키 보존을 보장하지 못한다.

use std::path::{Path, PathBuf};

/// 명시 지정이 없을 때 리스너가 처음 시도하는 포트. 이전 버전이 자동으로 써 넣던 값이기도 하다.
pub const DEFAULT_PORT: u16 = 28429;

/// 자동으로 써 넣은 포트를 정리한 뒤의 파일 형식 표시.
const FORMAT_KEY: &str = "format";
const FORMAT: i64 = 2;

/// 데이터 루트가 없으면 임시 디렉터리의 공유 경로를 사용한다.
pub fn path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("webhooks.toml"))
        // 이유: 홈이 없을 때도 포트 설정과 영속 등록이 같은 사용자 설정 파일을 공유한다.
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-webhooks.toml"))
}

/// 파일을 `toml::Table` 로 읽는다. 없거나 파싱 실패면 빈 테이블(파싱 실패는 warn).
pub fn read_table(path: &Path) -> toml::Table {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return toml::Table::new(),
    };
    match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("webhooks.toml parse failed ({e}); treating as empty");
            toml::Table::new()
        }
    }
}

/// 같은 디렉터리의 임시 파일을 쓴 뒤 대상 경로로 교체한다.
fn write_table(path: &Path, table: &toml::Table) -> std::io::Result<()> {
    use std::io::Write;
    let text = toml::to_string(table)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(text.as_bytes())?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// 테이블에서 `port` 를 유효 u16 으로 뽑는다. 부재/범위밖/타입불일치는 `None`.
fn port_from_table(table: &toml::Table) -> Option<u16> {
    let raw = table.get("port")?;
    // toml 정수는 i64. 1..=65535 만 유효 포트로 인정(0 = 미설정 취급).
    let n = raw.as_integer()?;
    match u16::try_from(n) {
        Ok(port) if port > 0 => Some(port),
        _ => {
            tracing::warn!("webhooks.toml 'port' = {n} out of range (1..=65535); ignoring");
            None
        }
    }
}

/// 형식 표시가 없는 파일의 자동 기록 포트를 지운다. 바꾼 내용이 있으면 파일에 다시 쓴다.
/// 쓰기에 실패하면 이번 실행에서만 지운 값으로 다룬다.
fn migrate(path: &Path, table: &mut toml::Table) {
    if !path.exists() || table.get(FORMAT_KEY).is_some() {
        return;
    }
    if table.get("port").and_then(toml::Value::as_integer) == Some(i64::from(DEFAULT_PORT)) {
        table.remove("port");
        tracing::info!(
            "webhooks.toml: dropped the port {DEFAULT_PORT} an older version wrote automatically"
        );
    }
    table.insert(FORMAT_KEY.into(), toml::Value::Integer(FORMAT));
    if let Err(e) = write_table(path, table) {
        tracing::warn!("webhooks.toml format update failed: {e}");
    }
}

/// 사용자가 명시로 정한 포트. 없거나 범위 밖이면 None이다. 읽을 때 이전 파일을 정리한다.
pub fn read_port(path: &Path) -> Option<u16> {
    let mut table = read_table(path);
    migrate(path, &mut table);
    port_from_table(&table)
}

/// port를 저장한다. 읽어 온 다른 키는 보존하며 리스너에는 다음 실행부터 적용된다.
pub fn set_port(path: &Path, port: u16) -> std::io::Result<()> {
    update(path, |table| {
        table.insert("port".into(), toml::Value::Integer(i64::from(port)));
    })
}

/// 저장한 port를 지운다. 다음 실행부터 기본 포트부터 빈 포트를 찾는다.
pub fn clear_port(path: &Path) -> std::io::Result<()> {
    update(path, |table| {
        table.remove("port");
    })
}

fn update(path: &Path, change: impl FnOnce(&mut toml::Table)) -> std::io::Result<()> {
    let mut table = read_table(path);
    migrate(path, &mut table);
    change(&mut table);
    table.insert(FORMAT_KEY.into(), toml::Value::Integer(FORMAT));
    write_table(path, &table)
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
        let table = read_table(&path);
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
        let table = read_table(&path);
        assert_eq!(table.get("port").and_then(|v| v.as_integer()), Some(40000));
        assert_eq!(
            table.get("keep_me").and_then(|v| v.as_str()),
            Some("s5"),
            "set_port가 다른 설정 키를 보존해야 한다"
        );
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
