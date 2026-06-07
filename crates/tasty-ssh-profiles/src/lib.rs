//! SSH 연결 프로필 저장소 — `~/.tasty/ssh-profiles.toml` (attach/detach 단계 7).
//!
//! 워크스페이스↔컴퓨터 매핑(`tasty-model::WorkspaceAttachMapping`)이 프로필 `name`
//! 으로 참조하는 *장비 인벤토리*. 자동 attach(호스트) 와 `tasty attach --profile`(CLI)
//! 이 이 프로필을 resolve 해 SSH 터널(`tasty_cli::ssh`)을 세운다.
//!
//! 설계(plan-v2 §3.1):
//! - **별도 파일** — `config.toml`(`Settings::save()` 전체 덮어쓰기)과 분리해 손편집/
//!   동기화 충돌 0. `Settings::load/save` 와 동형 패턴(`tasty_home()` 기반, 없으면
//!   default, 파싱 실패 시 default+warn, save 는 pretty TOML 전체 쓰기).
//! - **비밀번호 미저장**(decisions 5) — 인증은 identity_file(`-i`) 또는 ssh-agent
//!   위임(`use_agent=true` 기본). passphrase 도 tasty 가 보관하지 않는다(agent 처리).

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tasty_utils::path::tasty_home;

/// 한 SSH 접속 대상의 저장 프로필. `host` 는 ssh destination(`user@host` / alias 도
/// 허용)을 그대로 ssh 에 위임할 수 있다 — tasty 는 파싱하지 않는다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshProfile {
    /// 고유 식별자(워크스페이스 매핑이 이 name 을 참조).
    pub name: String,
    /// UI 표시용 라벨(옵션, 사용자 자유 입력 — i18n 대상 아님).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// ssh destination: `host` | `user@host` | `~/.ssh/config` alias.
    pub host: String,
    /// ssh 포트(`-p`). None 이면 ssh config / 22 위임.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// ssh 유저(`host` 에 이미 `user@` 가 있으면 불필요). None 이면 ssh 현재 유저.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// identity 파일 경로(`-i`). None + `use_agent` 면 agent 위임.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    /// ssh-agent / Windows OpenSSH Agent 위임(권장 기본 true).
    #[serde(default = "default_true")]
    pub use_agent: bool,
    /// 추가 ssh `-o` 옵션. `"Key=Value"` → `-o Key=Value`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_options: Vec<String>,
    /// 원격 진입 직후 실행할 명령(예약 — 현재 mirror 경로 미사용).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_command: Option<String>,
    /// 원격 tasty 바이너리 경로(`ssh host <path> port` 포트 발견용). 기본 `"tasty"`.
    #[serde(default = "default_remote_tasty")]
    pub remote_tasty: String,
    /// 원격 포트 발견 모드: `auto`(기본) | `subcommand` | `file-unix` | `file-windows`.
    #[serde(default = "default_port_mode")]
    pub port_mode: String,
}

fn default_true() -> bool {
    true
}
fn default_remote_tasty() -> String {
    "tasty".into()
}
fn default_port_mode() -> String {
    "auto".into()
}

impl SshProfile {
    /// 최소 필드(name + host)로 프로필을 만든다(나머지는 기본값).
    pub fn new(name: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: None,
            host: host.into(),
            port: None,
            user: None,
            identity_file: None,
            use_agent: true,
            extra_options: Vec::new(),
            remote_command: None,
            remote_tasty: default_remote_tasty(),
            port_mode: default_port_mode(),
        }
    }

    /// ssh destination 문자열(`user@host` 합성). `host` 에 이미 `user@` 가 있거나
    /// `user` 가 None 이면 `host` 를 그대로 쓴다.
    pub fn ssh_destination(&self) -> String {
        match &self.user {
            Some(u) if !self.host.contains('@') => format!("{u}@{}", self.host),
            _ => self.host.clone(),
        }
    }
}

/// `~/.tasty/ssh-profiles.toml` 전체 — 프로필 목록 + 스키마 버전.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SshProfiles {
    #[serde(default)]
    pub version: u32,
    #[serde(default, rename = "profile")]
    pub profiles: Vec<SshProfile>,
}

impl SshProfiles {
    /// 저장 파일 경로: `~/.tasty/ssh-profiles.toml`.
    pub fn path() -> Option<PathBuf> {
        tasty_home().map(|dir| dir.join("ssh-profiles.toml"))
    }

    /// 저장 디렉토리(`~/.tasty/`) 보장.
    fn ensure_dir() -> Result<()> {
        if let Some(path) = Self::path()
            && let Some(parent) = path.parent()
        {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// 프로필을 로드한다. 파일이 없거나 파싱 실패면 빈 목록(default)로 폴백한다
    /// (`Settings::load` 와 동형 — 잘못된 파일이 부팅을 막지 않는다).
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            tracing::info!("no ssh-profiles path available, using empty profile list");
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<SshProfiles>(&contents) {
                Ok(p) => {
                    tracing::info!("loaded {} ssh profile(s) from {}", p.profiles.len(), path.display());
                    p
                }
                Err(e) => {
                    tracing::warn!("failed to parse ssh-profiles file: {e}, using empty list");
                    Self::default()
                }
            },
            Err(_) => {
                tracing::info!("no ssh-profiles file at {}, using empty list", path.display());
                Self::default()
            }
        }
    }

    /// pretty TOML 로 전체를 덮어쓴다(`version` 미설정이면 1 로 채운다).
    pub fn save(&self) -> Result<()> {
        Self::ensure_dir()?;
        let Some(path) = Self::path() else {
            anyhow::bail!("could not determine ssh-profiles path");
        };
        let mut to_write = self.clone();
        if to_write.version == 0 {
            to_write.version = 1;
        }
        let contents = toml::to_string_pretty(&to_write)?;
        fs::write(&path, contents)?;
        tracing::info!("saved {} ssh profile(s) to {}", to_write.profiles.len(), path.display());
        Ok(())
    }

    /// 이름으로 프로필 조회.
    pub fn get(&self, name: &str) -> Option<&SshProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// name 이 같은 프로필이 있으면 교체, 없으면 추가.
    pub fn upsert(&mut self, profile: SshProfile) {
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.name == profile.name) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
    }

    /// name 으로 프로필을 제거한다. 제거됐으면 true.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.name != name);
        self.profiles.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_replaces_same_name() {
        let mut ps = SshProfiles::default();
        ps.upsert(SshProfile::new("a", "host-1"));
        ps.upsert(SshProfile::new("a", "host-2")); // 같은 name → 교체
        assert_eq!(ps.profiles.len(), 1);
        assert_eq!(ps.get("a").unwrap().host, "host-2");
    }

    #[test]
    fn get_and_remove() {
        let mut ps = SshProfiles::default();
        ps.upsert(SshProfile::new("a", "ha"));
        ps.upsert(SshProfile::new("b", "hb"));
        assert!(ps.get("a").is_some());
        assert!(ps.remove("a"));
        assert!(!ps.remove("a")); // 이미 없음
        assert!(ps.get("a").is_none());
        assert_eq!(ps.profiles.len(), 1);
    }

    #[test]
    fn ssh_destination_synthesizes_user() {
        let mut p = SshProfile::new("a", "192.168.0.10");
        assert_eq!(p.ssh_destination(), "192.168.0.10");
        p.user = Some("zilhak".into());
        assert_eq!(p.ssh_destination(), "zilhak@192.168.0.10");
        // host 에 이미 user@ 가 있으면 user 무시.
        p.host = "root@box".into();
        assert_eq!(p.ssh_destination(), "root@box");
    }

    #[test]
    fn toml_roundtrip_preserves_fields() {
        let mut ps = SshProfiles::default();
        let mut p = SshProfile::new("gx10", "gx10");
        p.user = Some("zilhak".into());
        p.port = Some(2222);
        p.identity_file = Some("~/.ssh/id_ed25519".into());
        p.extra_options = vec!["ServerAliveInterval=30".into()];
        p.remote_tasty = "/usr/local/bin/tasty".into();
        p.port_mode = "file-unix".into();
        ps.upsert(p);
        let toml_str = toml::to_string_pretty(&ps).unwrap();
        let back: SshProfiles = toml::from_str(&toml_str).unwrap();
        let bp = back.get("gx10").unwrap();
        assert_eq!(bp.user.as_deref(), Some("zilhak"));
        assert_eq!(bp.port, Some(2222));
        assert_eq!(bp.identity_file.as_deref(), Some("~/.ssh/id_ed25519"));
        assert_eq!(bp.extra_options, vec!["ServerAliveInterval=30".to_string()]);
        assert_eq!(bp.remote_tasty, "/usr/local/bin/tasty");
        assert_eq!(bp.port_mode, "file-unix");
        assert!(bp.use_agent); // default true
    }

    #[test]
    fn defaults_applied_on_minimal_toml() {
        let toml_str = r#"
            [[profile]]
            name = "a"
            host = "h"
        "#;
        let ps: SshProfiles = toml::from_str(toml_str).unwrap();
        let p = ps.get("a").unwrap();
        assert!(p.use_agent);
        assert_eq!(p.remote_tasty, "tasty");
        assert_eq!(p.port_mode, "auto");
        assert!(p.port.is_none());
    }
}
