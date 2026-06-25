//! `ssh-profiles.toml` → `remote-profiles.toml` + `passkeys.toml` 마이그레이션.
//!
//! 구 SSH 프로필(attach 전용)을 범용 프로필(kind="ssh")로 옮기고, `identity_file` 을
//! path kind passkey 로 분리한다. 멱등 — `remote-profiles.toml` 이 이미 있으면 skip.
//!
//! 구 크레이트(`tasty-ssh-profiles`)에 의존하지 않도록 **legacy 스키마를 로컬에서
//! 최소 파싱**한다(이 크레이트가 구 크레이트보다 오래 살아남아야 하므로).

use std::collections::BTreeMap;
use std::fs;

use anyhow::Result;
use serde::Deserialize;

use crate::passkey::{Passkey, Passkeys, sanitize_passkey_name};
use crate::profile::{FieldValue, RemoteProfile, RemoteProfiles};

fn default_true() -> bool {
    true
}
fn default_remote_tasty() -> String {
    "tasty".to_string()
}
fn default_port_mode() -> String {
    "auto".to_string()
}
fn default_shell() -> String {
    "auto".to_string()
}

/// 구 `ssh-profiles.toml` 의 최소 파싱 미러(읽기 전용). `remote_command` 는 미사용
/// 예약 필드라 의도적으로 받지 않는다(드롭).
#[derive(Debug, Deserialize)]
struct LegacySshProfiles {
    #[serde(default, rename = "profile")]
    profiles: Vec<LegacySshProfile>,
}

#[derive(Debug, Deserialize)]
struct LegacySshProfile {
    name: String,
    #[serde(default)]
    label: Option<String>,
    host: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    identity_file: Option<String>,
    #[serde(default = "default_true")]
    use_agent: bool,
    #[serde(default)]
    extra_options: Vec<String>,
    #[serde(default = "default_remote_tasty")]
    remote_tasty: String,
    #[serde(default = "default_port_mode")]
    port_mode: String,
    #[serde(default = "default_shell")]
    shell: String,
    #[serde(default)]
    detect_failed: bool,
}

/// 충돌 안 나는 passkey 이름을 고른다(`base`, `base-2`, `base-3`, …).
fn unique_name(base: &str, used: &std::collections::BTreeSet<String>) -> String {
    if !used.contains(base) {
        return base.to_string();
    }
    let mut n = 2;
    loop {
        let cand = format!("{base}-{n}");
        if !used.contains(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// 구 toml 문자열을 파싱해 (프로필, passkey) 로 변환한다(순수 — 디스크 미접근).
/// `identity_file` 이 같은 프로필끼리는 한 passkey 로 dedup 한다.
pub fn parse_and_transform(legacy_toml: &str) -> Result<(RemoteProfiles, Passkeys)> {
    let legacy: LegacySshProfiles = toml::from_str(legacy_toml)?;

    let mut profiles = RemoteProfiles {
        version: 1,
        profiles: Vec::new(),
    };
    let mut passkeys = Passkeys {
        version: 1,
        passkeys: Vec::new(),
    };
    let mut by_path: BTreeMap<String, String> = BTreeMap::new(); // identity_file → passkey name
    let mut used: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for sp in legacy.profiles {
        // identity_file → path kind passkey(경로 기준 dedup).
        let passkey_ref = sp.identity_file.as_ref().map(|idf| {
            if let Some(existing) = by_path.get(idf) {
                existing.clone()
            } else {
                let base = sanitize_passkey_name(&format!("{}-key", sp.name));
                let name = unique_name(&base, &used);
                used.insert(name.clone());
                by_path.insert(idf.clone(), name.clone());
                passkeys.passkeys.push(Passkey {
                    name: name.clone(),
                    kind: "path".to_string(),
                    path: idf.clone(),
                });
                name
            }
        });

        let mut fields: BTreeMap<String, FieldValue> = BTreeMap::new();
        fields.insert("host".into(), sp.host.into());
        if let Some(p) = sp.port {
            fields.insert("port".into(), p.to_string().into());
        }
        if let Some(u) = sp.user {
            fields.insert("user".into(), u.into());
        }
        // use_agent: SshView 가 agent 위임 판단에 사용(기본 true 라 false 일 때만 의미가
        // 크지만, 명시 보존을 위해 항상 기록).
        fields.insert("use_agent".into(), sp.use_agent.to_string().into());
        if !sp.extra_options.is_empty() {
            fields.insert("extra_options".into(), FieldValue::List(sp.extra_options));
        }
        fields.insert("remote_tasty".into(), sp.remote_tasty.into());
        fields.insert("port_mode".into(), sp.port_mode.into());
        fields.insert("shell".into(), sp.shell.into());
        if sp.detect_failed {
            fields.insert("detect_failed".into(), "true".into());
        }

        profiles.profiles.push(RemoteProfile {
            name: sp.name,
            label: sp.label,
            kind: "ssh".to_string(),
            passkey_ref,
            fields,
        });
    }

    Ok((profiles, passkeys))
}

/// 디스크 마이그레이션(멱등). 변환·저장이 일어났으면 Ok(true).
///
/// 1. `remote-profiles.toml` 이 이미 있으면 skip(false).
/// 2. `ssh-profiles.toml` 이 없으면 skip(false — 신규 설치는 빈 상태로 시작).
/// 3. 변환 → `remote-profiles.toml` + `passkeys.toml` 저장 → 구파일을 `.bak` 으로 보존.
pub fn migrate_if_needed() -> Result<bool> {
    let Some(home) = tasty_utils::path::tasty_home() else {
        return Ok(false);
    };
    let new_path = home.join("remote-profiles.toml");
    if new_path.exists() {
        return Ok(false);
    }
    let legacy_path = home.join("ssh-profiles.toml");
    let Ok(legacy_toml) = fs::read_to_string(&legacy_path) else {
        return Ok(false);
    };

    let (profiles, passkeys) = parse_and_transform(&legacy_toml)?;
    // passkeys 먼저 저장(0600) — 프로필이 참조하므로.
    passkeys.save()?;
    profiles.save()?;
    // 모든 surface(attach/GUI/IPC/CLI)가 새 파일로 이관됐으므로 구파일을 `.bak` 으로
    // rename 해 보존(삭제 아님). 멱등성은 `remote-profiles.toml` 존재 검사가 보장.
    let bak = home.join("ssh-profiles.toml.bak");
    if let Err(e) = fs::rename(&legacy_path, &bak) {
        tracing::warn!("migrated ssh-profiles but failed to rename to .bak: {e}");
    }
    tracing::info!(
        "migrated {} ssh profile(s) → remote-profiles.toml ({} passkey(s))",
        profiles.profiles.len(),
        passkeys.passkeys.len()
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        version = 1

        [[profile]]
        name = "gx10"
        host = "gx10"
        user = "zilhak"
        port = 2222
        identity_file = "~/.ssh/id_ed25519"
        extra_options = ["ServerAliveInterval=30"]
        remote_tasty = "/usr/local/bin/tasty"
        port_mode = "file-unix"
        shell = "powershell"

        [[profile]]
        name = "agent-box"
        host = "box"
        use_agent = true

        [[profile]]
        name = "shared"
        host = "box2"
        identity_file = "~/.ssh/id_ed25519"
    "#;

    #[test]
    fn transform_maps_ssh_fields() {
        let (profiles, _) = parse_and_transform(SAMPLE).unwrap();
        let gx = profiles.get("gx10").unwrap();
        assert_eq!(gx.kind, "ssh");
        let v = gx.as_ssh().unwrap();
        assert_eq!(v.host(), Some("gx10"));
        assert_eq!(v.user(), Some("zilhak"));
        assert_eq!(v.port(), Some(2222));
        assert_eq!(v.remote_tasty(), "/usr/local/bin/tasty");
        assert_eq!(v.port_mode(), "file-unix");
        assert_eq!(v.shell(), "powershell");
        assert_eq!(
            v.extra_options(),
            vec!["ServerAliveInterval=30".to_string()]
        );
    }

    #[test]
    fn transform_creates_path_passkey_and_links() {
        let (profiles, passkeys) = parse_and_transform(SAMPLE).unwrap();
        let gx = profiles.get("gx10").unwrap();
        let pk_name = gx.passkey_ref.as_deref().unwrap();
        let pk = passkeys.get(pk_name).unwrap();
        assert_eq!(pk.kind, "path");
        assert_eq!(pk.path, "~/.ssh/id_ed25519");
    }

    #[test]
    fn transform_dedups_shared_identity_file() {
        let (profiles, passkeys) = parse_and_transform(SAMPLE).unwrap();
        // gx10 과 shared 가 같은 identity_file → 같은 passkey 하나만.
        let gx_ref = profiles
            .get("gx10")
            .unwrap()
            .passkey_ref
            .as_deref()
            .unwrap();
        let shared_ref = profiles
            .get("shared")
            .unwrap()
            .passkey_ref
            .as_deref()
            .unwrap();
        assert_eq!(gx_ref, shared_ref);
        assert_eq!(passkeys.passkeys.len(), 1);
    }

    #[test]
    fn transform_no_identity_file_has_no_passkey() {
        let (profiles, _) = parse_and_transform(SAMPLE).unwrap();
        assert!(profiles.get("agent-box").unwrap().passkey_ref.is_none());
        assert_eq!(
            profiles.get("agent-box").unwrap().as_ssh().unwrap().host(),
            Some("box")
        );
    }

    #[test]
    fn transform_roundtrips_through_toml() {
        // 변환 결과가 새 스키마로 저장/재로드 가능한지.
        let (profiles, passkeys) = parse_and_transform(SAMPLE).unwrap();
        let ptoml = toml::to_string_pretty(&profiles).unwrap();
        let back: RemoteProfiles = toml::from_str(&ptoml).unwrap();
        assert_eq!(back.profiles.len(), 3);
        let ktoml = toml::to_string_pretty(&passkeys).unwrap();
        let kback: Passkeys = toml::from_str(&ktoml).unwrap();
        assert_eq!(kback.passkeys.len(), 1);
    }

    #[test]
    fn empty_legacy_yields_empty() {
        let (profiles, passkeys) = parse_and_transform("version = 1").unwrap();
        assert!(profiles.profiles.is_empty());
        assert!(passkeys.passkeys.is_empty());
    }
}
