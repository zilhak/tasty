//! 로그인 자격증명(Playwright storageState) 저장 — **평문 파일 + OS 파일 권한**.
//!
//! ADR-0018(ADR-0005 철학 계승): 암호화-앳-레스트를 하지 않는다. 보호 범위를
//! "OS user / 파일 권한" 한 가지로 정직하게 좁힌다. plugin sandbox 가 없는 현재,
//! 같은 user 권한의 악성 프로세스는 어차피 키도 파일도 읽을 수 있어 AES-GCM+keyring
//! 은 false sense of security 일 뿐이다(ADR-0005 와 동일 결론). 디스크 직접 노출 /
//! 백업·클라우드 sync / 기기 도난 시 평문 노출은 책임지지 않는다 — 그 한계를 문서로
//! 명시한다(ADR-0018).
//!
//! Unix 는 파일 모드를 0600 으로 제한한다. Windows 는 파일이 user 프로필(data_dir)
//! 아래라 기본 ACL 이 user-scoped 이며, 추가 ACL 강화는 정책상 약속하지 않는다.

use std::path::{Path, PathBuf};

/// storageState 평문 저장 파일명.
const AUTH_FILE: &str = "auth.json";

pub fn auth_path(data_dir: &Path) -> PathBuf {
    data_dir.join(AUTH_FILE)
}

/// 저장된 auth 가 존재하는지 (값을 읽지 않고 빠르게).
pub fn has_auth(data_dir: &Path) -> bool {
    auth_path(data_dir).is_file()
}

/// storageState(JSON 문자열)를 data_dir 에 atomic 저장. Unix 는 0600.
pub fn save_auth(storage_state: &str, data_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = auth_path(data_dir);
    let tmp = data_dir.join("auth.json.tmp");
    std::fs::write(&tmp, storage_state)?;
    set_owner_only(&tmp)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 저장된 storageState 를 읽는다. 없으면 `Ok(None)`.
pub fn load_auth(data_dir: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(auth_path(data_dir)) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// 저장된 auth 를 삭제(logout). 없으면 no-op.
pub fn clear_auth(data_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(auth_path(data_dir)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> std::io::Result<()> {
    // Windows: data_dir 이 user 프로필 아래라 기본 ACL 이 user-scoped. 평문 정책상
    // 추가 ACL 약속은 하지 않는다(ADR-0018).
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_clear_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("tasty-design-auth-{}", std::process::id()));
        assert_eq!(load_auth(&dir).unwrap(), None);
        assert!(!has_auth(&dir));

        let payload = r#"{"cookies":[{"name":"sessionKey","value":"x"}],"origins":[]}"#;
        save_auth(payload, &dir).unwrap();
        assert!(has_auth(&dir));
        assert_eq!(load_auth(&dir).unwrap().as_deref(), Some(payload));

        clear_auth(&dir).unwrap();
        assert_eq!(load_auth(&dir).unwrap(), None);
        clear_auth(&dir).unwrap(); // 두 번째도 안전(no-op).

        if let Err(e) = std::fs::remove_dir_all(&dir) {
            eprintln!("cleanup failed: {e}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_perms_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tasty-design-perm-{}", std::process::id()));
        save_auth("{}", &dir).unwrap();
        let mode = std::fs::metadata(auth_path(&dir)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "auth file must be 0600");
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            eprintln!("cleanup failed: {e}");
        }
    }
}
