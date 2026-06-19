//! Passkey 저장소 — `~/.tasty/passkeys.toml` (0600) + `~/.tasty/passkeys/` (0700).
//!
//! 모든 자격증명은 at-rest 에서 **파일 경로로 수렴**한다:
//! - `kind="path"` — 사용자 소유 기존 파일 참조(tasty 는 경로만, 수명 미관여).
//! - `kind="inline"` — 사용자 입력 문자열 → tasty 가 `~/.tasty/passkeys/<name>` 0600
//!   파일로 써서 소유(passkey 삭제/수정 시 파일도 관리).
//!
//! toml 엔 비밀 *값* 이 없다(경로뿐). 보호는 암호화가 아니라 OS 파일권한 위임이다.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tasty_utils::path::tasty_home;

/// 알려진 passkey kind. 그 외(미등록)도 저장은 되되 UI/CLI 가 경고 표시(상위).
pub const KNOWN_PASSKEY_KINDS: &[&str] = &["path", "inline"];

/// **대화형 등록용** 이름 검증 — 영숫자/`-`/`_` 화이트리스트. name 이 파일명으로
/// 쓰이므로 path traversal 을 원천 차단한다(그 외 문자는 거부).
pub fn is_valid_passkey_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// **마이그레이션/자동생성용** 이름 변환 — 거부 대신 비허용 문자를 `_` 로 치환한다
/// (기존 데이터는 거부할 수 없으므로). 결과는 항상 화이트리스트를 통과한다.
pub fn sanitize_passkey_name(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() { "key".to_string() } else { s }
}

/// 한 자격증명. `path` 는 항상 파일 경로(inline 이면 tasty 관리 파일 경로).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Passkey {
    /// 고유 식별자(프로필이 `passkey_ref` 로 참조).
    pub name: String,
    /// `path` | `inline`(+ 미등록). 라이프사이클 소유권을 가른다.
    pub kind: String,
    /// at-rest 경로. inline 이면 `~/.tasty/passkeys/<name>`.
    pub path: String,
}

impl Passkey {
    /// tasty 가 소유·관리하는 inline 파일인지(삭제 시 파일도 지운다).
    pub fn is_managed(&self) -> bool {
        self.kind == "inline"
    }
}

/// `~/.tasty/passkeys.toml` 전체 — 자격증명 목록 + 스키마 버전.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct Passkeys {
    #[serde(default)]
    pub version: u32,
    #[serde(default, rename = "passkey")]
    pub passkeys: Vec<Passkey>,
}

/// `~/.tasty/passkeys.toml` 경로.
pub fn passkeys_file() -> Option<PathBuf> {
    tasty_home().map(|dir| dir.join("passkeys.toml"))
}

/// inline 비밀 파일 디렉토리 `~/.tasty/passkeys/`.
pub fn passkeys_dir() -> Option<PathBuf> {
    tasty_home().map(|dir| dir.join("passkeys"))
}

/// 파일 권한 0600(unix). 다른 OS 는 no-op(유저 프로필 ACL 에 위임).
fn set_private_file(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    let _ = path;
    Ok(())
}

/// 디렉토리 권한 0700(unix). 다른 OS 는 no-op.
fn set_private_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let _ = path;
    Ok(())
}

/// 주어진 디렉토리에 inline 비밀을 0600 파일로 쓰고 그 경로를 반환한다(경로 주입형 —
/// 테스트가 temp dir 로 호출). 이름은 [`is_valid_passkey_name`] 통과 필수.
pub fn materialize_inline_in(dir: &Path, name: &str, secret: &str) -> Result<PathBuf> {
    if !is_valid_passkey_name(name) {
        bail!("invalid passkey name '{name}' (allowed: letters, digits, - and _)");
    }
    fs::create_dir_all(dir).with_context(|| format!("create passkeys dir {}", dir.display()))?;
    set_private_dir(dir).ok(); // best-effort(이미 존재하던 dir 권한 강제는 실패 무시)
    let path = dir.join(name);
    fs::write(&path, secret).with_context(|| format!("write passkey file {}", path.display()))?;
    set_private_file(&path)
        .with_context(|| format!("chmod 0600 {}", path.display()))?;
    Ok(path)
}

impl Passkeys {
    /// 저장 파일 경로.
    pub fn path() -> Option<PathBuf> {
        passkeys_file()
    }

    fn ensure_dir() -> Result<()> {
        if let Some(path) = Self::path()
            && let Some(parent) = path.parent()
        {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// 로드한다. 없거나 파싱 실패면 빈 목록(default)으로 폴백한다.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            tracing::info!("no passkeys path available, using empty list");
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<Passkeys>(&contents) {
                Ok(p) => {
                    tracing::info!("loaded {} passkey(s) from {}", p.passkeys.len(), path.display());
                    p
                }
                Err(e) => {
                    tracing::warn!("failed to parse passkeys file: {e}, using empty list");
                    Self::default()
                }
            },
            Err(_) => {
                tracing::info!("no passkeys file at {}, using empty list", path.display());
                Self::default()
            }
        }
    }

    /// pretty TOML 로 전체를 덮어쓰고 파일을 0600 으로 만든다.
    pub fn save(&self) -> Result<()> {
        Self::ensure_dir()?;
        let Some(path) = Self::path() else {
            anyhow::bail!("could not determine passkeys path");
        };
        let mut to_write = self.clone();
        if to_write.version == 0 {
            to_write.version = 1;
        }
        let contents = toml::to_string_pretty(&to_write)?;
        fs::write(&path, contents)?;
        set_private_file(&path).ok(); // best-effort
        tracing::info!("saved {} passkey(s) to {}", to_write.passkeys.len(), path.display());
        Ok(())
    }

    /// 이름으로 조회.
    pub fn get(&self, name: &str) -> Option<&Passkey> {
        self.passkeys.iter().find(|p| p.name == name)
    }

    /// name 이 같으면 교체, 없으면 추가(메모리만 — 디스크 반영은 [`Self::save`]).
    pub fn upsert(&mut self, passkey: Passkey) {
        if let Some(existing) = self.passkeys.iter_mut().find(|p| p.name == passkey.name) {
            *existing = passkey;
        } else {
            self.passkeys.push(passkey);
        }
    }

    /// path kind passkey 등록(이름 검증). 메모리만 변경.
    pub fn upsert_path(&mut self, name: impl Into<String>, file_path: impl Into<String>) -> Result<()> {
        let name = name.into();
        if !is_valid_passkey_name(&name) {
            bail!("invalid passkey name '{name}' (allowed: letters, digits, - and _)");
        }
        self.upsert(Passkey { name, kind: "path".into(), path: file_path.into() });
        Ok(())
    }

    /// inline kind passkey 등록 — 비밀을 `~/.tasty/passkeys/<name>` 0600 파일로
    /// materialize 하고 그 경로를 저장한다. 메모리만 변경(toml 반영은 [`Self::save`]).
    pub fn upsert_inline(&mut self, name: impl Into<String>, secret: &str) -> Result<()> {
        let name = name.into();
        let dir = passkeys_dir().context("could not determine passkeys dir")?;
        let path = materialize_inline_in(&dir, &name, secret)?;
        self.upsert(Passkey {
            name,
            kind: "inline".into(),
            path: path.to_string_lossy().into_owned(),
        });
        Ok(())
    }

    /// name 으로 제거. inline 이면 관리 파일도 삭제한다. 제거됐으면 true.
    pub fn remove(&mut self, name: &str) -> bool {
        let Some(idx) = self.passkeys.iter().position(|p| p.name == name) else {
            return false;
        };
        let pk = self.passkeys.remove(idx);
        if pk.is_managed()
            && let Err(e) = fs::remove_file(&pk.path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!("failed to remove managed passkey file {}: {e}", pk.path);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_name_whitelist() {
        assert!(is_valid_passkey_name("gx10-key"));
        assert!(is_valid_passkey_name("my_key_1"));
        assert!(!is_valid_passkey_name("")); // 빈 값
        assert!(!is_valid_passkey_name("my key")); // 공백
        assert!(!is_valid_passkey_name("../etc/passwd")); // traversal
        assert!(!is_valid_passkey_name("a/b")); // 슬래시
        assert!(!is_valid_passkey_name("키")); // 비-ASCII
    }

    #[test]
    fn sanitize_replaces_and_never_empty() {
        assert_eq!(sanitize_passkey_name("gx10-key"), "gx10-key");
        assert_eq!(sanitize_passkey_name("my key"), "my_key");
        assert_eq!(sanitize_passkey_name("../etc"), "___etc");
        assert_eq!(sanitize_passkey_name("키"), "_"); // 전부 치환되어도 비지 않음
        // 결과는 항상 화이트리스트 통과
        for raw in ["내 서버", "a b/c", "x", ""] {
            assert!(is_valid_passkey_name(&sanitize_passkey_name(raw)) || sanitize_passkey_name(raw) == "_");
        }
    }

    #[test]
    fn passkeys_toml_roundtrip() {
        let mut pk = Passkeys::default();
        pk.upsert(Passkey { name: "k1".into(), kind: "path".into(), path: "~/.ssh/id".into() });
        let s = toml::to_string_pretty(&pk).unwrap();
        let back: Passkeys = toml::from_str(&s).unwrap();
        assert_eq!(back.get("k1").unwrap().path, "~/.ssh/id");
        assert_eq!(back.get("k1").unwrap().kind, "path");
    }

    #[test]
    fn upsert_path_rejects_bad_name() {
        let mut pk = Passkeys::default();
        assert!(pk.upsert_path("../x", "~/p").is_err());
        assert!(pk.upsert_path("ok-1", "~/p").is_ok());
        assert_eq!(pk.get("ok-1").unwrap().path, "~/p");
    }

    #[test]
    fn materialize_inline_writes_file_and_rejects_bad_name() {
        let dir = std::env::temp_dir().join(format!("tasty-pk-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // 정상
        let path = materialize_inline_in(&dir, "tok", "s3cr3t").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "s3cr3t");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // 잘못된 이름
        assert!(materialize_inline_in(&dir, "../escape", "x").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_managed_deletes_file() {
        let dir = std::env::temp_dir().join(format!("tasty-pk-rm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = materialize_inline_in(&dir, "tok", "x").unwrap();
        let mut pk = Passkeys::default();
        pk.upsert(Passkey {
            name: "tok".into(),
            kind: "inline".into(),
            path: path.to_string_lossy().into_owned(),
        });
        assert!(path.exists());
        assert!(pk.remove("tok"));
        assert!(!path.exists()); // 관리 파일 삭제됨
        assert!(!pk.remove("tok")); // 이미 없음
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_path_kind_keeps_file() {
        // path kind 는 사용자 소유라 파일을 지우지 않는다.
        let dir = std::env::temp_dir().join(format!("tasty-pk-keep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("user-key");
        fs::write(&file, "owned").unwrap();
        let mut pk = Passkeys::default();
        pk.upsert(Passkey {
            name: "ref".into(),
            kind: "path".into(),
            path: file.to_string_lossy().into_owned(),
        });
        assert!(pk.remove("ref"));
        assert!(file.exists()); // 참조 해제했을 뿐 파일 보존
        let _ = fs::remove_dir_all(&dir);
    }
}
