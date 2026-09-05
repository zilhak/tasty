//! 웹훅 리스너 설정 영속화 — `~/.tasty/webhooks.toml`.
//!
//! **포트는 설정값 only**(자동 폴백 bind 없음). 파일이 처음 없으면 시드
//! [`SEED_PORT`] 한 개를 기록하고, 이후엔 파일의 `port` 값만 신뢰한다. 사용자가
//! `port` 를 비우면 리스너를 띄우지 않고 경고한다(caller 가 UI/로그로 노출).
//!
//! ## S5(영속화)와 파일 공유 — 라운드트립 보존
//! 이 파일은 S5 가 `Persistent` 웹훅 엔트리를 추가로 쓸 동일 파일이다. 그래서
//! read/write 를 [`toml::Table`] 통째로 다뤄 **`port` 이외의 키를 절대 건드리지
//! 않는다**(S5 가 추가할 `[[webhook]]` 등을 set 이 날려먹지 않도록). serde 구조체로
//! 역직렬화하지 않는 이유가 이 순방향/역방향 호환성이다.

use std::path::PathBuf;

/// 설정 파일이 처음 생성될 때 시드로 넣는 기본 포트.
///
/// 임의값 — IANA 등록/알려진 서비스 기본 포트가 아니며 User Ports(1024–49151)
/// 범위. 사용자가 그대로 쓰거나 바꾸면 된다.
pub const SEED_PORT: u16 = 28429;

/// `~/.tasty/webhooks.toml`. 홈 결정 실패 시 임시 경로(그 경우 seed 후에도 재기동
/// 시 새로 seed 되지만, 홈이 없는 환경은 예외적 — 파일핸들러 `user_config_path`
/// 선례와 동일한 fallback).
pub fn config_path() -> PathBuf {
    tasty_utils::path::tasty_home()
        .map(|d| d.join("webhooks.toml"))
        // 이유: 홈 미해결에서만 쓰는 공유 폴백. 인스턴스별 격리가 목적이 아니라 사용자
        // config 라 의도된 공유다(홈 없는 환경은 예외적, 파일핸들러 선례와 동일).
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

/// 테이블을 atomic write(파일핸들러 `save_user_config` 선례).
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

/// 부팅 시 포트 결정 — 파일이 없으면 시드 [`SEED_PORT`] 를 **처음 생성**하고
/// 반환한다. 파일이 있으면 그 `port` 값만 신뢰(비었으면 `None` → 리스너 미기동).
///
/// 반환 `None` = "포트 미설정" → caller 가 경고를 노출하고 bind 하지 않는다.
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

/// `port` 를 파일에 기록한다. **다른 키는 보존**(S5 호환). 리스너 재바인드는
/// 하지 않으므로 실제 반영은 재시작 시점이다(caller 가 안내).
pub fn set_port(port: u16) -> std::io::Result<()> {
    let path = config_path();
    let mut table = read_table(&path);
    table.insert("port".into(), toml::Value::Integer(port as i64));
    write_table(&path, &table)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트마다 격리된 TASTY_HOME 를 잡는다(seed/read/set 이 실파일을 건드리므로).
    /// 공유 락 획득·원값 복원까지 가드가 맡는다 — `crate::test_support` 참조.
    use crate::test_support::TastyHomeGuard as HomeGuard;

    #[test]
    fn seeds_default_port_when_absent() {
        let _home = HomeGuard::new();
        // 파일 부재 → 시드값 반환 + 파일 생성.
        assert_eq!(load_or_seed(), Some(SEED_PORT));
        assert!(config_path().exists());
        // 두 번째 호출은 방금 쓴 파일을 읽어 동일값.
        assert_eq!(load_or_seed(), Some(SEED_PORT));
        assert_eq!(read_port(), Some(SEED_PORT));
    }

    #[test]
    fn empty_port_yields_none() {
        let _home = HomeGuard::new();
        // port 없는 파일을 직접 써 둔다(S5 가 다른 키만 쓴 상태를 흉내).
        std::fs::write(config_path(), "other_key = 1\n").unwrap();
        assert_eq!(load_or_seed(), None);
        assert_eq!(read_port(), None);
    }

    #[test]
    fn set_port_preserves_other_keys() {
        let _home = HomeGuard::new();
        // S5 소유 키를 미리 심어 두고 set_port 가 보존하는지 확인.
        std::fs::write(config_path(), "keep_me = \"s5\"\nport = 100\n").unwrap();
        set_port(40000).unwrap();
        let table = read_table(&config_path());
        assert_eq!(table.get("port").and_then(|v| v.as_integer()), Some(40000));
        assert_eq!(
            table.get("keep_me").and_then(|v| v.as_str()),
            Some("s5"),
            "S5 소유 키가 set_port 후에도 보존돼야 한다"
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
