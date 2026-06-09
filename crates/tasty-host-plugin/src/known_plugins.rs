//! `~/.tasty/known-plugins.toml` — 사용자가 trust 한 외부 plugin 의 trust DB.
//!
//! 임베드 키 (`bundle_sig::TRUSTED_PUBKEYS`) 에 매칭되지 않는 plugin 의 경우,
//! 사용자가 모달에서 "신뢰" 를 누른 시점의 *pubkey + 권한 스냅샷* 을 본 파일에
//! 기록한다. 다음 구동부터는 모달 없이 자동 trust. 단, 매니페스트의 권한이
//! trust 시점과 달라졌으면 다시 모달이 뜨도록 [`Self::permissions_changed`] 가
//! 변경을 감지한다 (TOFU + permission-creep 방어).
//!
//! ## 파일 포맷 (TOML)
//!
//! ```toml
//! [plugins."com.example.myplugin"]
//! pubkey = "BASE64_OF_32_BYTE_ED25519_PUBKEY"
//! permissions = ["filesystem.read", "network"]
//! trusted_at = "2026-06-09T09:50:00Z"
//! publisher_fingerprint = "ab:cd:ef:..."
//! ```
//!
//! ## 신뢰성 / 동시성
//!
//! 본 파일은 사용자 머신의 *user-only* DB. release 빌드의 어떤 IPC 도 직접
//! 쓰기를 노출하지 않는다 (사용자 모달 승인 결과를 통해서만 갱신). 동시성은
//! 단순 `read → mutate → write` 로 race 가능하지만, plugin 등록 빈도가 매우
//! 낮아 lock 은 0.7+ 로 미룬다.

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};

/// 사용자 trust DB 의 전체 내용 — `~/.tasty/known-plugins.toml` 매핑.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct KnownPlugins {
    /// plugin_id → entry. `BTreeMap` 으로 toml dump 시 순서 안정.
    #[serde(default)]
    plugins: BTreeMap<String, KnownPluginEntry>,
}

/// 단일 plugin 의 trust 항목.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KnownPluginEntry {
    /// base64-encoded 32-byte ed25519 pubkey.
    pub pubkey: String,
    /// 사용자가 trust 한 시점의 매니페스트 권한 목록 (변경 감지용).
    #[serde(default)]
    pub permissions: Vec<String>,
    /// RFC3339 timestamp (예: `2026-06-09T09:50:00Z`).
    pub trusted_at: String,
    /// publisher 가 별도 채널 (공식 사이트 / 패키지 페이지) 로 게시한 키
    /// fingerprint. 사용자가 모달에서 수동 비교한 흔적 — 향후 자동 비교 활용.
    #[serde(default)]
    pub publisher_fingerprint: String,
}

impl KnownPluginEntry {
    /// base64 pubkey 를 raw 32 byte 로 디코드. 형식 오류면 None.
    pub fn pubkey_bytes(&self) -> Option<[u8; 32]> {
        let raw = BASE64.decode(self.pubkey.as_bytes()).ok()?;
        if raw.len() != 32 {
            return None;
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&raw);
        Some(out)
    }

    /// raw 32 byte pubkey 를 base64 로 인코드.
    pub fn encode_pubkey(pk: &[u8; 32]) -> String {
        BASE64.encode(pk)
    }
}

impl KnownPlugins {
    /// `~/.tasty/known-plugins.toml` 경로. home 디렉토리 결정 실패 시 None.
    pub fn path() -> Option<PathBuf> {
        tasty_utils::path::tasty_home().map(|h| h.join("known-plugins.toml"))
    }

    /// 파일을 읽어 파싱. 파일이 없으면 빈 DB 반환 (Ok(default)). 다른 io 에러는
    /// 그대로 전파.
    pub fn load() -> std::io::Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str::<Self>(&text)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// 파일에 atomic write — `<path>.tmp` 에 쓴 후 rename. 부모 디렉토리가 없으면
    /// 생성한다.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Err(std::io::Error::other(
                "tasty_home unavailable; cannot persist known-plugins.toml",
            ));
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, serialized)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// plugin_id 로 entry 조회. 없으면 None.
    pub fn lookup(&self, plugin_id: &str) -> Option<&KnownPluginEntry> {
        self.plugins.get(plugin_id)
    }

    /// 매니페스트의 현재 권한 목록이 trust 시점과 다른지 — 순서 무시 set 비교.
    /// plugin_id 자체가 없으면 *변경됨* 으로 간주 (true) — 호출처에서는 lookup
    /// 결과를 먼저 확인해야 의미 있다.
    pub fn permissions_changed(&self, plugin_id: &str, new_perms: &[String]) -> bool {
        let Some(entry) = self.plugins.get(plugin_id) else {
            return true;
        };
        let mut a: Vec<&str> = entry.permissions.iter().map(|s| s.as_str()).collect();
        let mut b: Vec<&str> = new_perms.iter().map(|s| s.as_str()).collect();
        a.sort_unstable();
        b.sort_unstable();
        a != b
    }

    /// 신규 trust 항목 추가 / 기존 항목 덮어쓰기. `save` 는 호출처가 별도로.
    pub fn add(&mut self, plugin_id: String, entry: KnownPluginEntry) {
        self.plugins.insert(plugin_id, entry);
    }

    /// 테스트 / 진단용 — 전체 plugin_id 목록.
    pub fn plugin_ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.keys().map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_for(pk_byte: u8, perms: &[&str]) -> KnownPluginEntry {
        let pk = [pk_byte; 32];
        KnownPluginEntry {
            pubkey: KnownPluginEntry::encode_pubkey(&pk),
            permissions: perms.iter().map(|s| s.to_string()).collect(),
            trusted_at: "2026-06-09T09:50:00Z".into(),
            publisher_fingerprint: String::new(),
        }
    }

    #[test]
    fn encode_decode_pubkey_round_trip() {
        let pk = [0xABu8; 32];
        let enc = KnownPluginEntry::encode_pubkey(&pk);
        let entry = KnownPluginEntry {
            pubkey: enc,
            permissions: vec![],
            trusted_at: "2026-06-09T09:50:00Z".into(),
            publisher_fingerprint: String::new(),
        };
        assert_eq!(entry.pubkey_bytes(), Some(pk));
    }

    #[test]
    fn pubkey_bytes_rejects_wrong_length() {
        let entry = KnownPluginEntry {
            pubkey: BASE64.encode(b"short"),
            permissions: vec![],
            trusted_at: "2026-06-09T09:50:00Z".into(),
            publisher_fingerprint: String::new(),
        };
        assert_eq!(entry.pubkey_bytes(), None);
    }

    #[test]
    fn lookup_returns_added_entry() {
        let mut db = KnownPlugins::default();
        db.add(
            "com.example.foo".into(),
            entry_for(0x11, &["filesystem.read"]),
        );
        assert!(db.lookup("com.example.foo").is_some());
        assert!(db.lookup("com.example.missing").is_none());
    }

    #[test]
    fn permissions_changed_detects_addition() {
        let mut db = KnownPlugins::default();
        db.add("p".into(), entry_for(0x11, &["a", "b"]));
        // 동일 set (순서 무관) → 변경 없음.
        assert!(!db.permissions_changed("p", &["b".into(), "a".into()]));
        // 신규 권한 추가 → 변경.
        assert!(db.permissions_changed("p", &["a".into(), "b".into(), "c".into()]));
        // 권한 제거 → 변경.
        assert!(db.permissions_changed("p", &["a".into()]));
    }

    #[test]
    fn permissions_changed_missing_plugin_is_true() {
        let db = KnownPlugins::default();
        assert!(db.permissions_changed("unknown", &[]));
    }

    #[test]
    fn toml_round_trip_via_string() {
        let mut db = KnownPlugins::default();
        db.add(
            "com.example.foo".into(),
            entry_for(0x22, &["network", "filesystem.write"]),
        );
        let dumped = toml::to_string_pretty(&db).unwrap();
        let parsed: KnownPlugins = toml::from_str(&dumped).unwrap();
        let e = parsed.lookup("com.example.foo").unwrap();
        assert_eq!(e.pubkey_bytes(), Some([0x22u8; 32]));
        assert_eq!(e.permissions.len(), 2);
    }
}
