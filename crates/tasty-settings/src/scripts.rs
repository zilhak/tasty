//! 사용자 등록 Lua 스크립트 목록 저장소 + SHA256 (ADR-0031).
//!
//! 단축키 트리거(04)·관리 창(05)·TOFU 게이트(06)가 모두 이 "등록 목록" 을 전제로 한다.
//! `Settings.scripts` 로 `~/.tasty/config.toml` 에 영속된다.
//!
//! **단축키 combo 는 여기 저장하지 않는다.** 바인딩 소유권은 `KeybindingSettings`(04)에
//! 있고(`script_id` 로 참조), 관리 창은 그 값을 조회해 표시만 한다 — 저장 위치 이중화 방지.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 등록된 스크립트 1개.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptEntry {
    /// 안정적 고유 id. 04 바인딩이 이 값으로 스크립트를 참조하므로 제거 후 재사용하지 않는다.
    pub id: String,
    /// 표시 이름. 미지정 시 파일명으로 대체(호출자 책임).
    pub name: String,
    /// 스크립트 파일 절대 경로.
    pub path: PathBuf,
    /// 등록/승인 시점의 **엔트리 파일** SHA256(hex). TOFU(06) 기준값.
    /// transitive `require` 의존 파일은 커버하지 않는다(ADR-0031 한계).
    pub sha256: String,
}

/// 스크립트 목록 저장소. `Settings.scripts` 로 config 에 영속.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptRegistry {
    #[serde(default)]
    scripts: Vec<ScriptEntry>,
    /// 다음 id 시퀀스. 제거 후 재사용 방지(04 바인딩 안정성).
    #[serde(default)]
    next_id: u64,
}

impl ScriptRegistry {
    /// 등록된 스크립트 순회.
    pub fn iter(&self) -> impl Iterator<Item = &ScriptEntry> {
        self.scripts.iter()
    }

    /// 등록 개수.
    pub fn len(&self) -> usize {
        self.scripts.len()
    }

    /// 비었는지.
    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }

    /// id 로 조회.
    pub fn get(&self, id: &str) -> Option<&ScriptEntry> {
        self.scripts.iter().find(|s| s.id == id)
    }

    /// 새 스크립트 등록. id 를 자동 생성해 반환한다.
    pub fn add(&mut self, name: String, path: PathBuf, sha256: String) -> String {
        let id = format!("script-{}", self.next_id);
        self.next_id += 1;
        self.scripts.push(ScriptEntry {
            id: id.clone(),
            name,
            path,
            sha256,
        });
        id
    }

    /// id 로 제거. 존재해서 제거했으면 true.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.scripts.len();
        self.scripts.retain(|s| s.id != id);
        self.scripts.len() != before
    }

    /// 이름 변경(05). 존재했으면 true.
    pub fn rename(&mut self, id: &str, name: String) -> bool {
        match self.scripts.iter_mut().find(|s| s.id == id) {
            Some(e) => {
                e.name = name;
                true
            }
            None => false,
        }
    }

    /// 승인 시 해시 갱신(06 TOFU). 존재했으면 true.
    pub fn update_hash(&mut self, id: &str, sha256: String) -> bool {
        match self.scripts.iter_mut().find(|s| s.id == id) {
            Some(e) => {
                e.sha256 = sha256;
                true
            }
            None => false,
        }
    }
}

/// 파일 내용의 SHA256 을 hex 문자열로 계산. **엔트리 파일만** 해시한다.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(hash_bytes(&bytes))
}

/// 바이트열의 SHA256 을 소문자 hex 문자열로.
pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_bytes_matches_known_vector() {
        // SHA256("abc") = ba7816bf...
        assert_eq!(
            hash_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hash_file_reads_content() {
        let dir = std::env::temp_dir().join(format!("tasty-script-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("s.lua");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(hash_file(&path).unwrap(), hash_bytes(b"abc"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_generates_unique_stable_ids() {
        let mut reg = ScriptRegistry::default();
        let id1 = reg.add("a".into(), PathBuf::from("/tmp/a.lua"), "h1".into());
        let id2 = reg.add("b".into(), PathBuf::from("/tmp/b.lua"), "h2".into());
        assert_ne!(id1, id2);
        // 제거 후 재등록해도 이전 id 를 재사용하지 않는다.
        reg.remove(&id1);
        let id3 = reg.add("c".into(), PathBuf::from("/tmp/c.lua"), "h3".into());
        assert_ne!(id3, id1);
        assert_ne!(id3, id2);
    }

    #[test]
    fn crud_roundtrip() {
        let mut reg = ScriptRegistry::default();
        let id = reg.add("orig".into(), PathBuf::from("/tmp/x.lua"), "oldhash".into());
        assert_eq!(reg.len(), 1);
        assert!(reg.rename(&id, "renamed".into()));
        assert!(reg.update_hash(&id, "newhash".into()));
        let e = reg.get(&id).unwrap();
        assert_eq!(e.name, "renamed");
        assert_eq!(e.sha256, "newhash");
        assert!(reg.remove(&id));
        assert!(reg.is_empty());
        assert!(!reg.remove(&id));
    }

    #[test]
    fn serde_toml_roundtrip_preserves_entries() {
        let mut reg = ScriptRegistry::default();
        reg.add(
            "hello".into(),
            PathBuf::from("/home/u/hello.lua"),
            "abc123".into(),
        );
        let s = toml::to_string(&reg).unwrap();
        let back: ScriptRegistry = toml::from_str(&s).unwrap();
        assert_eq!(back.len(), 1);
        let e = back.iter().next().unwrap();
        assert_eq!(e.name, "hello");
        assert_eq!(e.sha256, "abc123");
    }

    #[test]
    fn deserializes_empty_to_default() {
        // scripts 키가 없는 기존 config 조각 → 빈 레지스트리(마이그레이션 안전).
        let reg: ScriptRegistry = toml::from_str("").unwrap();
        assert!(reg.is_empty());
    }
}
