//! Plugin 매니페스트 정의 + 파싱 + 검증.
//!
//! `~/.tasty/plugins/<plugin-id>/tasty-plugin.toml` 형식.
//!
//! 일부 필드(authors/homepage/contributes/icon 등)는 deserialize surface로 정의돼
//! 있지만 호스트 본문이 아직 모두 활용하지는 않는다 — 매니페스트 schema를 한 곳에서
//! 정확히 표현하기 위해 의도적으로 남겨둔다.

#![allow(dead_code)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 호스트가 지원하는 plugin protocol 메이저 버전.
/// plugin 매니페스트의 `api_version`과 일치해야 한다.
pub const HOST_API_VERSION: &str = "1";

/// 매니페스트 스키마 버전 (이 파일 형식 자체의 버전).
pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    pub api_version: String,
    pub entry: Entry,
    #[serde(default)]
    pub surface_kinds: Vec<SurfaceKindDecl>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Entry {
    #[serde(rename = "process")]
    Process {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    // 향후 옵션:
    // #[serde(rename = "wasm")] Wasm { module: String },
}

/// Plugin이 매니페스트에 선언할 수 있는 권한 카테고리.
///
/// 평면 enum — `fs.write`는 `fs.read`를 자동 포함하지 않는다.
/// 매니페스트에 두 권한이 모두 필요하면 명시적으로 선언해야 한다.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Permission {
    /// surface/tab/workspace 트리 조회
    SurfaceRead,
    /// surface 생성/닫기/이동
    SurfaceWrite,
    /// 알림 생성/관리
    Notification,
    /// 클립보드 읽기
    ClipboardRead,
    /// 클립보드 쓰기
    ClipboardWrite,
    /// 호스트 노출 fs 읽기
    FsRead,
    /// 파일 쓰기
    FsWrite,
    /// 외부 프로세스 실행
    ProcessSpawn,
    /// 새 터미널 surface 생성
    TerminalSpawn,
    /// 터미널 입력 송신
    TerminalWrite,
    /// 터미널 출력/scrollback 읽기
    TerminalRead,
    /// 호스트를 통한 네트워크 (예약)
    Network,
    /// Claude 세션 메타데이터 조회 (예약)
    ClaudeRead,
    /// Claude API 호출 위임 (예약)
    ClaudeInvoke,
}

impl Permission {
    pub fn from_token(s: &str) -> Option<Self> {
        Some(match s {
            "surface.read" => Self::SurfaceRead,
            "surface.write" => Self::SurfaceWrite,
            "notification" => Self::Notification,
            "clipboard.read" => Self::ClipboardRead,
            "clipboard.write" => Self::ClipboardWrite,
            "fs.read" => Self::FsRead,
            "fs.write" => Self::FsWrite,
            "process.spawn" => Self::ProcessSpawn,
            "terminal.spawn" => Self::TerminalSpawn,
            "terminal.write" => Self::TerminalWrite,
            "terminal.read" => Self::TerminalRead,
            "network" => Self::Network,
            "claude.read" => Self::ClaudeRead,
            "claude.invoke" => Self::ClaudeInvoke,
            _ => return None,
        })
    }

    pub fn as_token(self) -> &'static str {
        match self {
            Self::SurfaceRead => "surface.read",
            Self::SurfaceWrite => "surface.write",
            Self::Notification => "notification",
            Self::ClipboardRead => "clipboard.read",
            Self::ClipboardWrite => "clipboard.write",
            Self::FsRead => "fs.read",
            Self::FsWrite => "fs.write",
            Self::ProcessSpawn => "process.spawn",
            Self::TerminalSpawn => "terminal.spawn",
            Self::TerminalWrite => "terminal.write",
            Self::TerminalRead => "terminal.read",
            Self::Network => "network",
            Self::ClaudeRead => "claude.read",
            Self::ClaudeInvoke => "claude.invoke",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SurfaceKindDecl {
    pub kind: String,
    pub display_name_i18n_key: String,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<CommandDecl>,
    #[serde(default)]
    pub menu_items: Vec<MenuItemDecl>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommandDecl {
    pub id: String,
    pub title_i18n_key: String,
    #[serde(default)]
    pub default_keybinding: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MenuItemDecl {
    pub menu: String,
    pub command: String,
    #[serde(default)]
    pub when: Option<String>,
}

impl Manifest {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let path = dir.join("tasty-plugin.toml");
        let s = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("cannot read {}: {}", path.display(), e))?;
        let manifest: Manifest = toml::from_str(&s)
            .map_err(|e| anyhow::anyhow!("invalid manifest at {}: {}", path.display(), e))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.manifest_version != MANIFEST_VERSION {
            anyhow::bail!(
                "unsupported manifest_version: {} (expected {})",
                self.manifest_version,
                MANIFEST_VERSION
            );
        }
        if self.api_version != HOST_API_VERSION {
            anyhow::bail!(
                "plugin api_version '{}' incompatible with host '{}'",
                self.api_version,
                HOST_API_VERSION
            );
        }
        if !is_valid_plugin_id(&self.id) {
            anyhow::bail!(
                "invalid plugin id: '{}' (must be lowercase reverse-domain like com.example.x)",
                self.id
            );
        }
        for kind in &self.surface_kinds {
            if !is_valid_kind(&kind.kind) {
                anyhow::bail!(
                    "invalid surface kind: '{}' (must be lowercase ascii + '_' + digits)",
                    kind.kind
                );
            }
        }
        for raw in &self.permissions {
            if Permission::from_token(raw).is_none() {
                anyhow::bail!(
                    "unknown permission '{}' in manifest (host may be older than plugin)",
                    raw
                );
            }
        }
        Ok(())
    }

    /// 매니페스트에 선언된 권한을 파싱한 set으로 반환.
    /// `validate()`가 통과한 매니페스트에 대해 호출되면 절대 실패하지 않는다.
    pub fn parsed_permissions(&self) -> anyhow::Result<HashSet<Permission>> {
        let mut out = HashSet::with_capacity(self.permissions.len());
        for raw in &self.permissions {
            match Permission::from_token(raw) {
                Some(p) => {
                    out.insert(p);
                }
                None => anyhow::bail!("unknown permission '{}'", raw),
            }
        }
        Ok(out)
    }
}

fn is_valid_plugin_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && s.contains('.')
}

fn is_valid_kind(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
}

/// 매니페스트가 들어 있는 디렉터리 핸들 — 디렉터리 + 파싱된 매니페스트 묶음.
#[derive(Debug, Clone)]
pub struct PluginPackage {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl PluginPackage {
    /// 실행할 entry binary의 경로. 매니페스트 디렉터리 기준 상대 경로면
    /// 디렉터리에 합쳐서 반환. 절대 경로 또는 PATH 의존이면 그대로.
    pub fn entry_command_path(&self) -> PathBuf {
        match &self.manifest.entry {
            Entry::Process { command, .. } => {
                let p = Path::new(command);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    let candidate = self.dir.join(command);
                    if candidate.exists() {
                        candidate
                    } else {
                        p.to_path_buf()
                    }
                }
            }
        }
    }

    pub fn entry_args(&self) -> Vec<String> {
        match &self.manifest.entry {
            Entry::Process { args, .. } => args.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> anyhow::Result<Manifest> {
        let m: Manifest = toml::from_str(src)?;
        m.validate()?;
        Ok(m)
    }

    #[test]
    fn rejects_unsupported_api() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "999"
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn rejects_unsupported_manifest_version() {
        let s = r#"
            manifest_version = 99
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn rejects_invalid_plugin_id_no_dot() {
        let s = r#"
            manifest_version = 1
            id = "explorer"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn rejects_invalid_kind_uppercase() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
            [[surface_kinds]]
            kind = "Explorer"
            display_name_i18n_key = "k"
        "#;
        assert!(parse(s).is_err());
    }

    #[test]
    fn accepts_minimal_valid() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1.0"
            api_version = "1"
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.id, "com.example.x");
    }

    #[test]
    fn rejects_unknown_permission() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            permissions = ["fs.read", "made.up.permission"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let err = parse(s).unwrap_err().to_string();
        assert!(err.contains("unknown permission"), "got: {err}");
    }

    #[test]
    fn parsed_permissions_returns_enum_set() {
        let s = r#"
            manifest_version = 1
            id = "com.example.x"
            name = "X"
            version = "0.1"
            api_version = "1"
            permissions = ["fs.read", "surface.write", "notification"]
            [entry]
            type = "process"
            command = "x"
        "#;
        let m = parse(s).expect("should parse");
        let perms = m.parsed_permissions().expect("should resolve");
        assert!(perms.contains(&Permission::FsRead));
        assert!(perms.contains(&Permission::SurfaceWrite));
        assert!(perms.contains(&Permission::Notification));
        assert_eq!(perms.len(), 3);
    }

    #[test]
    fn accepts_full_manifest() {
        // TOML rule: top-level keys must come before any table headers.
        let s = r#"
            manifest_version = 1
            id = "com.example.explorer"
            name = "Explorer"
            version = "1.2.0"
            authors = ["alice@example.com"]
            description = "File explorer"
            homepage = "https://example.com"
            api_version = "1"
            permissions = ["fs.read", "surface.read"]

            [entry]
            type = "process"
            command = "tasty-plugin-explorer"
            args = []

            [[surface_kinds]]
            kind = "explorer"
            display_name_i18n_key = "surface.kind.explorer"
            icon = "📁"

            [[contributes.commands]]
            id = "explorer.refresh"
            title_i18n_key = "explorer.command.refresh"
            default_keybinding = "F5"
        "#;
        let m = parse(s).expect("should parse");
        assert_eq!(m.surface_kinds.len(), 1);
        assert_eq!(m.surface_kinds[0].kind, "explorer");
        assert_eq!(m.permissions.len(), 2);
        assert_eq!(m.contributes.commands.len(), 1);
    }
}
