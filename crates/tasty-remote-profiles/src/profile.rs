//! 원격 접속 프로필 저장소 — `~/.tasty/remote-profiles.toml`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tasty_utils::path::tasty_home;

/// core 가 직접 소비하는 내장 타입. `http` 등은 여기 없다 — 플러그인이 manifest 로
/// 선언한다. 유효 known 집합 = `BUILTIN_KINDS` ∪ {설치 플러그인 선언 타입}(런타임 계산,
/// 상위에서). `smb` 소비자(explorer mount)는 추후지만 의도된 내장 타입이라 미리 등록.
pub const BUILTIN_KINDS: &[&str] = &["ssh", "tasty-attach", "smb"];

/// core 내장 타입인지. 미등록(노란 배지) 최종 판정은 런타임 집합 기준(상위)이며 이
/// 함수는 core 내장 여부만 답한다.
pub fn is_builtin_kind(kind: &str) -> bool {
    BUILTIN_KINDS.contains(&kind)
}

/// ssh kind 의 `shell` 필드가 가질 수 있는 값(GUI/CLI 드롭다운·검증 공용). `auto` 는
/// 자동감지. ssh 도메인 지식이라 이 크레이트가 단일 출처로 보관한다.
pub const SHELLS: &[&str] = &["powershell", "cmd", "bash", "zsh", "auto"];

/// 셸 문자열이 허용 값인지.
pub fn is_valid_shell(s: &str) -> bool {
    SHELLS.contains(&s)
}

/// 셸 종류 → 원격 포트 발견 모드 매핑(2026-06-12 실측 기반).
///
/// - `powershell` → `file-unix` (`cat ~/...` — PowerShell 의 cat alias + `~` 확장)
/// - `cmd` → `file-windows` (`type %USERPROFILE%\...` — cmd 전용)
/// - `bash` / `zsh` → `subcommand` (`tasty port` — unix·git bash 성공)
/// - `auto` / 알 수 없는 값 → `None` (자동감지 또는 fallback 체인 필요)
pub fn shell_to_port_mode(shell: &str) -> Option<&'static str> {
    match shell {
        "powershell" => Some("file-unix"),
        "cmd" => Some("file-windows"),
        "bash" | "zsh" => Some("subcommand"),
        _ => None,
    }
}

/// 필드 값 = 스칼라 string 또는 string 리스트. TOML 의 스칼라/배열에 네이티브 매핑되어
/// `extra_options` 같은 리스트도 인코딩 꼼수 없이 그대로 표현된다.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FieldValue {
    Str(String),
    List(Vec<String>),
}

impl FieldValue {
    /// 스칼라면 `&str`, 리스트면 None.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FieldValue::Str(s) => Some(s.as_str()),
            FieldValue::List(_) => None,
        }
    }

    /// 리스트면 슬라이스, 스칼라면 None.
    pub fn as_list(&self) -> Option<&[String]> {
        match self {
            FieldValue::List(v) => Some(v),
            FieldValue::Str(_) => None,
        }
    }
}

impl From<&str> for FieldValue {
    fn from(s: &str) -> Self {
        FieldValue::Str(s.to_string())
    }
}
impl From<String> for FieldValue {
    fn from(s: String) -> Self {
        FieldValue::Str(s)
    }
}
impl From<Vec<String>> for FieldValue {
    fn from(v: Vec<String>) -> Self {
        FieldValue::List(v)
    }
}

/// 한 원격 연결 디스크립터. 비밀 없음 — passkey 는 이름으로 참조한다.
///
/// `fields`(타입별 자유 필드)는 TOML 직렬화상 sub-table 이 되므로 **구조체 마지막**에
/// 둔다(스칼라 필드가 sub-table 뒤에 오면 TOML 직렬화가 깨진다).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteProfile {
    /// 고유 식별자(워크스페이스 매핑·소비자가 참조).
    pub name: String,
    /// UI 표시용 라벨(옵션, 사용자 자유 입력 — i18n 대상 아님).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 타입 태그(열린 string). 알려진: core 내장 `ssh`/`smb` + 플러그인 선언.
    pub kind: String,
    /// 참조 passkey name(없으면 무인증 / ssh-agent 위임).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passkey_ref: Option<String>,
    /// 타입별 자유 필드(열린 스키마). **마지막 필드 유지**(위 주석 참조).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, FieldValue>,
}

impl RemoteProfile {
    /// 빈 fields 로 프로필을 만든다.
    pub fn new(name: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: None,
            kind: kind.into(),
            passkey_ref: None,
            fields: BTreeMap::new(),
        }
    }

    /// fields 빌더(체이닝). 스칼라/리스트 모두 `Into<FieldValue>`.
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<FieldValue>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    /// fields 의 한 키를 설정(가변).
    pub fn set_field(&mut self, key: impl Into<String>, value: impl Into<FieldValue>) {
        self.fields.insert(key.into(), value.into());
    }

    /// fields 에서 한 키를 제거(가변). 제거됐으면 true.
    pub fn remove_field(&mut self, key: &str) -> bool {
        self.fields.remove(key).is_some()
    }

    /// `kind` 이 core 내장인지(런타임 known 집합 판정은 상위에서).
    pub fn is_builtin_kind(&self) -> bool {
        is_builtin_kind(&self.kind)
    }

    /// ssh 타입이면 typed view. 그 외 None.
    pub fn as_ssh(&self) -> Option<SshView<'_>> {
        (self.kind == "ssh").then_some(SshView(self))
    }

    /// tasty-attach 타입이면 typed view. 그 외 None.
    pub fn as_attach(&self) -> Option<AttachView<'_>> {
        (self.kind == "tasty-attach").then_some(AttachView(self))
    }
}

/// ssh kind 프로필의 typed view — `fields` 맵에서 ssh 필드를 안전 추출한다.
/// (모델은 열린 스키마라 값이 없거나 형식이 틀리면 기본값/None + warn 으로 흡수.)
pub struct SshView<'a>(pub &'a RemoteProfile);

impl<'a> SshView<'a> {
    fn field_str(&self, key: &str) -> Option<&'a str> {
        self.0.fields.get(key).and_then(FieldValue::as_str)
    }

    fn field_bool(&self, key: &str, default: bool) -> bool {
        match self.field_str(key) {
            Some("true") => true,
            Some("false") => false,
            _ => default,
        }
    }

    /// ssh destination host(`host` | `user@host` | alias). tasty 는 파싱하지 않는다.
    pub fn host(&self) -> Option<&'a str> {
        self.field_str("host")
    }

    /// ssh 포트(`-p`). 파싱 실패면 None + warn.
    pub fn port(&self) -> Option<u16> {
        let raw = self.field_str("port")?;
        match raw.parse::<u16>() {
            Ok(p) => Some(p),
            Err(_) => {
                tracing::warn!(
                    "remote profile '{}': invalid port '{}', ignoring",
                    self.0.name,
                    raw
                );
                None
            }
        }
    }

    /// ssh 유저(`host` 에 `user@` 가 있으면 불필요).
    pub fn user(&self) -> Option<&'a str> {
        self.field_str("user")
    }

    /// ssh-agent 위임(기본 true — 필드 없으면 위임).
    pub fn use_agent(&self) -> bool {
        self.field_bool("use_agent", true)
    }

    /// 추가 ssh `-o` 옵션. 스칼라로 저장된 단일 값도 1-원소 리스트로 흡수.
    pub fn extra_options(&self) -> Vec<String> {
        match self.0.fields.get("extra_options") {
            Some(FieldValue::List(v)) => v.clone(),
            Some(FieldValue::Str(s)) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    /// 원격 셸 종류. 기본 `"auto"`.
    pub fn shell(&self) -> &'a str {
        self.field_str("shell").unwrap_or("auto")
    }

    /// `shell=auto` 감지가 전 프로브 실패한 상태(true 면 attach 거부).
    pub fn detect_failed(&self) -> bool {
        self.field_bool("detect_failed", false)
    }

    /// ssh destination 문자열(`user@host` 합성).
    pub fn ssh_destination(&self) -> String {
        let host = self.host().unwrap_or("");
        match self.user() {
            Some(u) if !host.contains('@') => format!("{u}@{host}"),
            _ => host.to_string(),
        }
    }

    /// 감지 실패로 **비활성**인지. 비활성 프로필은 attach 가 거부한다.
    pub fn is_disabled(&self) -> bool {
        self.detect_failed()
    }
}

/// tasty-attach kind 프로필의 typed view.
///
/// attach 는 ssh 연결 정보를 **참조(ref)** 하거나 **인라인** 으로 보유한다:
/// - `ssh_ref` 필드가 있으면 **ref 모드** — 연결 정보는 참조된 ssh 프로필에서
///   resolve 시점에 로드하고, 이 view 의 인라인 접근자(host/user/…)는 None/기본값.
/// - `ssh_ref` 가 없으면 **인라인 모드** — 자기 `fields` 의 ssh 정보를 [`SshView`]
///   로직 그대로 재사용해 노출한다(중복 구현 방지).
///
/// attach 전용 필드(`remote_tasty`/`port_mode`/`port_file`)는 모드와 무관하게
/// 항상 tasty-attach 프로필 자신이 소유한다.
pub struct AttachView<'a>(pub &'a RemoteProfile);

impl<'a> AttachView<'a> {
    fn field_str(&self, key: &str) -> Option<&'a str> {
        self.0.fields.get(key).and_then(FieldValue::as_str)
    }

    /// 참조하는 ssh 프로필 name. 있으면 ref 모드, 없으면 인라인 모드.
    pub fn ssh_ref(&self) -> Option<&'a str> {
        self.field_str("ssh_ref")
    }

    /// 인라인 모드일 때만 자기 fields 를 읽는 SshView(ref 모드면 None).
    fn inline(&self) -> Option<SshView<'a>> {
        self.ssh_ref().is_none().then_some(SshView(self.0))
    }

    /// 인라인 ssh host(ref 모드면 None — 연결 정보는 참조 ssh 에서 온다).
    pub fn host(&self) -> Option<&'a str> {
        self.inline().and_then(|s| s.host())
    }

    /// 인라인 ssh 유저(ref 모드면 None).
    pub fn user(&self) -> Option<&'a str> {
        self.inline().and_then(|s| s.user())
    }

    /// 인라인 ssh 포트(ref 모드면 None).
    pub fn port(&self) -> Option<u16> {
        self.inline().and_then(|s| s.port())
    }

    /// 인라인 ssh-agent 위임(ref 모드면 기본 true).
    pub fn use_agent(&self) -> bool {
        self.inline().map(|s| s.use_agent()).unwrap_or(true)
    }

    /// 인라인 추가 ssh `-o` 옵션(ref 모드면 빈 vec).
    pub fn extra_options(&self) -> Vec<String> {
        self.inline().map(|s| s.extra_options()).unwrap_or_default()
    }

    /// 인라인 ssh destination(`user@host` 합성, ref 모드면 빈 문자열).
    pub fn ssh_destination(&self) -> String {
        self.inline()
            .map(|s| s.ssh_destination())
            .unwrap_or_default()
    }

    /// 인라인 원격 셸 종류(ref 모드면 기본 `"auto"`).
    pub fn shell(&self) -> &'a str {
        self.inline().map(|s| s.shell()).unwrap_or("auto")
    }

    /// 인라인 셸 감지 실패 상태(ref 모드면 false — 판정은 resolve 가 참조 ssh 에서).
    pub fn detect_failed(&self) -> bool {
        self.inline().map(|s| s.detect_failed()).unwrap_or(false)
    }

    /// 원격 tasty 바이너리 경로(포트 발견용). 기본 `"tasty"`.
    pub fn remote_tasty(&self) -> &'a str {
        self.field_str("remote_tasty").unwrap_or("tasty")
    }

    /// 원격 포트 발견 모드. 기본 `"auto"`.
    pub fn port_mode(&self) -> &'a str {
        self.field_str("port_mode").unwrap_or("auto")
    }

    /// 원격 port 파일의 명시 경로(없으면 None — discovery 가 관례 경로 탐색).
    pub fn port_file(&self) -> Option<&'a str> {
        self.field_str("port_file")
    }
}

/// `~/.tasty/remote-profiles.toml` 전체 — 프로필 목록 + 스키마 버전.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct RemoteProfiles {
    #[serde(default)]
    pub version: u32,
    #[serde(default, rename = "profile")]
    pub profiles: Vec<RemoteProfile>,
}

impl RemoteProfiles {
    /// 저장 파일 경로: `~/.tasty/remote-profiles.toml`.
    pub fn path() -> Option<PathBuf> {
        tasty_home().map(|dir| dir.join("remote-profiles.toml"))
    }

    fn ensure_dir() -> Result<()> {
        if let Some(path) = Self::path()
            && let Some(parent) = path.parent()
        {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    /// 로드한다. 파일이 없거나 파싱 실패면 빈 목록(default)으로 폴백한다
    /// (`Settings::load` 와 동형 — 잘못된 파일이 부팅을 막지 않는다).
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            tracing::info!("no remote-profiles path available, using empty list");
            return Self::default();
        };
        match fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<RemoteProfiles>(&contents) {
                Ok(p) => {
                    tracing::info!(
                        "loaded {} remote profile(s) from {}",
                        p.profiles.len(),
                        path.display()
                    );
                    p
                }
                Err(e) => {
                    tracing::warn!("failed to parse remote-profiles file: {e}, using empty list");
                    Self::default()
                }
            },
            Err(_) => {
                tracing::info!(
                    "no remote-profiles file at {}, using empty list",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// pretty TOML 로 전체를 덮어쓴다(`version` 미설정이면 1 로 채운다).
    pub fn save(&self) -> Result<()> {
        Self::ensure_dir()?;
        let Some(path) = Self::path() else {
            anyhow::bail!("could not determine remote-profiles path");
        };
        let mut to_write = self.clone();
        if to_write.version == 0 {
            to_write.version = 1;
        }
        let contents = toml::to_string_pretty(&to_write)?;
        fs::write(&path, contents)?;
        tracing::info!(
            "saved {} remote profile(s) to {}",
            to_write.profiles.len(),
            path.display()
        );
        Ok(())
    }

    /// 이름으로 프로필 조회.
    pub fn get(&self, name: &str) -> Option<&RemoteProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// name 이 같으면 교체, 없으면 추가.
    pub fn upsert(&mut self, profile: RemoteProfile) {
        if let Some(existing) = self.profiles.iter_mut().find(|p| p.name == profile.name) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
    }

    /// name 으로 제거. 제거됐으면 true.
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
    fn field_value_toml_roundtrip_scalar_and_list() {
        // 스칼라 / 리스트가 TOML 의 string / array 로 네이티브 직렬화·역직렬화되는지.
        let mut p = RemoteProfile::new("gx10", "ssh")
            .with_field("host", "gx10")
            .with_field("port", "2222")
            .with_field(
                "extra_options",
                vec![
                    "ServerAliveInterval=30".to_string(),
                    "Compression=yes".to_string(),
                ],
            );
        p.passkey_ref = Some("gx10-key".into());
        let mut ps = RemoteProfiles::default();
        ps.upsert(p);

        let toml_str = toml::to_string_pretty(&ps).unwrap();
        assert!(toml_str.contains("extra_options = ["));
        let back: RemoteProfiles = toml::from_str(&toml_str).unwrap();
        let bp = back.get("gx10").unwrap();
        assert_eq!(bp.kind, "ssh");
        assert_eq!(bp.passkey_ref.as_deref(), Some("gx10-key"));
        assert_eq!(
            bp.fields.get("extra_options").unwrap().as_list().unwrap(),
            &[
                "ServerAliveInterval=30".to_string(),
                "Compression=yes".to_string()
            ]
        );
        assert_eq!(bp.fields.get("host").unwrap().as_str(), Some("gx10"));
    }

    #[test]
    fn ssh_view_reads_fields_with_defaults() {
        let p = RemoteProfile::new("gx", "ssh")
            .with_field("host", "box")
            .with_field("user", "zilhak")
            .with_field("port", "2222");
        let v = p.as_ssh().unwrap();
        assert_eq!(v.host(), Some("box"));
        assert_eq!(v.user(), Some("zilhak"));
        assert_eq!(v.port(), Some(2222));
        assert_eq!(v.ssh_destination(), "zilhak@box");
        // 기본값
        assert_eq!(v.shell(), "auto");
        assert!(v.use_agent()); // 필드 없으면 위임
        assert!(!v.is_disabled());
    }

    #[test]
    fn ssh_view_invalid_port_yields_none() {
        let p = RemoteProfile::new("x", "ssh").with_field("port", "notaport");
        assert_eq!(p.as_ssh().unwrap().port(), None);
    }

    #[test]
    fn ssh_view_destination_respects_existing_user_at_host() {
        let p = RemoteProfile::new("x", "ssh")
            .with_field("host", "root@box")
            .with_field("user", "ignored");
        assert_eq!(p.as_ssh().unwrap().ssh_destination(), "root@box");
    }

    #[test]
    fn ssh_view_detect_failed_disables() {
        let p = RemoteProfile::new("x", "ssh").with_field("detect_failed", "true");
        assert!(p.as_ssh().unwrap().is_disabled());
    }

    #[test]
    fn non_ssh_kind_has_no_ssh_view() {
        let p = RemoteProfile::new("site", "http").with_field("url", "https://x");
        assert!(p.as_ssh().is_none());
    }

    #[test]
    fn extra_options_absorbs_scalar() {
        let p = RemoteProfile::new("x", "ssh").with_field("extra_options", "Compression=yes");
        assert_eq!(
            p.as_ssh().unwrap().extra_options(),
            vec!["Compression=yes".to_string()]
        );
    }

    #[test]
    fn upsert_get_remove() {
        let mut ps = RemoteProfiles::default();
        ps.upsert(RemoteProfile::new("a", "ssh").with_field("host", "h1"));
        ps.upsert(RemoteProfile::new("a", "ssh").with_field("host", "h2")); // 교체
        assert_eq!(ps.profiles.len(), 1);
        assert_eq!(
            ps.get("a").unwrap().fields.get("host").unwrap().as_str(),
            Some("h2")
        );
        assert!(ps.remove("a"));
        assert!(!ps.remove("a"));
        assert!(ps.get("a").is_none());
    }

    #[test]
    fn unknown_kind_still_valid_profile() {
        // 미등록 타입도 정상 저장/로드 (배지 판정은 상위).
        let p = RemoteProfile::new("weird", "asdfasdf").with_field("k", "v");
        assert!(!p.is_builtin_kind());
        let mut ps = RemoteProfiles::default();
        ps.upsert(p);
        let s = toml::to_string_pretty(&ps).unwrap();
        let back: RemoteProfiles = toml::from_str(&s).unwrap();
        assert_eq!(back.get("weird").unwrap().kind, "asdfasdf");
    }

    #[test]
    fn builtin_kinds_membership() {
        assert!(is_builtin_kind("ssh"));
        assert!(is_builtin_kind("tasty-attach"));
        assert!(is_builtin_kind("smb"));
        assert!(!is_builtin_kind("http")); // 플러그인 제공
        assert!(!is_builtin_kind("asdf"));
    }

    #[test]
    fn attach_view_ref_mode() {
        let p = RemoteProfile::new("gb10-attach", "tasty-attach")
            .with_field("ssh_ref", "gb10")
            .with_field("remote_tasty", "/opt/tasty/tasty")
            .with_field("port_file", "/home/z/.tasty/tasty.port");
        let v = p.as_attach().unwrap();
        assert_eq!(v.ssh_ref(), Some("gb10"));
        assert_eq!(v.remote_tasty(), "/opt/tasty/tasty");
        assert_eq!(v.port_file(), Some("/home/z/.tasty/tasty.port"));
        assert_eq!(v.host(), None); // ref 모드 — 인라인 host 없음
        assert_eq!(v.port_mode(), "auto"); // 기본값
    }

    #[test]
    fn attach_view_inline_mode() {
        let p = RemoteProfile::new("box-attach", "tasty-attach")
            .with_field("host", "box")
            .with_field("port", "22")
            .with_field("remote_tasty", "tasty");
        let v = p.as_attach().unwrap();
        assert_eq!(v.ssh_ref(), None);
        assert_eq!(v.host(), Some("box"));
        assert_eq!(v.port(), Some(22));
        assert_eq!(v.ssh_destination(), "box");
        assert_eq!(v.port_mode(), "auto"); // 기본값
        assert_eq!(v.port_file(), None); // 명시 없음
    }

    #[test]
    fn attach_view_ref_ignores_inline_ssh_fields() {
        // ref 모드는 인라인 host 가 실수로 있어도 읽지 않는다(연결은 참조 ssh 소유).
        let p = RemoteProfile::new("mix", "tasty-attach")
            .with_field("ssh_ref", "gb10")
            .with_field("host", "leftover");
        let v = p.as_attach().unwrap();
        assert_eq!(v.host(), None);
        assert_eq!(v.shell(), "auto");
        assert!(v.use_agent());
    }

    #[test]
    fn ssh_kind_has_no_attach_view() {
        let p = RemoteProfile::new("gb10", "ssh").with_field("host", "box");
        assert!(p.as_attach().is_none());
        // 반대로 tasty-attach 는 as_ssh 가 None.
        let a = RemoteProfile::new("gb10-attach", "tasty-attach");
        assert!(a.as_ssh().is_none());
    }
}
