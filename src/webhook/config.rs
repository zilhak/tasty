//! 데이터 루트의 webhooks.toml에서 리스너 포트를 읽고 쓴다.
//! 파일이 없으면 SEED_PORT를 저장하려고 시도하고 이번 실행에서도 그 값을 사용한다.
//! 기존 파일에 유효한 port가 없으면 리스너를 시작하지 않으며 다른 포트로 재시도하지 않는다.
//!
//! TOML 테이블에서 port만 바꿔 읽어 온 다른 키를 보존한다.
//! 읽기·파싱 실패 시에는 빈 테이블로 처리하므로 기존 키 보존을 보장하지 못한다.

use std::path::PathBuf;

/// 설정 파일이 없을 때 사용할 초기 포트.
pub const SEED_PORT: u16 = 28429;

/// 데이터 루트가 없으면 임시 디렉터리의 공유 경로를 사용한다.
pub fn config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("webhooks.toml"))
        // 이유: 홈이 없을 때도 포트 설정과 영속 등록이 같은 사용자 설정 파일을 공유한다.
        .unwrap_or_else(|| std::env::temp_dir().join("tasty-webhooks.toml"))
}

/// 파일을 `toml::Table` 로 읽는다. 없거나 파싱 실패면 빈 테이블(파싱 실패는 warn).
fn read_table(path: &std::path::Path) -> toml::Table {
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
fn write_table(path: &std::path::Path, table: &toml::Table) -> std::io::Result<()> {
    use std::io::Write;
    let text = toml::to_string(table)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
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
    if (1..=65535).contains(&n) {
        Some(n as u16)
    } else {
        tracing::warn!("webhooks.toml 'port' = {n} out of range (1..=65535); ignoring");
        None
    }
}

/// 파일이 없으면 초기 포트를 반환한다. 기존 파일의 유효하지 않은 port는 None이다.
pub fn load_or_seed() -> Option<u16> {
    let path = config_path();
    if !path.exists() {
        let mut table = toml::Table::new();
        table.insert("port".into(), toml::Value::Integer(SEED_PORT as i64));
        if let Err(e) = write_table(&path, &table) {
            tracing::warn!("webhooks.toml seed write failed: {e}");
            // 파일을 못 만들어도 시드값 자체는 이번 세션에 쓸 수 있게 반환.
        }
        return Some(SEED_PORT);
    }
    port_from_table(&read_table(&path))
}

/// 파일의 현재 `port` 를 읽는다(시드하지 않음). `webhook.config` get 용.
pub fn read_port() -> Option<u16> {
    port_from_table(&read_table(&config_path()))
}

/// port를 저장한다. 읽어 온 다른 키는 보존하며 리스너에는 재시작 후 적용된다.
pub fn set_port(port: u16) -> std::io::Result<()> {
    let path = config_path();
    let mut table = read_table(&path);
    table.insert("port".into(), toml::Value::Integer(port as i64));
    write_table(&path, &table)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 공유 락과 TestHome으로 실제 설정 경로를 시험용 홈에 격리한다.
    use crate::test_support::TastyHomeGuard as HomeGuard;

    #[test]
    fn seeds_default_port_when_absent() {
        let _home = HomeGuard::new();
        assert_eq!(load_or_seed(), Some(SEED_PORT));
        assert!(config_path().exists());
        assert_eq!(load_or_seed(), Some(SEED_PORT));
        assert_eq!(read_port(), Some(SEED_PORT));
    }

    #[test]
    fn empty_port_yields_none() {
        let _home = HomeGuard::new();
        std::fs::write(config_path(), "other_key = 1\n").unwrap();
        assert_eq!(load_or_seed(), None);
        assert_eq!(read_port(), None);
    }

    #[test]
    fn set_port_preserves_other_keys() {
        let _home = HomeGuard::new();
        std::fs::write(config_path(), "keep_me = \"s5\"\nport = 100\n").unwrap();
        set_port(40000).unwrap();
        let table = read_table(&config_path());
        assert_eq!(table.get("port").and_then(|v| v.as_integer()), Some(40000));
        assert_eq!(
            table.get("keep_me").and_then(|v| v.as_str()),
            Some("s5"),
            "set_port가 다른 설정 키를 보존해야 한다"
        );
    }

    #[test]
    fn out_of_range_port_ignored() {
        let _home = HomeGuard::new();
        std::fs::write(config_path(), "port = 70000\n").unwrap();
        assert_eq!(read_port(), None);
        std::fs::write(config_path(), "port = 0\n").unwrap();
        assert_eq!(read_port(), None);
    }
}
